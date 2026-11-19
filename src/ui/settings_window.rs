use std::sync::{Arc, OnceLock};

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::input::Input;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::component::{Icon, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{self, Locale, tr, tr_shared};
use crate::model::ConfigStore;
use crate::model::settings::{
    DEFAULT_BACKGROUND_OPACITY, LOG_FONT_SIZE_PRESETS, StreamOutputScope,
};
use crate::ui::theme::NanoTheme;
use crate::ui::theme_editor::{FIELDS, ThemeEditor};
use crate::ui::theme_manager::{ThemeHandle, ThemeManager, ThemeSaveError, ThemeSelectError};
use crate::ui::theme_store::ThemeIdError;
use crate::ui::update_manager::{UpdateHandle, UpdateManager, UpdateState};
use crate::ui::workspace::image_format_for_path;

const HEADER_HEIGHT: Pixels = px(38.);
const APP_AUTHOR: &str = "yanggx98";
const GITHUB_URL: &str = "https://github.com/yanggx98/Sasha";

fn theme_save_error_text(locale: Locale, err: &ThemeSaveError) -> String {
    match err {
        ThemeSaveError::InvalidName(ThemeIdError::Empty) => {
            tr(locale, "settings.theme_edit_err_name_empty").into_owned()
        }
        ThemeSaveError::InvalidName(ThemeIdError::Reserved) => {
            tr(locale, "settings.theme_edit_err_name_reserved").into_owned()
        }
        ThemeSaveError::Exists => tr(locale, "settings.theme_edit_err_exists").into_owned(),
        ThemeSaveError::Io(msg) => i18n::theme_edit_failed(locale, msg),
    }
}

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../assets/icons/sasha-arrows-transparent.png").to_vec(),
        ))
    })
    .clone()
}

/// Top-level tabs inside [`SettingsWindow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    View,
    About,
}

impl SettingsTab {
    const ALL: [Self; 3] = [Self::General, Self::View, Self::About];

    fn id(self) -> &'static str {
        match self {
            Self::General => "settings-tab-general",
            Self::View => "settings-tab-view",
            Self::About => "settings-tab-about",
        }
    }

    fn label(self, locale: Locale) -> SharedString {
        match self {
            Self::General => tr_shared(locale, "settings.tab_general"),
            Self::View => tr_shared(locale, "settings.tab_view"),
            Self::About => tr_shared(locale, "settings.tab_about"),
        }
    }

    fn subtitle(self, locale: Locale) -> SharedString {
        match self {
            Self::General => tr_shared(locale, "settings.tab_general_sub"),
            Self::View => tr_shared(locale, "settings.tab_view_sub"),
            Self::About => tr_shared(locale, "settings.tab_about_sub"),
        }
    }

    fn icon(self) -> LucideIcon {
        match self {
            Self::General => LucideIcon::Settings,
            Self::View => LucideIcon::Eye,
            Self::About => LucideIcon::Info,
        }
    }
}

/// Independent settings window with Windows Terminal-style top tabs extending
/// into the transparent titlebar and flat Zed-style setting rows.
pub struct SettingsWindow {
    theme: NanoTheme,
    theme_manager: Entity<ThemeManager>,
    theme_menu_open: bool,
    /// Last theme-folder / theme-selection problem shown in the warnings area.
    theme_notice: Option<String>,
    theme_editor: Option<ThemeEditor>,
    _release_subscription: Subscription,
    _theme_subscription: Subscription,
    update_manager: Entity<UpdateManager>,
    _update_subscription: Subscription,
    config_store: Entity<ConfigStore>,
    active_tab: SettingsTab,
    save_error: Option<String>,
    bg_opacity_slider: Entity<SliderState>,
    _slider_subscription: Subscription,
    _config_subscription: Subscription,
}

impl SettingsWindow {
    pub fn new(
        config_store: Entity<ConfigStore>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&config_store, |_, _, cx| cx.notify());
        let theme_manager = ThemeHandle::get(cx);
        let theme_subscription = cx.observe(&theme_manager, |_, _, cx| cx.notify());
        let release_manager = theme_manager.clone();
        let release_subscription = cx.on_release(move |_, cx| {
            release_manager.update(cx, |manager, cx| manager.clear_preview(cx));
        });
        let update_manager = UpdateHandle::get(cx);
        let update_subscription = cx.observe(&update_manager, |_, _, cx| cx.notify());
        let initial_theme = theme_manager.read(cx).nano();
        let initial_opacity = config_store.read(cx).config().background_opacity as f32;
        let bg_opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(initial_opacity)
        });
        let slider_subscription = cx.subscribe(
            &bg_opacity_slider,
            |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    let pct = value.end().round().clamp(0., 100.) as u8;
                    this.preview_background_opacity(pct, cx);
                }
                SliderEvent::Release(value) => {
                    let pct = value.end().round().clamp(0., 100.) as u8;
                    this.commit_background_opacity(pct, cx);
                }
            },
        );
        Self {
            theme: initial_theme,
            theme_manager,
            theme_menu_open: false,
            theme_notice: None,
            theme_editor: None,
            _release_subscription: release_subscription,
            _theme_subscription: theme_subscription,
            update_manager,
            _update_subscription: update_subscription,
            config_store,
            active_tab: SettingsTab::General,
            save_error: None,
            bg_opacity_slider,
            _slider_subscription: slider_subscription,
            _config_subscription: subscription,
        }
    }

    fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.active_tab != tab {
            self.active_tab = tab;
            cx.notify();
        }
    }

    fn current_locale(&self, cx: &App) -> Locale {
        self.config_store.read(cx).config().locale
    }

    fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.update(|cfg| cfg.locale = locale);
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn set_log_font_size(&mut self, size: u16, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.update(|cfg| cfg.log_font_size = size);
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    /// A color row changed: preview the draft when every row is valid.
    pub(crate) fn theme_editor_changed(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.theme_editor else {
            return;
        };
        editor.delete_armed = false;
        let base = editor.base;
        if let Some(colors) = editor.draft(cx) {
            self.theme_manager
                .update(cx, |manager, cx| manager.preview(base, colors, cx));
        }
        cx.notify();
    }

    fn start_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        let id = self.theme_manager.read(cx).current_id().to_string();
        let Some(entry) = self.theme_manager.read(cx).theme_entry(&id) else {
            return;
        };
        let placeholder = tr_shared(locale, "settings.theme_edit_name_placeholder");
        self.theme_editor = Some(ThemeEditor::new(&entry, placeholder, window, cx));
        self.theme_menu_open = false;
        self.theme_notice = None;
        cx.notify();
    }

    fn close_theme_editor(&mut self, cx: &mut Context<Self>) {
        if self.theme_editor.take().is_some() {
            self.theme_manager
                .update(cx, |manager, cx| manager.clear_preview(cx));
        }
        cx.notify();
    }

    fn save_theme_editor(&mut self, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        let Some(editor) = &self.theme_editor else {
            return;
        };
        let name = editor.name(cx);
        let Some(colors) = editor.draft(cx) else {
            self.theme_notice =
                Some(tr(locale, "settings.theme_edit_err_invalid_color").into_owned());
            cx.notify();
            return;
        };
        let (base, source) = (
            editor.base,
            (!editor.builtin).then(|| editor.source_id.clone()),
        );
        if name.is_empty() {
            self.theme_notice = Some(tr(locale, "settings.theme_edit_err_name_empty").into_owned());
            cx.notify();
            return;
        }
        let result = self.theme_manager.update(cx, |manager, cx| {
            manager.save_theme(source.as_deref(), &name, base, &colors, cx)
        });
        match result {
            Ok(_) => {
                self.theme_editor = None;
                self.theme_notice = None;
            }
            Err(err) => self.theme_notice = Some(theme_save_error_text(locale, &err)),
        }
        cx.notify();
    }

    fn delete_theme_editor(&mut self, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        let Some(editor) = &mut self.theme_editor else {
            return;
        };
        if !editor.delete_armed {
            editor.delete_armed = true;
            cx.notify();
            return;
        }
        let id = editor.source_id.clone();
        let result = self
            .theme_manager
            .update(cx, |manager, cx| manager.delete_theme(&id, cx));
        match result {
            Ok(()) => {
                self.theme_editor = None;
                self.theme_notice = None;
            }
            Err(err) => self.theme_notice = Some(theme_save_error_text(locale, &err)),
        }
        cx.notify();
    }

    fn select_theme(&mut self, id: &str, cx: &mut Context<Self>) {
        self.close_theme_editor(cx);
        let locale = self.current_locale(cx);
        let result = self
            .theme_manager
            .update(cx, |manager, cx| manager.select(id, cx));
        self.theme_notice = None;
        self.save_error = None;
        match result {
            Ok(()) => {}
            Err(ThemeSelectError::Unknown(id)) => {
                self.theme_notice = Some(i18n::theme_unknown(locale, &id));
            }
            Err(ThemeSelectError::Save(err)) => self.save_error = Some(err),
        }
        self.theme_menu_open = false;
        cx.notify();
    }

    fn open_theme_dir(&mut self, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        let dir = self.theme_manager.read(cx).themes_dir().to_path_buf();
        // Recreates a deleted directory and restores watching right away.
        let created = self
            .theme_manager
            .update(cx, |manager, cx| manager.ensure_dir(cx));
        if let Err(err) = created {
            self.theme_notice = Some(i18n::theme_open_failed(locale, &err.to_string()));
            cx.notify();
            return;
        }
        #[cfg(windows)]
        let spawned = std::process::Command::new("explorer").arg(&dir).spawn();
        #[cfg(target_os = "macos")]
        let spawned = std::process::Command::new("open").arg(&dir).spawn();
        #[cfg(not(any(windows, target_os = "macos")))]
        let spawned = std::process::Command::new("xdg-open").arg(&dir).spawn();
        self.theme_notice = spawned
            .err()
            .map(|err| i18n::theme_open_failed(locale, &err.to_string()));
        cx.notify();
    }

    fn reset_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.reset_to_default();
            cx.notify();
            res
        });
        self.bg_opacity_slider.update(cx, |slider, cx| {
            slider.set_value(
                SliderValue::Single(DEFAULT_BACKGROUND_OPACITY as f32),
                window,
                cx,
            );
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn choose_background_image(&mut self, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr_shared(locale, "settings.bg_image_choose")),
        });
        cx.spawn(async move |this, cx| {
            let result = picker.await;
            let _ = this.update(cx, |this, cx| {
                let locale = this.current_locale(cx);
                match result {
                    Ok(Ok(Some(mut paths))) => {
                        let Some(path) = paths.pop() else {
                            return;
                        };
                        if path.is_file() && image_format_for_path(&path).is_some() {
                            let res = this.config_store.update(cx, |store, cx| {
                                let saved = store.update(|cfg| cfg.background_image = Some(path));
                                cx.notify();
                                saved
                            });
                            this.save_error = res.err();
                        } else {
                            this.save_error =
                                Some(tr(locale, "settings.bg_image_invalid").into_owned());
                        }
                    }
                    Ok(Ok(None)) => return,
                    _ => {
                        this.save_error =
                            Some(tr(locale, "settings.bg_image_load_failed").into_owned());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn clear_background_image(&mut self, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.update(|cfg| cfg.background_image = None);
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn preview_background_opacity(&mut self, opacity: u8, cx: &mut Context<Self>) {
        self.config_store.update(cx, |store, cx| {
            store.update_in_memory(|cfg| cfg.background_opacity = opacity);
            cx.notify();
        });
        cx.notify();
    }

    fn commit_background_opacity(&mut self, opacity: u8, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.update(|cfg| cfg.background_opacity = opacity);
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn set_stream_scope(&mut self, scope: StreamOutputScope, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let result = store.update(|cfg| cfg.stream_output_scope = scope);
            cx.notify();
            result
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn set_auto_check_updates(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.update(|cfg| cfg.auto_check_updates = enabled);
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    /// Status line, detail and action buttons for the About page's update area.
    fn render_update_status(&self, locale: Locale, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme;
        let state = self.update_manager.read(cx).state().clone();
        let (status, buttons): (String, Vec<AnyElement>) = match &state {
            UpdateState::Idle => (String::new(), vec![self.update_check_button(locale, cx)]),
            UpdateState::Checking => (tr(locale, "update.checking").into_owned(), Vec::new()),
            UpdateState::UpToDate => (
                tr(locale, "update.up_to_date").into_owned(),
                vec![self.update_check_button(locale, cx)],
            ),
            UpdateState::Available(release) => {
                let mut buttons = Vec::new();
                if release.asset.is_some() {
                    buttons.push(self.flat_action_button(
                        "settings-update-install",
                        tr_shared(locale, "update.install"),
                        |this, _, cx| {
                            this.update_manager
                                .update(cx, |manager, cx| manager.download_and_install(cx));
                        },
                        cx,
                    ));
                }
                buttons.push(self.update_notes_button(locale, release.page_url.clone(), cx));
                (
                    i18n::update_available(locale, &release.version.to_string()),
                    buttons,
                )
            }
            UpdateState::Downloading(release) => (
                i18n::update_downloading(locale, &release.version.to_string()),
                Vec::new(),
            ),
            UpdateState::Installed(release) => (
                i18n::update_installed(locale, &release.version.to_string()),
                vec![self.flat_action_button(
                    "settings-update-restart",
                    tr_shared(locale, "update.restart"),
                    |this, _, cx| {
                        this.update_manager
                            .update(cx, |manager, cx| manager.restart(cx));
                    },
                    cx,
                )],
            ),
            UpdateState::Failed(detail, release) => {
                let mut buttons = vec![self.update_check_button(locale, cx)];
                if let Some(release) = release {
                    buttons.push(self.update_notes_button(locale, release.page_url.clone(), cx));
                }
                (i18n::update_failed(locale, detail), buttons)
            }
        };

        v_flex()
            .items_center()
            .gap_2()
            .when(!status.is_empty(), |this| {
                this.child(
                    div()
                        .max_w(px(360.))
                        .text_center()
                        .text_color(theme.text_secondary)
                        .child(status),
                )
            })
            .when(!buttons.is_empty(), |this| {
                this.child(h_flex().gap_1p5().children(buttons))
            })
            .into_any_element()
    }

    fn update_check_button(&self, locale: Locale, cx: &mut Context<Self>) -> AnyElement {
        self.flat_action_button(
            "settings-update-check",
            tr_shared(locale, "update.check"),
            |this, _, cx| {
                this.update_manager
                    .update(cx, |manager, cx| manager.check(cx));
            },
            cx,
        )
    }

    fn update_notes_button(
        &self,
        locale: Locale,
        url: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.flat_action_button(
            "settings-update-notes",
            tr_shared(locale, "update.release_notes"),
            move |_, _, cx| cx.open_url(&url),
            cx,
        )
    }

    fn copy_text(&self, text: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    }

    fn render_tab_button(&self, tab: SettingsTab, cx: &mut Context<Self>) -> AnyElement {
        let is_active = self.active_tab == tab;
        let theme = self.theme;
        let locale = self.current_locale(cx);

        h_flex()
            .id(tab.id())
            .min_w(px(108.))
            .px_3()
            .items_center()
            .gap_1p5()
            .text_xs()
            .cursor_pointer()
            // Active tab connects seamlessly to the content canvas below (Windows Terminal style).
            .when(is_active, |this| {
                this.h(px(32.))
                    .rounded_t(px(8.))
                    .bg(theme.bg_canvas)
                    .text_color(theme.text_primary)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!is_active, |this| {
                this.h(px(28.))
                    .mb(px(4.))
                    .rounded(px(6.))
                    .text_color(theme.text_secondary)
                    .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.select_tab(tab, cx);
                }),
            )
            .child(Icon::new(tab.icon()).size(px(12.)))
            .child(tab.label(locale))
            .into_any_element()
    }

    fn render_section_header(&self, tab: SettingsTab, locale: Locale) -> impl IntoElement {
        v_flex()
            .w_full()
            .pb_3()
            .gap_1()
            .border_b_1()
            .border_color(self.theme.border_card)
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.text_primary)
                    .child(tab.label(locale)),
            )
            .child(
                div()
                    .font_family(crate::ui::fonts::MONO_FONT)
                    .text_xs()
                    .text_color(self.theme.text_muted)
                    .child(tab.subtitle(locale)),
            )
    }

    fn render_setting_row(
        &self,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        control: AnyElement,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .py_4()
            .items_center()
            .justify_between()
            .gap_6()
            .border_b_1()
            .border_color(self.theme.border_card)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.text_primary)
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.text_secondary)
                            .child(description.into()),
                    ),
            )
            .child(div().flex_shrink_0().child(control))
            .into_any_element()
    }

    fn flat_action_button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        div()
            .id(id)
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .text_xs()
            .text_color(theme.text_primary)
            .hover(|s| s.bg(theme.hover_over(theme.bg_card)))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    on_click(this, window, cx);
                }),
            )
            .child(label.into())
            .into_any_element()
    }

    /// Inline color editor under the theme row (`None` when not editing).
    fn render_theme_editor(&self, locale: Locale, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.theme_editor.as_ref()?;
        let theme = self.theme;
        let parsed = editor.parsed(cx);
        let rows = FIELDS
            .iter()
            .zip(&editor.inputs)
            .zip(&editor.pickers)
            .zip(&parsed)
            .map(|((((label_key, _), input), picker), value)| {
                let valid = value.is_ok();
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(110.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(theme.text_secondary)
                            .child(tr_shared(locale, label_key)),
                    )
                    .child(ColorPicker::new(picker).small())
                    .child(
                        div()
                            .w(px(140.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .px_1p5()
                            .rounded(px(6.))
                            .bg(theme.bg_card)
                            .border_1()
                            .border_color(if valid {
                                theme.border_card
                            } else {
                                theme.error_text
                            })
                            .font_family(crate::ui::fonts::MONO_FONT)
                            .text_xs()
                            .text_color(theme.text_primary)
                            .child(Input::new(input).appearance(false)),
                    )
                    .into_any_element()
            });

        let name_row = div()
            .w(px(260.))
            .h(px(22.))
            .flex()
            .items_center()
            .px_1p5()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .border_1()
            .border_color(theme.border_card)
            .text_xs()
            .text_color(theme.text_primary)
            .child(Input::new(&editor.name_input).appearance(false));

        let save_label = if editor.builtin {
            "settings.theme_edit_save_as"
        } else {
            "settings.theme_edit_save"
        };
        let mut actions = h_flex().gap_2().child(self.flat_action_button(
            "settings-theme-edit-save",
            tr_shared(locale, save_label),
            |this, _, cx| this.save_theme_editor(cx),
            cx,
        ));
        if !editor.builtin {
            actions = actions.child(self.flat_action_button(
                "settings-theme-edit-delete",
                tr_shared(
                    locale,
                    if editor.delete_armed {
                        "settings.theme_edit_delete_confirm"
                    } else {
                        "settings.theme_edit_delete"
                    },
                ),
                |this, _, cx| this.delete_theme_editor(cx),
                cx,
            ));
        }

        Some(
            v_flex()
                .w_full()
                .py_3()
                .gap_2()
                .border_b_1()
                .border_color(theme.border_card)
                .children(editor.builtin.then(|| {
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(tr_shared(locale, "settings.theme_edit_builtin_hint"))
                }))
                .children(rows)
                .child(name_row)
                .child(actions)
                .into_any_element(),
        )
    }

    fn render_general_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.current_locale(cx);
        let theme = self.theme;
        let path_text = self.config_store.read(cx).path().display().to_string();

        let language_picker = h_flex()
            .p_0p5()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .gap_0p5()
            .children(Locale::ALL.into_iter().enumerate().map(|(idx, candidate)| {
                let is_selected = locale == candidate;
                div()
                    .id(ElementId::named_usize("settings-locale", idx))
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.))
                    .text_xs()
                    .cursor_pointer()
                    .when(is_selected, |this| {
                        this.bg(theme.bg_toggle_active)
                            .text_color(theme.accent_blue_hover)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!is_selected, |this| {
                        this.text_color(theme.text_secondary)
                            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_locale(candidate, cx);
                        }),
                    )
                    .child(candidate.native_name())
            }))
            .into_any_element();

        let copy_btn = self.flat_action_button(
            "settings-copy-config-path",
            tr_shared(locale, "settings.copy_path"),
            |this, _, cx| {
                let path = this.config_store.read(cx).path().display().to_string();
                this.copy_text(&path, cx);
            },
            cx,
        );

        let selected_scope = self.config_store.read(cx).config().stream_output_scope;
        let scope_picker = v_flex()
            .gap_1()
            .children(
                [
                    (
                        StreamOutputScope::CurrentBufferAndFuture,
                        "settings.stream_scope_buffer",
                    ),
                    (
                        StreamOutputScope::FutureOnly,
                        "settings.stream_scope_future",
                    ),
                ]
                .into_iter()
                .enumerate()
                .map(|(idx, (candidate, key))| {
                    let selected = candidate == selected_scope;
                    div()
                        .id(ElementId::named_usize("settings-stream-scope", idx))
                        .px_2p5()
                        .py_1()
                        .rounded(px(4.))
                        .text_xs()
                        .cursor_pointer()
                        .when(selected, |this| {
                            this.bg(theme.bg_toggle_active)
                                .text_color(theme.accent_blue_hover)
                                .font_weight(FontWeight::MEDIUM)
                        })
                        .when(!selected, |this| {
                            this.text_color(theme.text_secondary).hover(|style| {
                                style.bg(theme.bg_hover).text_color(theme.text_primary)
                            })
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.set_stream_scope(candidate, cx);
                            }),
                        )
                        .child(tr_shared(locale, key))
                }),
            )
            .into_any_element();

        let auto_check = self.config_store.read(cx).config().auto_check_updates;
        let auto_check_picker = h_flex()
            .p_0p5()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .gap_0p5()
            .children(
                [(true, "settings.on"), (false, "settings.off")]
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (value, key))| {
                        let selected = auto_check == value;
                        div()
                            .id(ElementId::named_usize("settings-auto-update", idx))
                            .px_2p5()
                            .py_1()
                            .rounded(px(4.))
                            .text_xs()
                            .cursor_pointer()
                            .when(selected, |this| {
                                this.bg(theme.bg_toggle_active)
                                    .text_color(theme.accent_blue_hover)
                                    .font_weight(FontWeight::MEDIUM)
                            })
                            .when(!selected, |this| {
                                this.text_color(theme.text_secondary)
                                    .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.set_auto_check_updates(value, cx);
                                }),
                            )
                            .child(tr_shared(locale, key))
                    }),
            )
            .into_any_element();

        let reset_btn = self.flat_action_button(
            "settings-reset-defaults",
            tr_shared(locale, "settings.reset_btn"),
            |this, window, cx| this.reset_defaults(window, cx),
            cx,
        );

        v_flex()
            .w_full()
            .child(self.render_section_header(SettingsTab::General, locale))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.language_title"),
                tr_shared(locale, "settings.language_desc"),
                language_picker,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.stream_scope_title"),
                tr_shared(locale, "settings.stream_scope_desc"),
                scope_picker,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.auto_update_title"),
                tr_shared(locale, "settings.auto_update_desc"),
                auto_check_picker,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.config_path_title"),
                path_text,
                copy_btn,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.reset_title"),
                tr_shared(locale, "settings.reset_desc"),
                reset_btn,
            ))
    }

    fn render_view_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.current_locale(cx);
        let (current_size, bg_image, bg_opacity) = {
            let cfg = self.config_store.read(cx).config();
            (
                cfg.log_font_size,
                cfg.background_image.clone(),
                cfg.background_opacity,
            )
        };
        let has_bg_image = bg_image.is_some();
        let bg_name: SharedString = bg_image
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned().into())
            .unwrap_or_else(|| tr_shared(locale, "settings.bg_image_none"));
        let theme = self.theme;

        let font_size_picker = h_flex()
            .p_0p5()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .gap_0p5()
            .children(LOG_FONT_SIZE_PRESETS.into_iter().map(|preset| {
                let is_selected = current_size == preset;
                div()
                    .id(ElementId::named_usize(
                        "settings-font-size",
                        preset as usize,
                    ))
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.))
                    .text_xs()
                    .cursor_pointer()
                    .when(is_selected, |this| {
                        this.bg(theme.bg_toggle_active)
                            .text_color(theme.accent_blue_hover)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!is_selected, |this| {
                        this.text_color(theme.text_secondary)
                            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_log_font_size(preset, cx);
                        }),
                    )
                    .child(format!("{preset}px"))
            }))
            .into_any_element();

        let (theme_infos, theme_selected, theme_current, theme_dir, theme_messages) = {
            let manager = self.theme_manager.read(cx);
            (
                manager.themes(),
                manager.selected_id().to_string(),
                manager.current_id().to_string(),
                manager.themes_dir().display().to_string(),
                manager.messages(),
            )
        };
        let builtin_label = tr_shared(locale, "settings.theme_builtin");
        let current_name: SharedString = theme_infos
            .iter()
            .find(|info| info.id == theme_current)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| theme_current.clone())
            .into();
        let theme_fell_back = theme_selected != theme_current;
        let theme_desc: SharedString = if theme_fell_back {
            i18n::theme_status_fallback(locale, &current_name, &theme_selected).into()
        } else {
            tr_shared(locale, "settings.theme_desc")
        };
        let menu_open = self.theme_menu_open;
        let theme_button = h_flex()
            .id("settings-theme-button")
            .items_center()
            .gap_1p5()
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .hover(|s| s.bg(theme.hover_over(theme.bg_card)))
            .cursor_pointer()
            .text_xs()
            .text_color(theme.text_primary)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.theme_menu_open = !this.theme_menu_open;
                    cx.notify();
                }),
            )
            .child(current_name)
            .child(
                Icon::new(if menu_open {
                    LucideIcon::ChevronUp
                } else {
                    LucideIcon::ChevronDown
                })
                .size(px(12.)),
            )
            .into_any_element();
        let theme_menu = menu_open.then(|| {
            v_flex()
                .w_full()
                .py_1()
                .gap_0p5()
                .border_b_1()
                .border_color(theme.border_card)
                .children(theme_infos.iter().enumerate().map(|(index, info)| {
                    let id = info.id.clone();
                    let is_current = info.id == theme_current;
                    h_flex()
                        .id(ElementId::named_usize("settings-theme-item", index))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .gap_2()
                        .items_center()
                        .rounded(px(6.))
                        .text_xs()
                        .cursor_pointer()
                        .when(is_current, |this| {
                            this.bg(theme.bg_toggle_active)
                                .text_color(theme.accent_blue_hover)
                                .font_weight(FontWeight::MEDIUM)
                        })
                        .when(!is_current, |this| {
                            this.text_color(theme.text_secondary)
                                .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.select_theme(&id, cx);
                            }),
                        )
                        .child(div().flex_1().truncate().child(info.name.clone()))
                        .when(info.builtin, |this| {
                            this.child(
                                div()
                                    .text_color(theme.text_muted)
                                    .child(builtin_label.clone()),
                            )
                        })
                        .into_any_element()
                }))
                .into_any_element()
        });

        let theme_dir_button = self.flat_action_button(
            "settings-theme-open-dir",
            tr_shared(locale, "settings.theme_open_dir"),
            |this, _, cx| this.open_theme_dir(cx),
            cx,
        );
        let theme_notice = self.theme_notice.clone();
        let theme_warnings = (!theme_messages.is_empty() || theme_notice.is_some()).then(|| {
            v_flex()
                .w_full()
                .py_3()
                .gap_1()
                .border_b_1()
                .border_color(theme.border_card)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.error_text)
                        .child(tr_shared(locale, "settings.theme_warnings_title")),
                )
                .children(
                    theme_notice
                        .map(|notice| div().text_xs().text_color(theme.error_text).child(notice)),
                )
                .children(theme_messages.iter().map(|message| {
                    div()
                        .text_xs()
                        .text_color(theme.text_secondary)
                        .child(i18n::theme_message(locale, message))
                }))
                .into_any_element()
        });

        let bg_image_controls = h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .max_w(px(160.))
                    .truncate()
                    .text_xs()
                    .text_color(if has_bg_image {
                        theme.text_primary
                    } else {
                        theme.text_muted
                    })
                    .child(bg_name),
            )
            .child(self.flat_action_button(
                "settings-bg-choose",
                tr_shared(locale, "settings.bg_image_choose"),
                |this, _, cx| this.choose_background_image(cx),
                cx,
            ))
            .when(has_bg_image, |this| {
                this.child(self.flat_action_button(
                    "settings-bg-clear",
                    tr_shared(locale, "settings.bg_image_clear"),
                    |this, _, cx| this.clear_background_image(cx),
                    cx,
                ))
            })
            .into_any_element();

        let bg_opacity_controls = h_flex()
            .items_center()
            .gap_3()
            .child(
                div().w(px(160.)).child(
                    Slider::new(&self.bg_opacity_slider)
                        .horizontal()
                        .disabled(!has_bg_image),
                ),
            )
            .child(
                div()
                    .w(px(40.))
                    .text_right()
                    .font_family(crate::ui::fonts::MONO_FONT)
                    .text_xs()
                    .text_color(if has_bg_image {
                        theme.text_primary
                    } else {
                        theme.text_muted
                    })
                    .child(format!("{bg_opacity}%")),
            )
            .into_any_element();

        let editing = self.theme_editor.is_some();
        let edit_button = self.flat_action_button(
            "settings-theme-edit",
            tr_shared(
                locale,
                if editing {
                    "settings.theme_edit_done"
                } else {
                    "settings.theme_edit"
                },
            ),
            move |this, window, cx| {
                if this.theme_editor.is_some() {
                    this.close_theme_editor(cx);
                } else {
                    this.start_theme_editor(window, cx);
                }
            },
            cx,
        );
        let theme_controls = h_flex()
            .items_center()
            .gap_2()
            .child(edit_button)
            .child(theme_button)
            .into_any_element();
        let theme_editor_panel = self.render_theme_editor(locale, cx);

        v_flex()
            .w_full()
            .child(self.render_section_header(SettingsTab::View, locale))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.font_size_title"),
                tr_shared(locale, "settings.font_size_desc"),
                font_size_picker,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.theme_title"),
                theme_desc,
                theme_controls,
            ))
            .children(theme_menu)
            .children(theme_editor_panel)
            .child(self.render_setting_row(
                tr_shared(locale, "settings.theme_dir_title"),
                i18n::theme_dir_desc(locale, &theme_dir),
                theme_dir_button,
            ))
            .children(theme_warnings)
            .child(self.render_setting_row(
                tr_shared(locale, "settings.bg_image_title"),
                tr_shared(locale, "settings.bg_image_desc"),
                bg_image_controls,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.bg_opacity_title"),
                tr_shared(locale, "settings.bg_opacity_desc"),
                bg_opacity_controls,
            ))
    }

    fn render_about_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.current_locale(cx);
        let version = format!("v{}", env!("CARGO_PKG_VERSION"));
        let copy_github_btn = self.flat_action_button(
            "settings-copy-github",
            tr_shared(locale, "settings.copy_url"),
            |this, _, cx| this.copy_text(GITHUB_URL, cx),
            cx,
        );
        let open_github_btn = self.flat_action_button(
            "settings-open-github",
            tr_shared(locale, "settings.open_repo"),
            |_, _, cx| cx.open_url(GITHUB_URL),
            cx,
        );
        let github_actions = h_flex()
            .justify_center()
            .gap_1p5()
            .child(open_github_btn)
            .child(copy_github_btn)
            .into_any_element();

        v_flex()
            .w_full()
            .child(
                v_flex()
                    .id("settings-about-brand")
                    .w_full()
                    .items_center()
                    .pt_2()
                    .pb_5()
                    .gap_1()
                    .child(img(app_icon()).size(px(64.)).flex_shrink_0())
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.text_primary)
                            .child("Sasha"),
                    )
                    .child(
                        div()
                            .font_family(crate::ui::fonts::MONO_FONT)
                            .text_xs()
                            .text_color(self.theme.text_muted)
                            .child(version),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .pt_2()
                    .gap_3()
                    .text_xs()
                    .text_color(self.theme.text_muted)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(tr(locale, "settings.built_with"))
                            .child(self.about_project_link(
                                "about-gpui-kit",
                                "GPUI-KIT",
                                "https://github.com/longbridge/gpui-kit",
                            ))
                            .child("·")
                            .child(self.about_project_link(
                                "about-gpui",
                                "GPUI",
                                "https://github.com/zed-industries/zed/tree/main/crates/gpui",
                            )),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(tr(locale, "settings.ai_assisted"))
                            .child("DeepSeek · Grok · ChatGPT · Gemini · Claude"),
                    )
                    .child(div().child(format!(
                        "{} {}",
                        tr(locale, "settings.created_by"),
                        APP_AUTHOR
                    )))
                    .child(div().mt_3().child(github_actions))
                    .child(div().mt_2().child(self.render_update_status(locale, cx))),
            )
    }

    fn about_project_link(
        &self,
        id: &'static str,
        label: &'static str,
        url: &'static str,
    ) -> AnyElement {
        div()
            .id(id)
            .cursor_pointer()
            .text_color(self.theme.text_secondary)
            .hover(|style| style.text_color(self.theme.accent_blue_hover))
            .on_click(move |_, _, cx| cx.open_url(url))
            .child(label)
            .into_any_element()
    }

    fn build_top_bar(&self) -> impl IntoElement {
        div()
            .id("settings-top-bar-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(34.))
            .occlude()
            .cursor_default()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                gpui_kit::base::GlobalState::suppress_text_selection(cx);
            })
            .child(
                TitleBar::new()
                    .bg(gpui_kit::transparent_black())
                    .border_b_0(),
            )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.theme = self.theme_manager.read(cx).nano();
        let theme = self.theme;
        let locale = self.current_locale(cx);
        let save_error = self.save_error.clone();

        v_flex()
            .id("settings-window-root")
            .relative()
            .size_full()
            .bg(theme.bg_canvas)
            .text_color(theme.text_primary)
            // Clicks that no control handled (controls stop propagation) close the
            // theme dropdown.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.theme_menu_open {
                        this.theme_menu_open = false;
                        cx.notify();
                    }
                }),
            )
            .on_key_down(|event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    window.remove_window();
                }
            })
            // Recessed Windows Terminal-style tab strip extending to the top edge.
            .child(
                div()
                    .id("settings-header-surface")
                    .flex_shrink_0()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .bg(theme.bg_strip),
            )
            // Scrollable borderless settings content area (Zed-style flat rows).
            .child(
                v_flex()
                    .id("settings-content")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .px_6()
                    .py_5()
                    .when_some(save_error, |this, err| {
                        this.child(
                            div()
                                .w_full()
                                .mb_3()
                                .px_3()
                                .py_2()
                                .rounded(px(6.))
                                .bg(theme.error_bg)
                                .text_xs()
                                .text_color(theme.error_text)
                                .child(i18n::settings_save_error(locale, &err)),
                        )
                    })
                    .child(match self.active_tab {
                        SettingsTab::General => self.render_general_tab(cx).into_any_element(),
                        SettingsTab::View => self.render_view_tab(cx).into_any_element(),
                        SettingsTab::About => self.render_about_tab(cx).into_any_element(),
                    }),
            )
            // Transparent TitleBar overlay for window dragging and native window controls.
            .child(self.build_top_bar())
            // Interactive tabs aligned to the bottom of the header strip so the active tab
            // merges into the content canvas below.
            .child(
                h_flex()
                    .id("settings-top-tabs")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right(px(140.))
                    .h(HEADER_HEIGHT)
                    .pl_2()
                    .pt(px(6.))
                    .items_end()
                    .gap_1()
                    .children(
                        SettingsTab::ALL
                            .into_iter()
                            .map(|tab| self.render_tab_button(tab, cx)),
                    ),
            )
    }
}
