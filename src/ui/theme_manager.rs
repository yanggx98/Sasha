//! Application-level theme runtime: owns the loaded themes, applies the current
//! one to gpui's global `Theme`, persists the selection and watches the themes
//! directory. Pure logic lives in `theme_store`; this file is the gpui glue.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use notify::{EventKind, RecursiveMode, Watcher};

use crate::model::ConfigStore;
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::{
    Debouncer, GpuiThemeColors, ThemeBase, ThemeMessage, ThemeState, WatchHealth,
    gpui_theme_colors, is_toml_path, plan_watch, surface_opacity,
};

const THEMES_DIR_NAME: &str = "themes";
const DEBOUNCE_QUIET: Duration = Duration::from_millis(250);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(3);

/// `themes/` next to the config file.
pub fn themes_dir_for(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(THEMES_DIR_NAME)
}

/// Why [`ThemeManager::select`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSelectError {
    /// No loaded theme has this id.
    Unknown(String),
    /// The theme was applied but persisting the choice failed.
    Save(String),
}

/// Row for the settings dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub builtin: bool,
}

/// Global handle so any window can reach the shared manager.
#[derive(Clone)]
pub struct ThemeHandle(pub Entity<ThemeManager>);

impl Global for ThemeHandle {}

impl ThemeHandle {
    pub fn get(cx: &App) -> Entity<ThemeManager> {
        cx.global::<ThemeHandle>().0.clone()
    }
}

pub struct ThemeManager {
    config_store: Entity<ConfigStore>,
    state: ThemeState,
    applied_base: Option<ThemeBase>,
    /// Watcher events: `true` when the event concerns the directory itself.
    watch_rx: Option<Receiver<bool>>,
    _watcher: Option<notify::RecommendedWatcher>,
    debouncer: Debouncer,
    /// Directory creation problems (non-fatal).
    setup_warnings: Vec<ThemeMessage>,
    /// Why directory watching is currently unavailable, if it is.
    watch_error: Option<String>,
    /// A directory-level event is waiting to be processed.
    dir_event_pending: bool,
    last_health_check: Instant,
    /// Last failure to persist the selected theme.
    save_error: Option<String>,
    _poll_task: Option<Task<()>>,
}

impl ThemeManager {
    /// Creates the manager, ensures the themes directory exists, applies the
    /// configured theme to the gpui global `Theme`, starts watching and
    /// registers the [`ThemeHandle`] global. Call once at startup.
    pub fn init(config_store: Entity<ConfigStore>, cx: &mut App) -> Entity<Self> {
        let (dir, selected) = {
            let store = config_store.read(cx);
            (themes_dir_for(store.path()), store.config().theme.clone())
        };
        let mut setup_warnings = Vec::new();
        if let Err(err) = std::fs::create_dir_all(&dir) {
            setup_warnings.push(ThemeMessage::DirCreateFailed {
                path: dir.display().to_string(),
                error: err.to_string(),
            });
        }
        let state = ThemeState::load(&dir, &selected);

        let entity = cx.new(|cx| {
            let mut manager = Self {
                config_store,
                state,
                applied_base: None,
                watch_rx: None,
                _watcher: None,
                debouncer: Debouncer::new(DEBOUNCE_QUIET),
                setup_warnings,
                watch_error: None,
                dir_event_pending: false,
                last_health_check: Instant::now(),
                save_error: None,
                _poll_task: None,
            };
            manager.start_watching();
            manager.apply_global(cx);
            manager._poll_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                loop {
                    cx.background_executor().timer(POLL_INTERVAL).await;
                    if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                        break;
                    }
                }
            }));
            manager
        });
        cx.set_global(ThemeHandle(entity.clone()));
        entity
    }

    /// (Re)creates the directory watcher; on failure records why and leaves
    /// watching off so a later health check can retry.
    fn start_watching(&mut self) {
        let dir = self.state.dir().to_path_buf();
        let watched = dir.clone();
        let (tx, rx) = mpsc::channel::<bool>();
        let result = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && !matches!(event.kind, EventKind::Access(_))
            {
                let dir_event = event.paths.iter().any(|p| p == &watched);
                if dir_event || event.paths.iter().any(|p| is_toml_path(p)) {
                    let _ = tx.send(dir_event);
                }
            }
        })
        .and_then(|mut watcher| {
            watcher.watch(&dir, RecursiveMode::NonRecursive)?;
            Ok(watcher)
        });
        match result {
            Ok(watcher) => {
                self._watcher = Some(watcher);
                self.watch_rx = Some(rx);
                self.watch_error = None;
            }
            Err(err) => {
                self._watcher = None;
                self.watch_rx = None;
                let msg = err.to_string();
                if self.watch_error.as_deref() != Some(msg.as_str()) {
                    eprintln!("Theme directory watching unavailable: {msg}");
                }
                self.watch_error = Some(msg);
            }
        }
    }

    /// Drains watcher events, rescans after the debounce window and, every few
    /// seconds, checks that the directory and its watcher are still healthy.
    fn poll(&mut self, cx: &mut Context<Self>) {
        if let Some(rx) = &self.watch_rx {
            while let Ok(dir_event) = rx.try_recv() {
                self.debouncer.note(Instant::now());
                self.dir_event_pending |= dir_event;
            }
        }
        let now = Instant::now();
        let events_due = self.debouncer.poll(now);
        let periodic =
            now.saturating_duration_since(self.last_health_check) >= HEALTH_CHECK_INTERVAL;
        if periodic {
            self.last_health_check = now;
        }
        if events_due || periodic {
            self.check_watch(events_due, cx);
        }
    }

    /// Applies [`plan_watch`]: drop/rebuild the watcher and rescan as needed.
    fn check_watch(&mut self, events_due: bool, cx: &mut Context<Self>) {
        let plan = plan_watch(WatchHealth {
            dir_exists: self.state.dir().is_dir(),
            watching: self._watcher.is_some(),
            dir_event: std::mem::take(&mut self.dir_event_pending),
            events_due,
        });
        if plan.drop_watcher {
            self._watcher = None;
            self.watch_rx = None;
        }
        let before_error = self.watch_error.clone();
        if plan.rebuild_watcher {
            self.start_watching();
        }
        let mut changed = self.watch_error != before_error;
        if plan.rescan && self.state.rescan() {
            self.apply_global(cx);
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// Creates the themes directory if needed and restores watching at once
    /// (used by the settings page "open folder" button).
    pub fn ensure_dir(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        std::fs::create_dir_all(self.state.dir())?;
        self.check_watch(false, cx);
        Ok(())
    }

    /// Writes the current theme into gpui's global `Theme`: change the mode
    /// first (only when it flips or on first use), then one `Theme::update`.
    fn apply_global(&mut self, cx: &mut App) {
        let entry = self.state.current();
        let colors = gpui_theme_colors(&entry.theme, entry.base);
        if self.applied_base != Some(entry.base) {
            let mode = if entry.base.is_dark() {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            Theme::change(mode, None, cx);
            self.applied_base = Some(entry.base);
        }
        Theme::update(cx, |theme| write_colors(theme, &colors));
        cx.refresh_windows();
    }

    // ------------------------------------------------ interface for UI

    /// Tokens of the theme currently in effect.
    pub fn nano(&self) -> NanoTheme {
        self.state.current().theme
    }

    /// Light/dark family of the theme in effect.
    pub fn base(&self) -> ThemeBase {
        self.state.current().base
    }

    /// Alpha for surfaces drawn over a background image (see `surface_opacity`).
    pub fn surface_opacity(&self) -> f32 {
        surface_opacity(self.base())
    }

    /// Selectable themes: built-ins first, then custom sorted by id.
    pub fn themes(&self) -> Vec<ThemeInfo> {
        self.state
            .themes()
            .iter()
            .map(|t| ThemeInfo {
                id: t.id.clone(),
                name: t.name.clone(),
                builtin: t.builtin,
            })
            .collect()
    }

    /// Configured theme id (may be missing on disk; see [`Self::current_id`]).
    pub fn selected_id(&self) -> &str {
        self.state.selected_id()
    }

    /// Id of the theme actually in effect (`dark` after a fallback).
    pub fn current_id(&self) -> &str {
        &self.state.current().id
    }

    pub fn themes_dir(&self) -> &Path {
        self.state.dir()
    }

    /// Recent warnings/errors: save failure, setup/watch problems, fallback
    /// notice, then per-file parse errors. Empty when everything is fine.
    pub fn messages(&self) -> Vec<ThemeMessage> {
        let mut out = Vec::new();
        out.extend(self.save_error.clone().map(ThemeMessage::SaveFailed));
        out.extend(self.setup_warnings.iter().cloned());
        out.extend(self.watch_error.clone().map(ThemeMessage::WatchUnavailable));
        out.extend(self.state.messages());
        out
    }

    /// Selects a theme: applies it immediately and persists the id.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), ThemeSelectError> {
        if !self.state.select(id) {
            return Err(ThemeSelectError::Unknown(id.to_string()));
        }
        self.apply_global(cx);
        let id = id.to_string();
        let result = self.config_store.update(cx, |store, cx| {
            let result = store.update(|cfg| cfg.theme = id);
            cx.notify();
            result
        });
        self.save_error = result.clone().err();
        cx.notify();
        result.map_err(ThemeSelectError::Save)
    }
}

fn write_colors(theme: &mut Theme, c: &GpuiThemeColors) {
    theme.background = c.background;
    theme.foreground = c.foreground;
    theme.input = c.input;
    theme.border = c.border;
    theme.primary = c.primary;
    theme.primary_hover = c.primary_hover;
    theme.primary_active = c.primary_active;
    theme.primary_foreground = c.primary_foreground;
    theme.muted = c.muted;
    theme.muted_foreground = c.muted_foreground;
    theme.popover = c.popover;
    theme.popover_foreground = c.popover_foreground;
    theme.caret = c.caret;
    theme.ring = c.ring;
    theme.selection = c.selection;
    theme.accent = c.accent;
    theme.accent_foreground = c.accent_foreground;
    theme.link = c.link;
    theme.scrollbar = c.scrollbar;
    theme.scrollbar_thumb = c.scrollbar_thumb;
    theme.scrollbar_thumb_hover = c.scrollbar_thumb_hover;
    theme.danger = c.danger;
    theme.danger_hover = c.danger_hover;
    theme.danger_active = c.danger_active;
    theme.danger_foreground = c.danger_foreground;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn themes_dir_is_next_to_config_file() {
        let dir = themes_dir_for(Path::new("C:/app/config.toml"));
        assert_eq!(dir, Path::new("C:/app").join("themes"));
    }
}
