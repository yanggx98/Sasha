use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use gpui_kit::base::TextSelection;
use gpui_kit::base::input::Copy;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::component::{TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{
    Locale, bottom_buffer_label, quick_save_failed, quick_skipped, signal_write_failed,
    timer_interval_error, tr, tr_shared,
};
use crate::model::log::format_hex_input_with_cursor;
use crate::model::log::{LogEntry, format_visible_logs};
use crate::model::log_filter::{LogFilter, LogFilterState, LogProjection};
use crate::model::quick_command::{
    QuickCommand, QuickEditorDraft, command_for_shortcut, remove_command, visible_rows,
};
use crate::model::shortcuts::{Modifiers, ShortcutAction};
use crate::model::timed_send::{
    DraftCommit, EnterAction, IconAction, MAX_TIMER_INTERVAL_MS, MIN_TIMER_INTERVAL_MS,
    SendButtonAction, StopCause, TickOutcome, TimedSendState, commit_interval_draft,
    draft_is_pending, enabled_after_stop, enter_action, icon_click_action, interval_for_start,
    manual_send_blocked, next_deadline, send_button_action, should_skip_cycle,
    should_stop_on_tx_error, stop_cause_for, tick_outcome, timer_reply_is_current,
};
use crate::model::{
    ActivePickerKind, ConfigStore, DataBits, FlowControl, LogSearchState, LogSelectionModel,
    ModemSignals, Parity, SAMPLE_INTERVAL_MS, SendPayloadError, SessionState, StopBits,
    ThroughputSpan, current_timestamp_hms_ms, format_buffer_usage, format_log_buffer_limit,
    parse_log_buffer_limit_for, theoretical_max_bytes_per_sec,
};
use crate::model::{MarkerColor, panel_items};
use crate::scripting::{ScriptEvent, ScriptManager, ScriptStatus};
use crate::serial::{SerialConnection, SerialEvent, enumerate_serial_ports_for};
use crate::session_service::{ControlLine, SessionService, SessionTxError};
use crate::ui::bottom_bar::{BottomBar, SignalControls};
use crate::ui::filter_bar::{
    FILTER_FRAME_RADIUS, FILTER_FRAME_SIDE, FILTER_FRAME_TOP, FILTER_FRAME_TRANSITION_MS,
    FilterControl, FilterFrameEdge, FilterFrameLabel, FrameTransition, LogFilterBar,
    animate_content_corners, animate_frame_inset,
};
use crate::ui::log_view::{LogView, log_row_height_hint};
use crate::ui::marker_editor::{
    MARKER_EDITOR_HEIGHT, MARKER_EDITOR_WIDTH, MarkerEditor, MarkerEditorState,
};
use crate::ui::marker_panel::{MarkerPanel, MarkerPanelItem};
use crate::ui::motion::{TransitionFlag, UiMotionState};
use crate::ui::parameter_picker::ParameterPickerPopover;
use crate::ui::quick_command_window::{
    QuickCommandWindow, QuickFillSource, persist_quick_commands,
};
use crate::ui::quick_send_panel::QuickSendPanel;
use crate::ui::receive_toolbar::ReceiveToolbar;
use crate::ui::script_panel::{
    ResizeScriptPanel, SCRIPT_PANEL_MIN_WIDTH, SCRIPT_PANEL_WIDTH, ScriptPanel, ScriptPanelItem,
    dragged_panel_width, panel_is_compact, panel_width,
};
use crate::ui::search_bar::LogSearchBar;
use crate::ui::send_card::{SendCard, TimerControls};
use crate::ui::settings_window::SettingsWindow;
use crate::ui::start_page::{StartPage, StartPageState};
use crate::ui::tab_rail::{TAB_RAIL_WIDTH, TabItem, TabRail};
use crate::ui::theme::NanoTheme;
use crate::ui::theme_manager::{ThemeHandle, ThemeManager};
use crate::ui::throughput_window::{ThroughputSnapshot, ThroughputWindow};
use crate::ui::update_manager::{UpdateHandle, UpdateManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPage {
    Start,
    Workspace,
}

#[derive(Debug, Clone, Default)]
struct ScriptEntryState {
    run_id: Option<u64>,
    status: Option<ScriptStatus>,
}

struct SessionTab {
    id: u64,
    page: AppPage,
    state: SessionState,
    motion: UiMotionState,
    start_page: StartPageState,
    start_manual_port_input: Option<Entity<InputState>>,
    _start_manual_port_sub: Option<Subscription>,
    start_baud_input: Option<Entity<InputState>>,
    _start_baud_sub: Option<Subscription>,
    serial_conn: Option<SerialConnection>,
    /// True while the port is being opened on a background thread.
    connecting: bool,
    serial_rx_task: Option<Task<()>>,
    session_service: SessionService,
    send_input: Option<Entity<TextareaState>>,
    _send_subscription: Option<Subscription>,
    timer: TimedSendState,
    timer_flag: TransitionFlag,
    /// Bumped on every timer start; lets late TX replies detect they belong to an older run.
    timer_run_id: u64,
    /// Timed sends enqueued whose write has not finished yet (current run only).
    timer_pending: u32,
    timer_task: Option<Task<()>>,
    timer_input: Option<Entity<InputState>>,
    _timer_input_sub: Option<Subscription>,
    send_error: Option<SharedString>,
    quick_error: Option<SharedString>,
    quick_collapsed: bool,
    scripts_collapsed: bool,
    quick_query: String,
    quick_collapsed_groups: HashSet<String>,
    quick_search_input: Option<Entity<InputState>>,
    _quick_search_sub: Option<Subscription>,
    quick_pending_delete: Option<String>,
    marker_collapsed: bool,
    marker_pending_clear: bool,
    log_focus: FocusHandle,
    log_list_state: ListState,
    /// `(item count, font size)` last given a uniform row-height hint.
    row_hint_key: Option<(usize, u16)>,
    log_filter: LogFilterState,
    log_projection: LogProjection,
    /// Whether the filter frame was drawn as on when this tab was last shown.
    frame_shown: bool,
    /// Bumped each time the frame switches on or off while this tab is on screen.
    frame_key: u64,
    /// When that last happened; the transition plays only for a moment after it.
    frame_changed_at: Option<Instant>,
    filter_input: Option<Entity<InputState>>,
    filter_input_display: Option<(Locale, bool)>,
    _filter_subscription: Option<Subscription>,
    last_notice: bool,
    last_display_flags: (bool, bool),
    log_selection: LogSelectionModel,
    log_search: LogSearchState,
    search_input: Option<Entity<InputState>>,
    _search_subscription: Option<Subscription>,
    script_paths: Vec<PathBuf>,
    script_states: HashMap<PathBuf, ScriptEntryState>,
    script_manager: ScriptManager,
    script_poll_task: Option<Task<()>>,
    script_preferred_width: Pixels,
    script_load_error: Option<SharedString>,
    stream_busy: bool,
    stream_error: Option<SharedString>,
    /// Error from the last "save log" export, shown until the next attempt.
    save_log_error: Option<SharedString>,
    stream_poll_task: Option<Task<()>>,
}

impl SessionTab {
    fn new(
        id: u64,
        locale: Locale,
        initial_scripts: Vec<PathBuf>,
        log_buffer_limit: usize,
        cx: &mut Context<SashaApp>,
    ) -> Self {
        let mut state =
            SessionState::new_real_session(crate::model::config::SerialConfig::default(), false);
        state.set_log_buffer_limit(log_buffer_limit);
        let motion = UiMotionState::new(
            state.receive.hex_mode,
            state.receive.show_timestamp,
            state.receive.paused,
            state.receive.auto_scroll,
            state.send.display_hex,
            state.send.send_hex,
            state.send.line_ending.is_active(),
            state.connection.is_connected(),
        );
        let log_selection = LogSelectionModel::new(
            state.receive.show_timestamp,
            state.receive.hex_mode,
            state.logs.len(),
        );
        Self {
            id,
            page: AppPage::Start,
            state,
            motion,
            start_page: StartPageState::new_for(locale),
            start_manual_port_input: None,
            _start_manual_port_sub: None,
            start_baud_input: None,
            _start_baud_sub: None,
            serial_conn: None,
            connecting: false,
            serial_rx_task: None,
            session_service: SessionService::new(),
            send_input: None,
            _send_subscription: None,
            timer: TimedSendState::default(),
            timer_flag: TransitionFlag::default(),
            timer_run_id: 0,
            timer_pending: 0,
            timer_task: None,
            timer_input: None,
            _timer_input_sub: None,
            send_error: None,
            quick_error: None,
            quick_collapsed: false,
            scripts_collapsed: false,
            quick_query: String::new(),
            quick_collapsed_groups: HashSet::new(),
            quick_search_input: None,
            _quick_search_sub: None,
            quick_pending_delete: None,
            marker_collapsed: false,
            marker_pending_clear: false,
            log_focus: cx.focus_handle(),
            log_list_state: ListState::new(0, ListAlignment::Top, px(200.)),
            row_hint_key: None,
            log_filter: LogFilterState::default(),
            log_projection: LogProjection::default(),
            frame_shown: false,
            frame_key: 0,
            frame_changed_at: None,
            filter_input: None,
            filter_input_display: None,
            _filter_subscription: None,
            last_notice: false,
            last_display_flags: (true, false),
            log_selection,
            log_search: LogSearchState::default(),
            search_input: None,
            _search_subscription: None,
            script_paths: initial_scripts,
            script_states: HashMap::new(),
            script_manager: ScriptManager::new(),
            script_poll_task: None,
            script_preferred_width: SCRIPT_PANEL_MIN_WIDTH,
            script_load_error: None,
            stream_busy: false,
            stream_error: None,
            save_log_error: None,
            stream_poll_task: None,
        }
    }

    fn tab_item(&self, active_id: u64, locale: Locale) -> TabItem {
        let title: SharedString = if self.page == AppPage::Start {
            tr_shared(locale, "tab.new_connection")
        } else if self.state.is_simulated_session {
            tr_shared(locale, "tab.simulated_demo")
        } else {
            self.state.config.port.clone().into()
        };
        TabItem {
            id: self.id,
            title,
            is_active: self.id == active_id,
            is_connected: self.page == AppPage::Workspace && self.state.connection.is_connected(),
            is_simulated: self.page == AppPage::Workspace && self.state.is_simulated_session,
            running_script_count: self.script_manager.running_count(),
        }
    }

    fn sync_motion_and_selection_from_state(&mut self) {
        self.motion = UiMotionState::new(
            self.state.receive.hex_mode,
            self.state.receive.show_timestamp,
            self.state.receive.paused,
            self.state.receive.auto_scroll,
            self.state.send.display_hex,
            self.state.send.send_hex,
            self.state.send.line_ending.is_active(),
            self.state.connection.is_connected(),
        );
        self.log_projection = LogProjection::default();
        self.last_notice = false;
        self.log_list_state.reset(0);
        self.log_selection = LogSelectionModel::new(
            self.state.receive.show_timestamp,
            self.state.receive.hex_mode,
            self.state.logs.len(),
        );
    }

    fn recompute_log_search(&mut self, preserve_active: bool) {
        let logs = self.visible_logs();
        self.log_search.recompute(
            &logs,
            self.state.receive.show_timestamp,
            self.state.receive.hex_mode,
            preserve_active,
        );
    }

    /// Log entries that pass the applied filter, in order. Input events can
    /// arrive after serial data changed but before the next render has
    /// synchronized the projection; take a fresh snapshot in that case.
    fn visible_logs(&self) -> Vec<LogEntry> {
        let hex_mode = self.state.receive.hex_mode;
        if self
            .log_projection
            .is_current(&self.state.logs, &self.log_filter, hex_mode)
        {
            return self.log_projection.entries(&self.state.logs);
        }
        let mut projection = LogProjection::default();
        projection.sync(&self.state.logs, &self.log_filter, hex_mode);
        projection.entries(&self.state.logs)
    }

    fn sync_log_list_state(&mut self, log_font_size: u16) {
        // Markers die with the entries the buffer limit evicted.
        self.state
            .markers
            .prune_before(self.state.logs.first().map(|entry| entry.id));
        let old_len = self.log_projection.rows.len();
        let old_offset = self.log_list_state.logical_scroll_top();
        let anchor_id = self
            .log_projection
            .rows
            .get(old_offset.item_ix)
            .map(|row| row.id);
        let active_match = self.log_search.current().and_then(|m| {
            self.log_projection
                .rows
                .get(m.log_index)
                .map(|row| (row.id, m.byte_range.clone()))
        });
        let change = self.log_projection.sync(
            &self.state.logs,
            &self.log_filter,
            self.state.receive.hex_mode,
        );
        let rows = &self.log_projection.rows;
        let notice = !self.state.simulated_notice.is_empty();
        let total_items = rows.len() + usize::from(notice);
        let display_flags = (
            self.state.receive.show_timestamp,
            self.state.receive.hex_mode,
        );
        let flags_changed = display_flags != self.last_display_flags;
        // Source indices may shift after trimming without changing a visible row.
        let prefix = change.prefix;
        let removed = change.removed;
        let content_changed = prefix != old_len || prefix != rows.len();
        if flags_changed {
            self.log_list_state.reset(total_items);
        } else if content_changed || notice != self.last_notice {
            let start = prefix.min(self.log_list_state.item_count());
            self.log_list_state
                .splice(start..self.log_list_state.item_count(), total_items - start);
        } else if self.log_list_state.item_count() != total_items {
            self.log_list_state.reset(total_items);
        }

        self.ensure_log_row_height_hints(log_font_size);

        if self.state.receive.auto_scroll && content_changed {
            self.log_list_state.scroll_to_end();
        } else if flags_changed || removed {
            if let Some(id) = anchor_id {
                let index = self
                    .log_projection
                    .rows
                    .partition_point(|row| row.id < id)
                    .min(self.log_projection.rows.len().saturating_sub(1));
                self.log_list_state.scroll_to(ListOffset {
                    item_ix: index,
                    ..old_offset
                });
            }
        }

        if self.log_search.open && (content_changed || flags_changed) {
            self.recompute_log_search(true);
            if let Some((id, range)) = active_match {
                if let Some(index) = self.log_search.matches.iter().position(|m| {
                    self.log_projection
                        .rows
                        .get(m.log_index)
                        .is_some_and(|row| row.id == id)
                        && m.byte_range == range
                }) {
                    self.log_search.active_match = Some(index);
                }
            }
        }

        self.last_notice = notice;
        self.last_display_flags = display_flags;
    }

    /// Give every row that has not been measured a real height.
    ///
    /// `ListState` otherwise records those rows as 0px. Dragging the scrollbar
    /// to the top then stops at the first measured row, and further upward
    /// scrolling still reveals older logs.
    fn ensure_log_row_height_hints(&mut self, font_size: u16) {
        let count = self.log_list_state.item_count();
        if count == 0 {
            self.row_hint_key = None;
            return;
        }
        let hint = log_row_height_hint(font_size);
        if let Some((hinted_count, hinted_font)) = self.row_hint_key
            && hinted_count == count
            && hinted_font == font_size
            && self.log_row_hints_cover_content(count, hint)
        {
            return;
        }
        if self.row_hint_key.is_some_and(|(_, size)| size != font_size) {
            self.log_list_state.remeasure();
        }
        let _ = self.log_list_state.clone().with_uniform_item_height(hint);
        self.row_hint_key = Some((count, font_size));
    }

    fn log_row_hints_cover_content(&self, count: usize, hint: Pixels) -> bool {
        if self.log_list_state.is_scrolled_to_end().is_some() {
            return true;
        }
        let viewport = self.log_list_state.viewport_bounds().size.height;
        if viewport <= px(0.) {
            return true;
        }
        if self.log_list_state.max_offset_for_scrollbar().y > px(0.) {
            return false;
        }
        hint * (count as f32) <= viewport + px(1.)
    }

    fn drain_tx_completed_logs(&mut self) -> bool {
        let completed = self.session_service.drain_tx_completed();
        if completed.is_empty() {
            return false;
        }
        let ts = current_timestamp_hms_ms();
        for event in completed {
            self.state.record_tx_bytes(&event.bytes, &ts);
        }
        self.log_selection.observe_log_len(self.state.logs.len());
        true
    }

    fn drain_script_events(&mut self) -> bool {
        let events = self.script_manager.drain_events();
        if events.is_empty() {
            return false;
        }
        for event in events {
            match event {
                ScriptEvent::Started { run_id, path, .. } => {
                    let entry = self.script_states.entry(path).or_default();
                    if entry.run_id.is_none_or(|existing| run_id >= existing) {
                        entry.run_id = Some(run_id);
                        if entry.status.is_none() {
                            entry.status = Some(ScriptStatus::Running);
                        }
                    }
                }
                ScriptEvent::StatusChanged { run_id, status } => {
                    if let Some(entry) = self
                        .script_states
                        .values_mut()
                        .find(|entry| entry.run_id == Some(run_id))
                    {
                        entry.status = Some(status);
                    }
                }
                ScriptEvent::Log(_) => {}
            }
        }
        true
    }

    fn script_panel_items(&self, locale: Locale) -> Vec<ScriptPanelItem> {
        let is_simulated = self.state.is_simulated_session;
        let is_connected = self.state.connection.is_connected();
        let can_run = !is_simulated && is_connected;
        let disabled_reason = if is_simulated {
            Some(tr_shared(locale, "scripts.simulated_unsupported"))
        } else if !is_connected {
            Some(tr_shared(locale, "scripts.connect_to_run"))
        } else {
            None
        };
        self.script_paths
            .iter()
            .map(|path| {
                let state = self.script_states.get(path);
                ScriptPanelItem {
                    path: path.clone(),
                    run_id: state.and_then(|s| s.run_id),
                    status: state.and_then(|s| s.status.clone()),
                    can_run,
                    disabled_reason: disabled_reason.clone(),
                }
            })
            .collect()
    }

    /// Stops the repeating send (no-op when idle). Dropping the task cancels it.
    fn stop_timer(&mut self) {
        self.timer.running = false;
        self.timer_task = None;
    }

    /// Stops the timer; whether the capsule stays enabled follows `enabled_after_stop`.
    fn stop_timer_for(&mut self, cause: StopCause) {
        self.stop_timer();
        if !enabled_after_stop(cause) {
            self.timer.enabled = false;
            self.timer_flag.set(false);
        }
    }

    fn disconnect_real_serial(&mut self, error: Option<String>) {
        self.stop_timer_for(StopCause::Disconnected);
        self.serial_rx_task = None;
        self.session_service.disconnect(error.clone());
        self.script_manager.handle_disconnect(error.clone());
        self.drain_script_events();
        if let Some(mut conn) = self.serial_conn.take() {
            conn.close();
        }
        self.state.set_connected(false);
        self.motion.connection.set(false);
        if let Some(err) = error {
            self.send_error = Some(err.into());
        }
    }
}

impl Drop for SessionTab {
    fn drop(&mut self) {
        self.disconnect_real_serial(None);
    }
}

pub struct SashaApp {
    tabs: Vec<SessionTab>,
    active_tab_id: u64,
    next_tab_id: u64,
    theme: NanoTheme,
    theme_manager: Entity<ThemeManager>,
    _theme_subscription: Subscription,
    update_manager: Entity<UpdateManager>,
    _update_subscription: Subscription,
    config_store: Entity<ConfigStore>,
    last_locale: Locale,
    settings_window: Option<AnyWindowHandle>,
    quick_editor_window: Option<AnyWindowHandle>,
    throughput_window: Option<(AnyWindowHandle, WeakEntity<ThroughputWindow>)>,
    last_throughput_sample_at: Instant,
    _config_subscription: Subscription,
    picker_port_presets: Vec<String>,
    picker_input: Option<Entity<InputState>>,
    _picker_subscription: Option<Subscription>,
    picker_error: Option<SharedString>,
    cached_bg_image: Option<(PathBuf, Option<Arc<Image>>)>,
    /// The log-marker form, open for at most one entry at a time.
    marker_editor: Option<MarkerEditorState>,
    _marker_editor_subscription: Option<Subscription>,
    /// The tab the filter frame was last drawn for, to tell a filter being switched on or off
    /// from a switch to another tab.
    last_frame_tab: Option<u64>,
    /// The line running around the filter frame's content, created the first time it is needed.
    filter_frame_edge: Option<Entity<FilterFrameEdge>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SendErrorSlot {
    SendCard,
    QuickPanel,
    /// Repeating send; behaves like `SendCard` but stops the timer on failure.
    Timer,
}

fn send_payload_error_text(
    locale: Locale,
    is_simulated: bool,
    err: &SendPayloadError,
) -> SharedString {
    match err {
        SendPayloadError::Disconnected => {
            if is_simulated {
                tr_shared(locale, "err.send_sim_disconnected")
            } else {
                tr_shared(locale, "err.send_disconnected")
            }
        }
        SendPayloadError::EmptyInput => tr_shared(locale, "err.send_empty"),
        SendPayloadError::InvalidHex(_) => tr_shared(locale, "err.invalid_hex"),
    }
}

impl SessionTab {
    fn set_send_error(&mut self, slot: SendErrorSlot, text: Option<SharedString>) {
        match slot {
            SendErrorSlot::SendCard | SendErrorSlot::Timer => self.send_error = text,
            SendErrorSlot::QuickPanel => self.quick_error = text,
        }
    }
}

impl SashaApp {
    pub fn new(config_store: Entity<ConfigStore>, cx: &mut Context<Self>) -> Self {
        cx.on_app_quit(|this, cx| {
            let outputs: Vec<_> = this
                .tabs
                .iter_mut()
                .filter_map(|tab| tab.state.stream_output.take())
                .collect();
            cx.background_executor().spawn(async move {
                for output in outputs {
                    let _ = output.finish();
                }
            })
        })
        .detach();
        let _config_subscription = cx.observe(&config_store, |_, _, cx| cx.notify());
        let theme_manager = ThemeHandle::get(cx);
        let _theme_subscription = cx.observe(&theme_manager, |_, _, cx| cx.notify());
        let update_manager = UpdateHandle::get(cx);
        let _update_subscription = cx.observe(&update_manager, |_, _, cx| cx.notify());
        let initial_theme = theme_manager.read(cx).nano();
        let (locale, initial_scripts, log_buffer_limit) = {
            let cfg = config_store.read(cx).config();
            (
                cfg.locale,
                cfg.scripts.clone(),
                cfg.log_buffer_limit as usize,
            )
        };
        let initial_tab = SessionTab::new(1, locale, initial_scripts, log_buffer_limit, cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(SAMPLE_INTERVAL_MS))
                    .await;
                let keep_going = this.update(cx, |app, cx| {
                    app.sample_all_throughputs(cx);
                });
                if keep_going.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            tabs: vec![initial_tab],
            active_tab_id: 1,
            next_tab_id: 2,
            theme: initial_theme,
            theme_manager,
            _theme_subscription,
            update_manager,
            _update_subscription,
            config_store,
            last_locale: locale,
            settings_window: None,
            quick_editor_window: None,
            throughput_window: None,
            last_throughput_sample_at: Instant::now(),
            _config_subscription,
            picker_port_presets: Vec::new(),
            picker_input: None,
            _picker_subscription: None,
            picker_error: None,
            cached_bg_image: None,
            marker_editor: None,
            _marker_editor_subscription: None,
            last_frame_tab: None,
            filter_frame_edge: None,
        }
    }

    fn sync_background_image(&mut self, cx: &App) -> (Option<Arc<Image>>, u8) {
        let (path, opacity) = {
            let cfg = self.config_store.read(cx).config();
            (cfg.background_image.clone(), cfg.background_opacity)
        };
        let Some(path) = path else {
            self.cached_bg_image = None;
            return (None, opacity);
        };
        if let Some((cached_path, cached_img)) = &self.cached_bg_image
            && cached_path == &path
        {
            return (cached_img.clone(), opacity);
        }
        let loaded = load_background_image(&path);
        self.cached_bg_image = Some((path, loaded.clone()));
        (loaded, opacity)
    }

    fn build_background_layer(&self, image: Option<Arc<Image>>, opacity: u8) -> Option<AnyElement> {
        let image = image?;
        if opacity == 0 {
            return None;
        }
        let scrim_opacity = 1.0 - (opacity as f32 / 100.0);
        Some(
            div()
                .id("workspace-bg-layer")
                .absolute()
                .inset_0()
                .overflow_hidden()
                .child(img(image).size_full().object_fit(ObjectFit::Cover))
                .when(scrim_opacity > 0.0, |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .bg(self.theme.bg_canvas.opacity(scrim_opacity)),
                    )
                })
                .into_any_element(),
        )
    }

    fn current_locale(&self, cx: &App) -> Locale {
        self.config_store.read(cx).config().locale
    }

    fn saved_log_buffer_limit(&self, cx: &App) -> usize {
        self.config_store.read(cx).config().log_buffer_limit as usize
    }

    fn sync_placeholders_if_locale_changed(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.last_locale == locale {
            return;
        }
        self.last_locale = locale;

        for tab in &self.tabs {
            if let Some(input) = &tab.start_manual_port_input {
                input.update(cx, |state, cx| {
                    state.set_placeholder(tr(locale, "start.manual_port_placeholder"), window, cx);
                });
            }
            if let Some(input) = &tab.start_baud_input {
                input.update(cx, |state, cx| {
                    state.set_placeholder(tr(locale, "start.baud_placeholder"), window, cx);
                });
            }
            if let Some(input) = &tab.timer_input {
                input.update(cx, |state, cx| {
                    state.set_placeholder(
                        tr(locale, "send.timer_interval_placeholder"),
                        window,
                        cx,
                    );
                });
            }
            if let Some(input) = &tab.send_input {
                let key = if tab.state.is_simulated_session {
                    "send.placeholder_sim"
                } else {
                    "send.placeholder"
                };
                input.update(cx, |state, cx| {
                    state.set_placeholder(tr(locale, key), window, cx);
                });
            }
        }

        if let Some(input) = &self.picker_input {
            let placeholder = self
                .active_tab()
                .state
                .active_picker
                .map(|k| k.placeholder_for(locale))
                .unwrap_or_else(|| tr(locale, "picker.input_placeholder"));
            input.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx);
            });
        }
    }

    fn active_tab(&self) -> &SessionTab {
        self.tabs
            .iter()
            .find(|t| t.id == self.active_tab_id)
            .unwrap_or(&self.tabs[0])
    }

    fn active_tab_mut(&mut self) -> &mut SessionTab {
        let active_id = self.active_tab_id;
        let idx = self
            .tabs
            .iter()
            .position(|t| t.id == active_id)
            .unwrap_or(0);
        &mut self.tabs[idx]
    }

    fn tab_mut(&mut self, tab_id: u64) -> Option<&mut SessionTab> {
        self.tabs.iter_mut().find(|t| t.id == tab_id)
    }

    fn tab_items(&self, locale: Locale) -> Vec<TabItem> {
        self.tabs
            .iter()
            .map(|t| t.tab_item(self.active_tab_id, locale))
            .collect()
    }

    fn dismiss_picker_state(&mut self) {
        self.picker_error = None;
        // Tab switches, new tabs and closed tabs all pass through here; the marker form
        // belongs to one tab's entry, so it goes too.
        self.marker_editor = None;
        self._marker_editor_subscription = None;
        for tab in &mut self.tabs {
            tab.state.active_picker = None;
            tab.quick_pending_delete = None;
        }
    }

    fn select_tab(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_id == tab_id || !self.tabs.iter().any(|t| t.id == tab_id) {
            return;
        }
        self.dismiss_picker_state();
        TextSelection::clear(window, cx);
        self.active_tab_id = tab_id;
        self.notify_throughput_window(true, cx);
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_picker_state();
        TextSelection::clear(window, cx);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let (locale, initial_scripts, log_buffer_limit) = {
            let cfg = self.config_store.read(cx).config();
            (
                cfg.locale,
                cfg.scripts.clone(),
                cfg.log_buffer_limit as usize,
            )
        };
        let tab = SessionTab::new(id, locale, initial_scripts, log_buffer_limit, cx);
        self.tabs.push(tab);
        self.active_tab_id = id;
        self.notify_throughput_window(true, cx);
        cx.notify();
    }

    fn close_tab(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        self.dismiss_picker_state();
        self.tabs[idx].disconnect_real_serial(None);
        self.tabs.remove(idx);

        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }

        if self.active_tab_id == tab_id {
            let next_idx = idx.min(self.tabs.len() - 1);
            self.active_tab_id = self.tabs[next_idx].id;
            TextSelection::clear(window, cx);
            self.notify_throughput_window(true, cx);
        }
        cx.notify();
    }

    fn cycle_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        let Some(idx) = self.tabs.iter().position(|t| t.id == self.active_tab_id) else {
            return;
        };
        let next_idx = (idx as isize + delta).rem_euclid(self.tabs.len() as isize) as usize;
        let next_id = self.tabs[next_idx].id;
        self.select_tab(next_id, window, cx);
    }

    /// The action bound to a key press, with the 1 to 9 digit for the digit families.
    fn shortcut_for(
        &self,
        event: &KeyDownEvent,
        cx: &App,
    ) -> Option<(ShortcutAction, Option<usize>)> {
        let m = &event.keystroke.modifiers;
        let mods = Modifiers::new(m.control, m.alt, m.shift, m.platform);
        self.config_store
            .read(cx)
            .config()
            .shortcuts
            .resolve(&event.keystroke.key, mods)
    }

    /// Session shortcuts (new, close, next, previous, jump). They work on every page.
    fn handle_tab_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((action, digit)) = self.shortcut_for(event, cx) else {
            return false;
        };
        match (action, digit) {
            (ShortcutAction::NewSession, _) => self.new_tab(window, cx),
            (ShortcutAction::CloseSession, _) => {
                let active_id = self.active_tab_id;
                self.close_tab(active_id, window, cx);
            }
            (ShortcutAction::NextSession, _) => self.cycle_tab(1, window, cx),
            (ShortcutAction::PrevSession, _) => self.cycle_tab(-1, window, cx),
            (ShortcutAction::JumpSession, Some(digit)) => {
                let target_idx = if digit == 9 {
                    self.tabs.len().saturating_sub(1)
                } else {
                    digit - 1
                };
                if let Some(tab) = self.tabs.get(target_idx) {
                    let id = tab.id;
                    self.select_tab(id, window, cx);
                }
            }
            _ => return false,
        }
        true
    }

    /// Sends the quick command bound to the pressed digit, on the workspace page only.
    fn handle_quick_shortcut(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let tab = self.active_tab();
        if tab.page != AppPage::Workspace || tab.state.active_picker.is_some() {
            return false;
        }
        let Some((ShortcutAction::QuickSend, Some(digit))) = self.shortcut_for(event, cx) else {
            return false;
        };
        let command_id =
            command_for_shortcut(&self.config_store.read(cx).config().quick_commands, digit)
                .map(|c| c.id.clone());
        if let Some(id) = command_id {
            self.send_quick_command(&id, cx);
            true
        } else {
            false
        }
    }

    /// Opens the marker form for a right-clicked log entry, prefilled when it is marked.
    fn open_marker_editor(
        &mut self,
        entry_id: u64,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab_id = self.active_tab_id;
        let existing = self
            .tab_mut(tab_id)
            .and_then(|tab| tab.state.markers.get(entry_id).cloned());
        let locale = self.config_store.read(cx).config().locale;
        let placeholder = tr_shared(locale, "marker.name_placeholder");
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        if let Some(marker) = &existing {
            let name = marker.name.clone();
            input.update(cx, |state, cx| state.set_value(name, window, cx));
        }
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, _input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.confirm_marker(cx);
                }
            },
        );
        input.read(cx).focus_handle(cx).focus(window, cx);

        let viewport = window.viewport_size();
        let max_x = (viewport.width - MARKER_EDITOR_WIDTH - px(8.)).max(px(8.));
        let max_y = (viewport.height - MARKER_EDITOR_HEIGHT - px(8.)).max(px(8.));
        self.marker_editor = Some(MarkerEditorState {
            tab_id,
            entry_id,
            position: point(
                position.x.clamp(px(8.), max_x),
                position.y.clamp(px(8.), max_y),
            ),
            color: existing
                .as_ref()
                .map(|marker| marker.color)
                .unwrap_or(MarkerColor::Red),
            input,
            existing: existing.is_some(),
            error: None,
        });
        self._marker_editor_subscription = Some(subscription);
        cx.notify();
    }

    /// Scrolls the log to a marked entry. Entries hidden by the filter are not scrolled to.
    pub fn jump_to_log_entry(&mut self, entry_id: u64, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        let Ok(index) = tab
            .log_projection
            .rows
            .binary_search_by_key(&entry_id, |row| row.id)
        else {
            return;
        };
        // New data would otherwise scroll straight back to the end.
        if tab.state.receive.auto_scroll {
            tab.state.toggle_auto_scroll();
            tab.motion.rx_auto_scroll.set(tab.state.receive.auto_scroll);
        }
        tab.log_list_state.scroll_to_reveal_item(index);
        cx.notify();
    }

    /// Removes one marker from the dock list.
    fn remove_marker(&mut self, entry_id: u64, cx: &mut Context<Self>) {
        self.active_tab_mut().state.markers.remove(entry_id);
        cx.notify();
    }

    fn clear_markers(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.markers.clear();
        tab.marker_pending_clear = false;
        cx.notify();
    }

    fn pick_marker_color(&mut self, color: MarkerColor, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.marker_editor {
            editor.color = color;
            cx.notify();
        }
    }

    /// Applies the form. An invalid name keeps the form open with the draft.
    fn confirm_marker(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.marker_editor.as_ref() else {
            return;
        };
        let name = editor.input.read(cx).value().to_string();
        let (tab_id, entry_id, color) = (editor.tab_id, editor.entry_id, editor.color);
        let locale = self.config_store.read(cx).config().locale;
        let Some(tab) = self.tab_mut(tab_id) else {
            self.close_marker_editor(cx);
            return;
        };
        match tab.state.markers.set(entry_id, &name, color) {
            Ok(()) => {
                // A confirmation armed before the list emptied must not reappear for new markers.
                tab.marker_pending_clear = false;
                self.close_marker_editor(cx);
            }
            Err(err) => {
                if let Some(editor) = &mut self.marker_editor {
                    editor.error = Some(tr_shared(locale, err.locale_key()));
                }
                cx.notify();
            }
        }
    }

    fn delete_marker(&mut self, cx: &mut Context<Self>) {
        if let Some((tab_id, entry_id)) = self
            .marker_editor
            .as_ref()
            .map(|editor| (editor.tab_id, editor.entry_id))
            && let Some(tab) = self.tab_mut(tab_id)
        {
            tab.state.markers.remove(entry_id);
        }
        self.close_marker_editor(cx);
    }

    fn close_marker_editor(&mut self, cx: &mut Context<Self>) {
        self.marker_editor = None;
        self._marker_editor_subscription = None;
        cx.notify();
    }

    fn ensure_start_manual_port_input(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(existing) = self
            .tab_mut(tab_id)
            .and_then(|t| t.start_manual_port_input.clone())
        {
            return existing;
        }

        let placeholder = tr(self.current_locale(cx), "start.manual_port_placeholder");
        let entity = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let entity_for_sub = entity.clone();
        let sub = cx.subscribe_in(
            &entity,
            window,
            move |this, _, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    let raw = entity_for_sub.read(cx).value().to_string();
                    if let Some(tab) = this.tab_mut(tab_id) {
                        tab.start_page.set_manual_port_value(&raw);
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    if this.active_tab_id == tab_id {
                        this.connect_from_start_page(cx);
                    }
                }
                _ => {}
            },
        );

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.start_manual_port_input = Some(entity.clone());
            tab._start_manual_port_sub = Some(sub);
        }
        entity
    }

    fn ensure_start_baud_input(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(existing) = self
            .tab_mut(tab_id)
            .and_then(|t| t.start_baud_input.clone())
        {
            return existing;
        }

        let placeholder = tr(self.current_locale(cx), "start.baud_placeholder");
        let entity = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let entity_for_sub = entity.clone();
        let sub = cx.subscribe_in(
            &entity,
            window,
            move |this, _, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    if let Some(tab) = this.tab_mut(tab_id)
                        && tab.start_page.baud_error.is_some()
                    {
                        tab.start_page.baud_error = None;
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => {
                    let raw = entity_for_sub.read(cx).value().to_string();
                    let locale = this.current_locale(cx);
                    if let Some(tab) = this.tab_mut(tab_id) {
                        let _ = tab.start_page.apply_custom_baud_for(&raw, locale);
                    }
                    cx.notify();
                }
                _ => {}
            },
        );

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.start_baud_input = Some(entity.clone());
            tab._start_baud_sub = Some(sub);
        }
        entity
    }

    fn refresh_start_ports(&mut self, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        self.active_tab_mut()
            .start_page
            .refresh_ports(enumerate_serial_ports_for(locale));
        cx.notify();
    }

    fn select_start_port(&mut self, port_name: String, cx: &mut Context<Self>) {
        self.active_tab_mut()
            .start_page
            .select_detected_port(port_name);
        cx.notify();
    }

    fn enable_start_manual_port(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab_id = self.active_tab_id;
        let current_port = {
            let tab = self.active_tab_mut();
            tab.start_page.enable_manual_port();
            tab.start_page.config.port.clone()
        };
        let input = self.ensure_start_manual_port_input(tab_id, window, cx);
        input.update(cx, |state, cx| {
            if !current_port.is_empty() && state.value().is_empty() {
                state.set_value(current_port, window, cx);
            }
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        let value = input.read(cx).value().to_string();
        self.active_tab_mut()
            .start_page
            .set_manual_port_value(&value);
        cx.notify();
    }

    fn toggle_start_baud_dropdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab_id = self.active_tab_id;
        let (opened, current_baud) = {
            let tab = self.active_tab_mut();
            tab.start_page.baud_dropdown_open = !tab.start_page.baud_dropdown_open;
            tab.start_page.baud_error = None;
            (
                tab.start_page.baud_dropdown_open,
                tab.start_page.config.baud_rate.to_string(),
            )
        };
        if opened {
            let input = self.ensure_start_baud_input(tab_id, window, cx);
            input.update(cx, |state, cx| {
                state.set_value(current_baud, window, cx);
                state.focus(window, cx);
                state.select_all(window, cx);
            });
        }
        cx.notify();
    }

    fn select_start_baud(&mut self, baud: u32, cx: &mut Context<Self>) {
        self.active_tab_mut().start_page.select_baud_preset(baud);
        cx.notify();
    }

    fn select_start_data_bits(&mut self, bits: DataBits, cx: &mut Context<Self>) {
        self.active_tab_mut().start_page.config.data_bits = bits;
        cx.notify();
    }

    fn select_start_parity(&mut self, parity: Parity, cx: &mut Context<Self>) {
        self.active_tab_mut().start_page.config.parity = parity;
        cx.notify();
    }

    fn select_start_stop_bits(&mut self, stop_bits: StopBits, cx: &mut Context<Self>) {
        self.active_tab_mut().start_page.config.stop_bits = stop_bits;
        cx.notify();
    }

    fn select_start_flow_control(&mut self, flow: FlowControl, cx: &mut Context<Self>) {
        self.active_tab_mut().start_page.config.flow_control = flow;
        cx.notify();
    }

    /// Flips the initial DTR/RTS level applied on the next connect.
    fn toggle_start_line(&mut self, line: ControlLine, cx: &mut Context<Self>) {
        let config = &mut self.active_tab_mut().start_page.config;
        match line {
            ControlLine::Dtr => config.dtr = !config.dtr,
            ControlLine::Rts if config.flow_control != FlowControl::Hardware => {
                config.rts = !config.rts;
            }
            ControlLine::Rts => {}
        }
        cx.notify();
    }

    fn connect_from_start_page(&mut self, cx: &mut Context<Self>) {
        let tab_id = self.active_tab_id;
        let manual_raw = {
            let tab = self.active_tab();
            if tab.start_page.manual_port_mode {
                tab.start_manual_port_input
                    .as_ref()
                    .map(|input| input.read(cx).value().to_string())
            } else {
                None
            }
        };
        if let Some(raw) = manual_raw {
            self.active_tab_mut().start_page.set_manual_port_value(&raw);
        }

        if self.active_tab().connecting {
            return;
        }
        let locale = self.current_locale(cx);
        let config = self.active_tab().start_page.config.clone();
        {
            let tab = self.active_tab_mut();
            tab.connecting = true;
            tab.start_page.connecting = true;
            tab.start_page.connect_error = None;
        }
        let open_config = config.clone();
        // Opening a port can block for seconds on some drivers; keep it off the UI thread.
        let opening = cx
            .background_executor()
            .spawn(async move { SerialConnection::open_for(&open_config, locale) });
        cx.spawn(async move |this, cx| {
            let result = opening.await;
            let _ = this.update(cx, |app, cx| {
                app.finish_connect_from_start_page(tab_id, config, result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_connect_from_start_page(
        &mut self,
        tab_id: u64,
        config: crate::model::SerialConfig,
        result: Result<(SerialConnection, Receiver<SerialEvent>), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return; // tab closed while opening; dropping `result` closes the port
        };
        tab.connecting = false;
        tab.start_page.connecting = false;
        match result {
            Ok((mut conn, rx)) => {
                let writer = conn
                    .take_writer()
                    .expect("newly opened SerialConnection must have a writer");
                let log_buffer_limit = self.saved_log_buffer_limit(cx);
                let Some(tab) = self.tab_mut(tab_id) else {
                    return;
                };
                tab.disconnect_real_serial(None);
                let generation = tab.session_service.attach_writer(writer);
                tab.state = SessionState::new_real_session(config, true);
                tab.state.set_log_buffer_limit(log_buffer_limit);
                tab.sync_motion_and_selection_from_state();
                tab.send_input = None;
                tab._send_subscription = None;
                tab.send_error = None;
                tab.start_page.connect_error = None;
                tab.serial_conn = Some(conn);
                tab.page = AppPage::Workspace;
                self.start_serial_rx_loop(tab_id, rx, generation, cx);
                cx.notify();
            }
            Err(err) => {
                if let Some(tab) = self.tab_mut(tab_id) {
                    tab.start_page.connect_error = Some(err);
                }
                cx.notify();
            }
        }
    }

    fn start_serial_rx_loop(
        &mut self,
        tab_id: u64,
        rx: Receiver<SerialEvent>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(session_service) = self.tab_mut(tab_id).map(|tab| tab.session_service.clone())
        else {
            return;
        };
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;

                let mut rx_chunks: Vec<Vec<u8>> = Vec::new();
                let mut latest_signals: Option<ModemSignals> = None;
                let mut disconnected_err: Option<String> = None;
                let mut channel_closed = false;

                loop {
                    match rx.try_recv() {
                        Ok(SerialEvent::Rx(bytes)) => {
                            session_service.publish_rx(generation, &bytes);
                            rx_chunks.push(bytes);
                        }
                        Ok(SerialEvent::Signals(signals)) => {
                            latest_signals = Some(signals);
                        }
                        Ok(SerialEvent::Disconnected(err)) => {
                            disconnected_err = Some(err);
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            channel_closed = true;
                            break;
                        }
                    }
                }

                let tx_completed = session_service.drain_tx_completed();
                let should_break = disconnected_err.is_some() || channel_closed;
                if !rx_chunks.is_empty()
                    || !tx_completed.is_empty()
                    || latest_signals.is_some()
                    || should_break
                {
                    let updated = this.update(cx, |app, cx| {
                        let is_active = app.active_tab_id == tab_id;
                        let closed_msg =
                            tr(app.current_locale(cx), "serial.rx_closed").into_owned();
                        let Some(tab) = app.tab_mut(tab_id) else {
                            return false;
                        };
                        if let Some(signals) = latest_signals {
                            tab.state.modem_signals = signals;
                        }
                        if !rx_chunks.is_empty() || !tx_completed.is_empty() {
                            let ts = current_timestamp_hms_ms();
                            for chunk in rx_chunks {
                                tab.state.append_rx_bytes(&chunk, &ts);
                            }
                            for event in tx_completed {
                                tab.state.record_tx_bytes(&event.bytes, &ts);
                            }
                            tab.log_selection.observe_log_len(tab.state.logs.len());
                        }
                        if let Some(err) = disconnected_err {
                            tab.disconnect_real_serial(Some(err));
                            cx.notify();
                        } else if channel_closed && tab.state.connection.is_connected() {
                            tab.disconnect_real_serial(Some(closed_msg));
                            cx.notify();
                        } else if is_active {
                            cx.notify();
                        }
                        true
                    });
                    match updated {
                        Ok(true) => {}
                        _ => break,
                    }
                }

                if should_break {
                    break;
                }
            }
        });

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.serial_rx_task = Some(task);
        }
    }

    fn connect_real_serial(&mut self, cx: &mut Context<Self>) {
        let tab_id = self.active_tab_id;
        if self.active_tab().connecting {
            return;
        }
        let config = {
            let tab = self.active_tab_mut();
            tab.disconnect_real_serial(None);
            tab.connecting = true;
            tab.state.config.clone()
        };
        let locale = self.current_locale(cx);
        // Opening a port can block for seconds on some drivers; keep it off the UI thread.
        let opening = cx
            .background_executor()
            .spawn(async move { SerialConnection::open_for(&config, locale) });
        cx.spawn(async move |this, cx| {
            let result = opening.await;
            let _ = this.update(cx, |app, cx| {
                app.finish_connect_real_serial(tab_id, result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_connect_real_serial(
        &mut self,
        tab_id: u64,
        result: Result<(SerialConnection, Receiver<SerialEvent>), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return; // tab closed while opening; dropping `result` closes the port
        };
        tab.connecting = false;
        match result {
            Ok((mut conn, rx)) => {
                let writer = conn
                    .take_writer()
                    .expect("newly opened SerialConnection must have a writer");
                let generation = tab.session_service.attach_writer(writer);
                tab.serial_conn = Some(conn);
                tab.state.set_connected(true);
                tab.motion.connection.set(true);
                tab.send_error = None;
                self.start_serial_rx_loop(tab_id, rx, generation, cx);
            }
            Err(err) => {
                if let Some(tab) = self.tab_mut(tab_id) {
                    tab.state.set_connected(false);
                    tab.motion.connection.set(false);
                    tab.send_error = Some(err.into());
                }
            }
        }
        cx.notify();
    }

    fn disconnect_real_serial(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.active_tab_mut().disconnect_real_serial(error);
        cx.notify();
    }

    /// Flips DTR or RTS on the live port without disconnecting.
    ///
    /// The level is applied optimistically and reverted if the writer thread
    /// reports a failure; failures only surface an error and keep the connection.
    fn toggle_modem_line(&mut self, line: ControlLine, cx: &mut Context<Self>) {
        let tab_id = self.active_tab_id;
        let locale = self.current_locale(cx);
        let tab = self.active_tab_mut();
        if tab.state.is_simulated_session || !tab.state.connection.is_connected() {
            return;
        }
        if line == ControlLine::Rts && tab.state.config.flow_control == FlowControl::Hardware {
            return;
        }
        let generation = tab.session_service.generation();
        let new_level = match line {
            ControlLine::Dtr => !tab.state.config.dtr,
            ControlLine::Rts => !tab.state.config.rts,
        };
        let reply_rx = match tab
            .session_service
            .enqueue_control(generation, line, new_level)
        {
            Ok(rx) => rx,
            Err(err) => {
                tab.send_error = Some(err.to_string().into());
                cx.notify();
                return;
            }
        };
        match line {
            ControlLine::Dtr => tab.state.config.dtr = new_level,
            ControlLine::Rts => tab.state.config.rts = new_level,
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { reply_rx.recv().unwrap_or(Err(SessionTxError::Disconnected)) })
                .await;
            let Err(err) = result else { return };
            let _ = this.update(cx, |app, cx| {
                let Some(tab) = app.tab_mut(tab_id) else {
                    return;
                };
                if tab.session_service.generation() != generation {
                    return;
                }
                let name = match line {
                    ControlLine::Dtr => {
                        tab.state.config.dtr = !new_level;
                        "DTR"
                    }
                    ControlLine::Rts => {
                        tab.state.config.rts = !new_level;
                        "RTS"
                    }
                };
                let detail = match &err {
                    SessionTxError::WriteFailed(detail) => detail.clone(),
                    other => other.to_string(),
                };
                tab.send_error = Some(signal_write_failed(locale, name, &detail).into());
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_connection(&mut self, cx: &mut Context<Self>) {
        let is_sim = self.active_tab().state.is_simulated_session;
        let is_conn = self.active_tab().state.connection.is_connected();
        if is_sim {
            self.toggle_simulated_connection(cx);
        } else if is_conn {
            self.disconnect_real_serial(None, cx);
        } else {
            self.connect_real_serial(cx);
        }
    }

    /// Works out how to draw the filter frame for the visible tab: whether it is mounted at all
    /// and whether it is mid-transition.
    ///
    /// A transition plays only when the filter is switched on or off while this tab is on screen;
    /// arriving at a tab that already has a filter on just shows the frame.
    fn frame_state(
        &mut self,
        tab_id: u64,
        filter_on: bool,
        cx: &mut Context<Self>,
    ) -> (bool, Option<FrameTransition>) {
        let same_tab = self.last_frame_tab.replace(tab_id) == Some(tab_id);
        let now = Instant::now();
        let Some(tab) = self.tab_mut(tab_id) else {
            return (filter_on, None);
        };
        let window = Duration::from_millis(FILTER_FRAME_TRANSITION_MS + 120);
        if tab.frame_shown != filter_on {
            tab.frame_shown = filter_on;
            if same_tab {
                tab.frame_key += 1;
                tab.frame_changed_at = Some(now);
                // Nothing else redraws once a leaving frame has finished, so ask for it.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(window).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        }
        let animating = tab
            .frame_changed_at
            .is_some_and(|at| now.duration_since(at) < window);
        let transition = animating.then_some(FrameTransition {
            key: (tab_id << 32) | (tab.frame_key & 0xFFFF_FFFF),
            entering: filter_on,
        });
        // A leaving frame stays mounted until its transition has run.
        (filter_on || animating, transition)
    }

    /// The frame's edge line, kept in step with the theme's accent.
    fn ensure_filter_frame_edge(
        &mut self,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> Entity<FilterFrameEdge> {
        match &self.filter_frame_edge {
            Some(edge) => {
                edge.update(cx, |edge, cx| edge.set_accent(accent, cx));
                edge.clone()
            }
            None => {
                let edge = cx.new(|_| FilterFrameEdge::new(accent));
                self.filter_frame_edge = Some(edge.clone());
                edge
            }
        }
    }

    fn ensure_send_input(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        if let Some(existing) = self.tab_mut(tab_id).and_then(|t| t.send_input.clone()) {
            return existing;
        }

        let (initial_text, is_sim) = self
            .tab_mut(tab_id)
            .map(|t| (t.state.send.text.clone(), t.state.is_simulated_session))
            .unwrap_or_default();
        let locale = self.current_locale(cx);
        let placeholder = if is_sim {
            tr(locale, "send.placeholder_sim")
        } else {
            tr(locale, "send.placeholder")
        };
        let entity = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial_text)
        });
        let entity_for_sub = entity.clone();
        let sub = cx.subscribe_in(
            &entity,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let raw = entity_for_sub.read(cx).value().to_string();
                    let display_hex = this
                        .tab_mut(tab_id)
                        .is_some_and(|t| t.state.send.display_hex);
                    if display_hex {
                        let cursor = entity_for_sub.read(cx).cursor();
                        let (formatted, next_cursor) = format_hex_input_with_cursor(&raw, cursor);
                        if formatted != raw {
                            if let Some(tab) = this.tab_mut(tab_id) {
                                tab.state.send.text = formatted.clone();
                                if tab.send_error.is_some() {
                                    tab.send_error = None;
                                }
                            }
                            entity_for_sub.update(cx, |state, cx| {
                                state.set_value(formatted, window, cx);
                                state.set_cursor_position(
                                    gpui_kit::component::input::Position::new(
                                        0,
                                        next_cursor as u32,
                                    ),
                                    window,
                                    cx,
                                );
                            });
                            cx.notify();
                            return;
                        }
                    }
                    if let Some(tab) = this.tab_mut(tab_id) {
                        tab.state.send.text = raw;
                        if tab.send_error.is_some() {
                            tab.send_error = None;
                        }
                    }
                    cx.notify();
                }
                InputEvent::PressEnter {
                    secondary: true, ..
                } => {
                    if this.active_tab_id == tab_id {
                        this.submit_send(cx);
                    }
                }
                _ => {}
            },
        );

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.send_input = Some(entity.clone());
            tab._send_subscription = Some(sub);
        }
        entity
    }

    fn ensure_picker_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(existing) = &self.picker_input {
            return existing.clone();
        }

        let placeholder = tr(self.current_locale(cx), "picker.input_placeholder");
        let entity = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe_in(
            &entity,
            window,
            |this, _, event: &InputEvent, _window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    this.apply_picker_input(cx);
                }
                InputEvent::Change if this.picker_error.is_some() => {
                    this.picker_error = None;
                    cx.notify();
                }
                _ => {}
            },
        );

        self.picker_input = Some(entity.clone());
        self._picker_subscription = Some(sub);
        entity
    }

    pub fn open_picker(
        &mut self,
        kind: ActivePickerKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().state.active_picker == Some(kind) {
            self.cancel_picker(cx);
            return;
        }

        let locale = self.current_locale(cx);
        let buffer_limit = self.config_store.read(cx).config().log_buffer_limit;
        let (is_sim, current_val) = {
            let tab = self.active_tab_mut();
            tab.state.active_picker = Some(kind);
            tab.motion.picker_open_epoch += 1;
            let current_val = if kind == ActivePickerKind::BufferLimit {
                format_log_buffer_limit(buffer_limit)
            } else {
                tab.state.config.current_value_str_for(kind, locale)
            };
            (tab.state.is_simulated_session, current_val)
        };
        self.picker_error = None;
        if kind == ActivePickerKind::Port && !is_sim {
            self.picker_port_presets = enumerate_serial_ports_for(locale)
                .into_iter()
                .map(|p| p.name)
                .collect();
        }

        let input = self.ensure_picker_input(window, cx);
        input.update(cx, |state, cx| {
            state.set_placeholder(kind.placeholder_for(locale), window, cx);
            state.set_value(current_val, window, cx);
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        cx.notify();
    }

    pub fn apply_picker_value(&mut self, raw: &str, cx: &mut Context<Self>) {
        let Some(kind) = self.active_tab().state.active_picker else {
            return;
        };

        let locale = self.current_locale(cx);
        if kind == ActivePickerKind::BufferLimit {
            self.apply_buffer_limit(raw, locale, cx);
            return;
        }
        let result = self
            .active_tab_mut()
            .state
            .config
            .apply_picker_value_for(kind, raw, locale);
        match result {
            Ok(()) => {
                self.picker_error = None;
                let should_disconnect = {
                    let tab = self.active_tab_mut();
                    tab.state.active_picker = None;
                    !tab.state.is_simulated_session && tab.state.connection.is_connected()
                };
                if should_disconnect {
                    self.disconnect_real_serial(None, cx);
                    return;
                }
            }
            Err(err) => {
                self.picker_error = Some(err.into());
            }
        }
        cx.notify();
    }

    fn apply_buffer_limit(&mut self, raw: &str, locale: Locale, cx: &mut Context<Self>) {
        match parse_log_buffer_limit_for(raw, locale) {
            Ok(limit) => {
                let saved = self.config_store.update(cx, |store, cx| {
                    let result = store.update(|cfg| cfg.log_buffer_limit = limit);
                    cx.notify();
                    result
                });
                match saved {
                    Ok(()) => {
                        self.picker_error = None;
                        for tab in &mut self.tabs {
                            tab.state.set_log_buffer_limit(limit as usize);
                            tab.state.active_picker = None;
                        }
                    }
                    Err(err) => {
                        self.picker_error = Some(err.into());
                    }
                }
            }
            Err(err) => {
                self.picker_error = Some(err.into());
            }
        }
        cx.notify();
    }

    pub fn apply_picker_input(&mut self, cx: &mut Context<Self>) {
        let raw = self
            .picker_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        self.apply_picker_value(&raw, cx);
    }

    pub fn cancel_picker(&mut self, cx: &mut Context<Self>) {
        if self.active_tab().state.active_picker.is_some() || self.picker_error.is_some() {
            self.active_tab_mut().state.active_picker = None;
            self.picker_error = None;
            cx.notify();
        }
    }

    pub fn toggle_simulated_connection(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.toggle_simulated_connection();
        tab.motion
            .connection
            .set(tab.state.connection.is_connected());
        if tab.state.connection.is_connected() {
            tab.send_error = None;
        } else {
            tab.stop_timer_for(StopCause::Disconnected);
        }
        cx.notify();
    }

    pub fn toggle_receive_hex(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.toggle_receive_hex();
        tab.motion.rx_hex.set(tab.state.receive.hex_mode);
        self.sync_log_selection(window, cx);
        cx.notify();
    }

    pub fn toggle_receive_timestamp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.toggle_receive_timestamp();
        tab.motion
            .rx_timestamp
            .set(tab.state.receive.show_timestamp);
        self.sync_log_selection(window, cx);
        cx.notify();
    }

    pub fn toggle_receive_pause(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.toggle_receive_pause();
        tab.motion.rx_pause.set(tab.state.receive.paused);
        cx.notify();
    }

    pub fn toggle_receive_auto_scroll(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.toggle_auto_scroll();
        tab.motion.rx_auto_scroll.set(tab.state.receive.auto_scroll);
        if tab.state.receive.auto_scroll {
            tab.log_list_state.scroll_to_end();
        }
        cx.notify();
    }

    pub fn disable_receive_auto_scroll(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        if tab.state.receive.auto_scroll {
            tab.state.toggle_auto_scroll();
            tab.motion.rx_auto_scroll.set(tab.state.receive.auto_scroll);
            cx.notify();
        }
    }

    pub fn clear_receive_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.active_tab_mut().state.clear_receive_logs();
        self.sync_log_selection(window, cx);
        cx.notify();
    }

    fn sync_log_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        let invalidate = tab.log_selection.sync_display(
            tab.state.receive.show_timestamp,
            tab.state.receive.hex_mode,
            tab.state.logs.len(),
        );
        if invalidate {
            TextSelection::clear(window, cx);
        }
    }

    fn ensure_filter_input(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let locale = self.current_locale(cx);
        let tab = self.tab_mut(tab_id).expect("filter session exists");
        let display = (locale, tab.state.receive.hex_mode);
        let placeholder = tr_shared(
            locale,
            if display.1 {
                "filter.hex_placeholder"
            } else {
                "filter.placeholder"
            },
        );
        if let Some(input) = tab.filter_input.clone() {
            if tab.filter_input_display != Some(display) {
                tab.filter_input_display = Some(display);
                input.update(cx, |state, cx| {
                    state.set_placeholder(placeholder, window, cx)
                });
            }
            return input;
        }
        let query = tab.log_filter.draft.query.clone();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(placeholder);
            state.set_value(query, window, cx);
            state
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                match event {
                    // Typing only edits the draft; the filter is applied
                    // explicitly (Enter or the Apply button).
                    InputEvent::Change => {
                        let query = input.read(cx).value().to_string();
                        if let Some(tab) = this.tab_mut(tab_id) {
                            if tab.log_filter.draft.query == query {
                                return;
                            }
                            tab.log_filter.draft.query = query;
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.apply_log_filter(tab_id, window, cx),
                    _ => {}
                }
            },
        );
        if let Some(tab) = self.tab_mut(tab_id) {
            tab.filter_input = Some(input.clone());
            tab.filter_input_display = Some(display);
            tab._filter_subscription = Some(sub);
        }
        input
    }

    fn toggle_log_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.log_filter.open = !tab.log_filter.open;
        if tab.log_filter.open {
            let input = self.ensure_filter_input(self.active_tab_id, window, cx);
            input.update(cx, |state, cx| state.focus(window, cx));
        } else {
            tab.log_focus.focus(window, cx);
        }
        cx.notify();
    }

    fn change_log_filter(
        &mut self,
        control: FilterControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab_id = self.active_tab_id;
        let tab = self.active_tab_mut();
        let draft = &mut tab.log_filter.draft;
        // Edits stay in the draft until applied; only Apply and Reset take effect.
        match control {
            FilterControl::CycleDirection => draft.direction = draft.direction.next(),
            FilterControl::CaseSensitive => draft.case_sensitive = !draft.case_sensitive,
            FilterControl::Regex => draft.regex = !draft.regex,
            FilterControl::Exclude => draft.exclude = !draft.exclude,
            FilterControl::Reset => *draft = LogFilter::default(),
            FilterControl::Apply => {}
        }
        match control {
            FilterControl::Apply => self.apply_log_filter(tab_id, window, cx),
            FilterControl::Reset => {
                let input = self.active_tab().filter_input.clone();
                self.apply_log_filter(tab_id, window, cx);
                if let Some(input) = input {
                    input.update(cx, |state, cx| state.set_value("", window, cx));
                }
            }
            _ => cx.notify(),
        }
    }

    /// Make the draft the filter in effect (an invalid draft keeps the last
    /// valid filter and only sets the error flag).
    fn apply_log_filter(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let is_active_tab = self.active_tab_id == tab_id;
        if let Some(tab) = self.tab_mut(tab_id) {
            tab.log_filter.apply();
            tab.log_selection.set_selected_text("");
        }
        if is_active_tab {
            TextSelection::clear(window, cx);
        }
        cx.notify();
    }

    fn ensure_search_input(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(existing) = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.search_input.clone())
        {
            return existing;
        }

        let placeholder = tr_shared(
            self.config_store.read(cx).config().locale,
            "search.placeholder",
        );
        let entity = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe_in(
            &entity,
            window,
            move |this, input, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    let query = input.read(cx).value().to_string();
                    let target_line = if let Some(tab) = this.tab_mut(tab_id) {
                        tab.log_search.query = query;
                        tab.recompute_log_search(false);
                        let line = tab.log_search.current().map(|m| m.log_index);
                        if let Some(idx) = line {
                            tab.log_list_state.scroll_to_reveal_item(idx);
                        }
                        line
                    } else {
                        None
                    };
                    let _ = target_line;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } if this.active_tab_id == tab_id => {
                    this.jump_search_match(true, cx);
                }
                _ => {}
            },
        );

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.search_input = Some(entity.clone());
            tab._search_subscription = Some(sub);
        }
        entity
    }

    fn ensure_quick_search_input(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(existing) = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.quick_search_input.clone())
        {
            return existing;
        }

        let placeholder = tr_shared(
            self.config_store.read(cx).config().locale,
            "quick.search_placeholder",
        );
        let entity = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe_in(
            &entity,
            window,
            move |this, input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    if let Some(tab) = this.tab_mut(tab_id) {
                        tab.quick_query = query;
                    }
                    cx.notify();
                }
            },
        );

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.quick_search_input = Some(entity.clone());
            tab._quick_search_sub = Some(sub);
        }

        entity
    }

    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab_id = self.active_tab_id;
        let locale = self.config_store.read(cx).config().locale;
        let (query, logs_len) = {
            let tab = self.active_tab_mut();
            tab.log_search.open = true;
            if !tab.log_search.query.is_empty() {
                tab.recompute_log_search(true);
            }
            (tab.log_search.query.clone(), tab.state.logs.len())
        };
        let _ = logs_len;

        let input = self.ensure_search_input(tab_id, window, cx);
        input.update(cx, |state, cx| {
            state.set_placeholder(tr_shared(locale, "search.placeholder"), window, cx);
            if state.value().as_str() != query {
                state.set_value(query, window, cx);
            }
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        cx.notify();
    }

    pub fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        if !tab.log_search.open {
            return;
        }
        tab.log_search.open = false;
        tab.log_search.matches.clear();
        tab.log_search.active_match = None;
        let log_focus = tab.log_focus.clone();
        log_focus.focus(window, cx);
        cx.notify();
    }

    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().log_search.open {
            self.close_search(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }

    pub fn jump_search_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        let target = if forward {
            tab.log_search.next().map(|m| m.log_index)
        } else {
            tab.log_search.prev().map(|m| m.log_index)
        };
        if let Some(log_index) = target {
            tab.log_list_state.scroll_to_reveal_item(log_index);
        }
        cx.notify();
    }

    fn editor_owns_copy(&self, window: &Window, cx: &App) -> bool {
        let send_focused = self
            .active_tab()
            .send_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        let search_focused = self
            .active_tab()
            .search_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        let picker_focused = self
            .picker_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        let filter_focused = self
            .active_tab()
            .filter_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        send_focused || search_focused || filter_focused || picker_focused
    }

    fn copy_log_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let selected = TextSelection::selected_text(window, cx);
        let tab = self.active_tab_mut();
        tab.log_selection.set_selected_text(&selected);
        let Some(text) = tab.log_selection.copy_payload() else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub fn save_receive_logs(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab();
        let content_to_save = if tab.log_filter.is_active() {
            format_visible_logs(
                &tab.visible_logs(),
                tab.state.receive.show_timestamp,
                tab.state.receive.hex_mode,
            )
        } else {
            tab.state.visible_log_export_text()
        };
        let suggested_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let tab_id = tab.id;
        let rx = cx.prompt_for_new_path(&suggested_dir, Some("sasha-log.txt"));

        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { std::fs::write(&path, content_to_save.as_bytes()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(tab_id) {
                    tab.save_log_error = result.err().map(|err| err.to_string().into());
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn toggle_stream_output(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        if tab.stream_busy {
            return;
        }
        let tab_id = tab.id;
        tab.stream_error = None;
        tab.stream_busy = true;
        if let Some(output) = tab.state.stream_output.take() {
            let finish = cx
                .background_executor()
                .spawn(async move { output.finish() });
            cx.spawn(async move |this, cx| {
                let error = finish.await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(tab) = this.tab_mut(tab_id) {
                        tab.stream_busy = false;
                        tab.stream_error = error.map(Into::into);
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }
        let hex_mode = tab.state.receive.hex_mode;
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let picker = cx.prompt_for_new_path(&dir, Some("sasha-stream.txt"));
        cx.spawn(async move |this, cx| {
            let selected = picker.await;
            let result = match selected {
                Ok(Ok(Some(path))) => Some(
                    cx.background_executor()
                        .spawn(async move {
                            crate::model::stream_output::StreamOutput::open(&path, hex_mode)
                        })
                        .await
                        .map_err(|error| error.to_string()),
                ),
                Ok(Ok(None)) => None,
                _ => Some(Err("Unable to open file picker".into())),
            };
            let _ = this.update(cx, |this, cx| {
                let stream_scope = this.config_store.read(cx).config().stream_output_scope;
                let Some(tab) = this.tab_mut(tab_id) else {
                    return;
                };
                tab.stream_busy = false;
                match result {
                    Some(Ok(output)) => tab.state.start_stream_output(output, stream_scope),
                    Some(Err(error)) => tab.stream_error = Some(error.into()),
                    None => {}
                }
                this.ensure_stream_poller(tab_id, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn ensure_stream_poller(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        if tab.stream_poll_task.is_some() || tab.state.stream_output.is_none() {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let keep = this.update(cx, |this, cx| {
                    let Some(tab) = this.tab_mut(tab_id) else {
                        return false;
                    };
                    let active = if let Some(output) = &mut tab.state.stream_output {
                        output.poll();
                        if let Some(error) = output.error() {
                            tab.stream_error = Some(error.to_owned().into());
                            cx.notify();
                        }
                        output.is_active()
                    } else {
                        false
                    };
                    if !active {
                        tab.stream_poll_task = None;
                    }
                    active
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        });
        if let Some(tab) = self.tab_mut(tab_id) {
            tab.stream_poll_task = Some(task);
        }
    }

    fn load_scripts(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        // GPUI's native picker has no extension-filter option. Validate the selection here.
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(tr_shared(locale, "scripts.load")),
        });
        cx.spawn(async move |this, cx| {
            let result = picker.await;
            let _ = this.update(cx, |this, cx| {
                let locale = this.current_locale(cx);
                let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match result {
                    Ok(Ok(Some(paths))) => {
                        let mut invalid = false;
                        let mut added = false;
                        for path in paths {
                            if path
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("lua"))
                                && path.is_file()
                            {
                                if !tab.script_paths.contains(&path) {
                                    tab.script_paths.push(path);
                                    added = true;
                                }
                            } else {
                                invalid = true;
                            }
                        }
                        tab.script_load_error =
                            invalid.then(|| tr_shared(locale, "scripts.lua_only"));
                        let updated_scripts = added.then(|| tab.script_paths.clone());
                        if let Some(scripts) = updated_scripts {
                            this.persist_scripts(scripts, cx);
                        }
                    }
                    Ok(Ok(None)) => return,
                    _ => tab.script_load_error = Some(tr_shared(locale, "scripts.load_failed")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn ensure_script_poller(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        if tab.script_poll_task.is_some() {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let keep_going = this.update(cx, |app, cx| {
                    let Some(tab) = app.tab_mut(tab_id) else {
                        return false;
                    };
                    let had_script_events = tab.drain_script_events();
                    let had_tx_logs = tab.drain_tx_completed_logs();
                    if had_script_events || had_tx_logs {
                        cx.notify();
                    }
                    let still_running = tab.script_manager.running_count() > 0;
                    if !still_running {
                        tab.script_poll_task = None;
                    }
                    still_running
                });
                match keep_going {
                    Ok(true) => {}
                    _ => break,
                }
            }
        });
        if let Some(tab) = self.tab_mut(tab_id) {
            tab.script_poll_task = Some(task);
        }
    }

    fn run_script(&mut self, tab_id: u64, path: PathBuf, cx: &mut Context<Self>) {
        let started = {
            let Some(tab) = self.tab_mut(tab_id) else {
                return;
            };
            if tab.state.is_simulated_session || !tab.state.connection.is_connected() {
                return;
            }
            let result = std::fs::read_to_string(&path)
                .map_err(|err| err.to_string())
                .and_then(|source| {
                    let generation = tab.session_service.generation();
                    tab.script_manager.start_script(
                        path.clone(),
                        source,
                        &tab.session_service,
                        generation,
                    )
                });
            let entry = tab.script_states.entry(path).or_default();
            match result {
                Ok(run_id) => {
                    entry.run_id = Some(run_id);
                    entry.status = Some(ScriptStatus::Running);
                    tab.drain_script_events();
                    true
                }
                Err(error) => {
                    entry.run_id = None;
                    entry.status = Some(ScriptStatus::Failed { error });
                    false
                }
            }
        };
        if started {
            self.ensure_script_poller(tab_id, cx);
        }
        cx.notify();
    }

    fn stop_script(&mut self, tab_id: u64, run_id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_mut(tab_id) else {
            return;
        };
        tab.script_manager.stop_instance(run_id);
        tab.drain_script_events();
        cx.notify();
    }

    fn persist_scripts(&self, scripts: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.config_store.update(cx, |store, cx| {
            let _ = store.update(|cfg| cfg.scripts = scripts);
            cx.notify();
        });
    }

    fn remove_script(&mut self, tab_id: u64, path: PathBuf, cx: &mut Context<Self>) {
        let updated_scripts = {
            let Some(tab) = self.tab_mut(tab_id) else {
                return;
            };
            if let Some(entry) = tab.script_states.get(&path)
                && let Some(run_id) = entry.run_id
                && entry.status.as_ref().is_some_and(ScriptStatus::is_active)
            {
                tab.script_manager.stop_instance(run_id);
                tab.drain_script_events();
            }
            tab.script_paths.retain(|item| item != &path);
            tab.script_states.remove(&path);
            tab.script_paths.clone()
        };
        self.persist_scripts(updated_scripts, cx);
        cx.notify();
    }

    pub fn toggle_send_display_hex(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let send_input = self.active_tab().send_input.clone();
        if let Some(input) = &send_input {
            let value = input.read(cx).value().to_string();
            self.active_tab_mut().state.send.text = value;
        }

        let locale = self.current_locale(cx);
        let res = self
            .active_tab_mut()
            .state
            .toggle_send_display_hex_for(locale);
        match res {
            Ok(()) => {
                let (display_hex, next_text) = {
                    let tab = self.active_tab_mut();
                    tab.motion.tx_display_hex.set(tab.state.send.display_hex);
                    tab.send_error = None;
                    (tab.state.send.display_hex, tab.state.send.text.clone())
                };
                let _ = display_hex;
                if let Some(input) = &send_input {
                    input.update(cx, |state, cx| {
                        state.set_value(next_text, window, cx);
                    });
                }
            }
            Err(err) => {
                self.active_tab_mut().send_error = Some(err.into());
            }
        }
        cx.notify();
    }

    /// Folds the send card into a read-only strip (observe mode) or restores it.
    /// Collapsing moves focus to the log; expanding focuses the input.
    pub fn toggle_send_collapsed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let collapsed = {
            let tab = self.active_tab_mut();
            tab.state.send.collapsed = !tab.state.send.collapsed;
            tab.state.send.collapsed
        };
        if collapsed {
            self.active_tab().log_focus.clone().focus(window, cx);
        } else if let Some(input) = self.active_tab().send_input.clone() {
            input.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    pub fn toggle_send_format_hex(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.toggle_send_format_hex();
        tab.motion.tx_send_hex.set(tab.state.send.send_hex);
        tab.send_error = None;
        cx.notify();
    }

    pub fn cycle_send_line_ending(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.state.cycle_send_line_ending();
        tab.motion
            .tx_line_ending
            .set(tab.state.send.line_ending.is_active());
        cx.notify();
    }

    fn enqueue_real_tx(
        &mut self,
        tab_id: u64,
        bytes: Vec<u8>,
        slot: SendErrorSlot,
        cx: &mut Context<Self>,
    ) {
        let locale = self.current_locale(cx);
        let Some((service, generation, timer_run)) = self.tab_mut(tab_id).map(|tab| {
            (
                tab.session_service.clone(),
                tab.session_service.generation(),
                tab.timer_run_id,
            )
        }) else {
            return;
        };
        match service.enqueue_tx(generation, bytes, None) {
            Ok(reply_rx) => {
                if slot == SendErrorSlot::Timer
                    && let Some(tab) = self.tab_mut(tab_id)
                {
                    tab.timer_pending += 1;
                }
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move { reply_rx.recv() })
                        .await;
                    let _ = this.update(cx, |app, cx| {
                        let locale = app.current_locale(cx);
                        let Some(tab) = app.tab_mut(tab_id) else {
                            return;
                        };
                        tab.drain_tx_completed_logs();
                        // A reply for an earlier timer run must not touch the current one
                        // (no stop, no error, no disconnect of a newer connection).
                        if slot == SendErrorSlot::Timer
                            && !timer_reply_is_current(
                                timer_run,
                                tab.timer_run_id,
                                tab.timer.running,
                            )
                        {
                            cx.notify();
                            return;
                        }
                        if slot == SendErrorSlot::Timer {
                            tab.timer_pending = tab.timer_pending.saturating_sub(1);
                        }
                        let stop_timer = slot == SendErrorSlot::Timer
                            && matches!(&result, Ok(Err(err)) if should_stop_on_tx_error(err));
                        match result {
                            Ok(Ok(())) => {
                                tab.set_send_error(slot, None);
                            }
                            Ok(Err(SessionTxError::Disconnected))
                            | Ok(Err(SessionTxError::StaleGeneration { .. })) => {
                                if tab.state.connection.is_connected() {
                                    tab.disconnect_real_serial(Some(
                                        tr(locale, "err.send_disconnected").into_owned(),
                                    ));
                                    // 发送卡沿用原行为（仅走断开流程）；快捷面板另显示自己的错误。
                                    if slot == SendErrorSlot::QuickPanel {
                                        tab.set_send_error(
                                            slot,
                                            Some(tr_shared(locale, "err.send_disconnected")),
                                        );
                                    }
                                } else {
                                    tab.set_send_error(
                                        slot,
                                        Some(tr_shared(locale, "err.send_disconnected")),
                                    );
                                }
                            }
                            Ok(Err(SessionTxError::WriteFailed(err))) => {
                                tab.set_send_error(slot, Some(err.into()));
                            }
                            Ok(Err(SessionTxError::Cancelled)) => {}
                            Ok(Err(SessionTxError::QueueFull)) => {
                                tab.set_send_error(
                                    slot,
                                    Some(SessionTxError::QueueFull.to_string().into()),
                                );
                            }
                            Err(_) => {}
                        }
                        if stop_timer {
                            tab.stop_timer_for(StopCause::TxFailed);
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            Err(SessionTxError::Disconnected) | Err(SessionTxError::StaleGeneration { .. }) => {
                if let Some(tab) = self.tab_mut(tab_id) {
                    tab.disconnect_real_serial(Some(
                        tr(locale, "err.send_disconnected").into_owned(),
                    ));
                    if slot == SendErrorSlot::QuickPanel {
                        tab.set_send_error(slot, Some(tr_shared(locale, "err.send_disconnected")));
                    }
                }
                cx.notify();
            }
            Err(err) => {
                if let Some(tab) = self.tab_mut(tab_id) {
                    tab.set_send_error(slot, Some(err.to_string().into()));
                    if slot == SendErrorSlot::Timer {
                        tab.stop_timer_for(StopCause::TxFailed);
                    }
                }
                cx.notify();
            }
        }
    }

    pub fn send_quick_command(&mut self, command_id: &str, cx: &mut Context<Self>) {
        let Some(cmd) = self
            .config_store
            .read(cx)
            .config()
            .quick_commands
            .iter()
            .find(|c| c.id == command_id)
            .cloned()
        else {
            return;
        };

        let locale = self.current_locale(cx);
        if manual_send_blocked(self.active_tab().timer.mode()) {
            self.active_tab_mut().set_send_error(
                SendErrorSlot::QuickPanel,
                Some(tr_shared(locale, "err.timer_enabled")),
            );
            cx.notify();
            return;
        }
        let is_simulated = self.active_tab().state.is_simulated_session;
        if is_simulated {
            let res = self
                .active_tab_mut()
                .state
                .submit_command_with_timestamp(&cmd, current_timestamp_hms_ms());
            match res {
                Ok(_) => {
                    self.active_tab_mut()
                        .set_send_error(SendErrorSlot::QuickPanel, None);
                }
                Err(err) => {
                    let msg = send_payload_error_text(locale, true, &err);
                    self.active_tab_mut()
                        .set_send_error(SendErrorSlot::QuickPanel, Some(msg));
                }
            }
            cx.notify();
            return;
        }

        match self.active_tab().state.prepare_command_bytes(&cmd) {
            Ok(bytes) => {
                self.enqueue_real_tx(self.active_tab_id, bytes, SendErrorSlot::QuickPanel, cx);
            }
            Err(err) => {
                let msg = send_payload_error_text(locale, false, &err);
                self.active_tab_mut()
                    .set_send_error(SendErrorSlot::QuickPanel, Some(msg));
                cx.notify();
            }
        }
    }

    fn persist_quick_commands(
        &self,
        commands: Vec<QuickCommand>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        persist_quick_commands(&self.config_store, commands, cx)
    }

    /// 在独立窗口中新建（`editing = None`）或编辑快捷命令。
    fn open_quick_editor(&mut self, editing: Option<String>, cx: &mut Context<Self>) {
        let draft = match editing {
            Some(ref id) => {
                let Some(cmd) = self
                    .config_store
                    .read(cx)
                    .config()
                    .quick_commands
                    .iter()
                    .find(|c| &c.id == id)
                else {
                    return;
                };
                QuickEditorDraft::from_command(cmd)
            }
            None => QuickEditorDraft::blank(),
        };

        let fill = {
            let tab = self.active_tab();
            let text = tab
                .send_input
                .as_ref()
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_else(|| tab.state.send.text.clone());
            QuickFillSource {
                text,
                hex: tab.state.send.send_hex,
                line_ending: tab.state.send.line_ending,
            }
        };

        // 同一时间只保留一个编辑窗口：已有则先关闭，再打开新的。
        if let Some(handle) = self.quick_editor_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }

        let bounds = Bounds::centered(None, size(px(480.), px(500.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(420.), px(440.))),
            is_resizable: true,
            ..TitleBar::window_options()
        };
        let config_store = self.config_store.clone();
        if let Ok((handle, _)) = open_window(options, cx, move |window, cx| {
            let store = config_store.clone();
            let draft = draft.clone();
            let fill = fill.clone();
            cx.new(move |cx| QuickCommandWindow::new(store, draft, fill, window, cx))
        }) {
            self.quick_editor_window = Some(handle);
        }
    }

    fn delete_quick_command(&mut self, tab_id: u64, id: String, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        let mut next_commands = self.config_store.read(cx).config().quick_commands.clone();
        if !remove_command(&mut next_commands, &id) {
            if let Some(tab) = self.tab_mut(tab_id) {
                tab.quick_pending_delete = None;
            }
            cx.notify();
            return;
        }

        if let Err(err) = self.persist_quick_commands(next_commands, cx) {
            if let Some(tab) = self.tab_mut(tab_id) {
                tab.quick_error = Some(quick_save_failed(locale, &err).into());
                tab.quick_pending_delete = None;
            }
            cx.notify();
            return;
        }

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.quick_pending_delete = None;
        }
        cx.notify();
    }

    pub fn submit_send(&mut self, cx: &mut Context<Self>) {
        match enter_action(self.active_tab().timer.mode()) {
            EnterAction::SendNow => {}
            EnterAction::StartTimer => {
                self.start_timer(cx);
                return;
            }
            EnterAction::IgnoreRunning => {
                let locale = self.current_locale(cx);
                self.active_tab_mut().send_error =
                    Some(tr_shared(locale, "err.timer_running_enter"));
                cx.notify();
                return;
            }
        }
        let send_input = self.active_tab().send_input.clone();
        if let Some(input) = &send_input {
            let value = input.read(cx).value().to_string();
            self.active_tab_mut().state.send.text = value;
        }

        let locale = self.current_locale(cx);
        let is_simulated = self.active_tab().state.is_simulated_session;
        if is_simulated {
            let res = self
                .active_tab_mut()
                .state
                .submit_send_with_timestamp(current_timestamp_hms_ms());
            match res {
                Ok(_) => {
                    self.active_tab_mut()
                        .set_send_error(SendErrorSlot::SendCard, None);
                }
                Err(err) => {
                    let msg = send_payload_error_text(locale, true, &err);
                    self.active_tab_mut()
                        .set_send_error(SendErrorSlot::SendCard, Some(msg));
                }
            }
            cx.notify();
            return;
        }

        match self.active_tab().state.prepare_send_bytes() {
            Ok(bytes) => {
                self.enqueue_real_tx(self.active_tab_id, bytes, SendErrorSlot::SendCard, cx);
            }
            Err(err) => {
                let msg = send_payload_error_text(locale, false, &err);
                self.active_tab_mut()
                    .set_send_error(SendErrorSlot::SendCard, Some(msg));
                cx.notify();
            }
        }
    }

    fn ensure_timer_input(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(existing) = self.tab_mut(tab_id).and_then(|t| t.timer_input.clone()) {
            return existing;
        }

        let locale = self.current_locale(cx);
        let saved = self.config_store.read(cx).config().timer_interval_ms;
        let placeholder = tr(locale, "send.timer_interval_placeholder");
        let entity = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        entity.update(cx, |state, cx| {
            state.set_value(saved.to_string(), window, cx);
        });
        let sub = cx.subscribe_in(
            &entity,
            window,
            move |this, _input, event: &InputEvent, window, cx| match event {
                // Typing only edits the draft (shown as pending); nothing applies yet.
                InputEvent::Change => cx.notify(),
                // Enter only commits the draft; it never starts the timer.
                InputEvent::PressEnter { .. } if this.active_tab_id == tab_id => {
                    this.commit_timer_interval(tab_id, window, cx);
                }
                InputEvent::Blur => this.commit_timer_interval(tab_id, window, cx),
                _ => {}
            },
        );

        if let Some(tab) = self.tab_mut(tab_id) {
            tab.timer.interval_ms = saved;
            tab.timer_input = Some(entity.clone());
            tab._timer_input_sub = Some(sub);
        }
        entity
    }

    /// Commits the interval field's draft (Enter / blur). A valid value applies from
    /// the next cycle; an invalid one keeps the previous interval, restores the field
    /// and shows the interval error.
    fn commit_timer_interval(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let locale = self.current_locale(cx);
        let Some((input, committed)) = self
            .tab_mut(tab_id)
            .and_then(|tab| tab.timer_input.clone().map(|i| (i, tab.timer.interval_ms)))
        else {
            return;
        };
        let draft = input.read(cx).value().to_string();
        let (text, error) = match commit_interval_draft(&draft, committed) {
            DraftCommit::Commit(ms) => {
                if let Some(tab) = self.tab_mut(tab_id) {
                    tab.timer.interval_ms = ms;
                }
                (ms.to_string(), false)
            }
            DraftCommit::Reject { restore_ms, .. } => (restore_ms.to_string(), true),
        };
        if draft != text {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
        if error && let Some(tab) = self.tab_mut(tab_id) {
            tab.send_error = Some(
                timer_interval_error(locale, MIN_TIMER_INTERVAL_MS, MAX_TIMER_INTERVAL_MS).into(),
            );
        }
        cx.notify();
    }

    fn persist_timer_interval(&self, interval_ms: u32, cx: &mut Context<Self>) {
        if self.config_store.read(cx).config().timer_interval_ms == interval_ms {
            return;
        }
        self.config_store.update(cx, |store, cx| {
            let _ = store.update(|cfg| cfg.timer_interval_ms = interval_ms);
            cx.notify();
        });
    }

    /// Icon-segment click: only toggles the enabled state (never sends).
    pub fn toggle_timer_enabled(&mut self, cx: &mut Context<Self>) {
        let (enabled, connected) = {
            let tab = self.active_tab();
            (tab.timer.enabled, tab.state.connection.is_connected())
        };
        match icon_click_action(enabled, connected) {
            IconAction::Enable => {
                let tab = self.active_tab_mut();
                tab.timer.enabled = true;
                tab.timer_flag.set(true);
                tab.send_error = None;
                cx.notify();
            }
            IconAction::Disable => {
                let interval = {
                    let tab = self.active_tab_mut();
                    tab.stop_timer_for(StopCause::Disabled);
                    tab.timer.interval_ms
                };
                self.persist_timer_interval(interval, cx);
                cx.notify();
            }
            IconAction::Ignore => {}
        }
    }

    /// Send-button click: normal send, play (start) or pause depending on the state.
    pub fn press_send_button(&mut self, cx: &mut Context<Self>) {
        match send_button_action(self.active_tab().timer.mode()) {
            SendButtonAction::SendNow => self.submit_send(cx),
            SendButtonAction::StartTimer => self.start_timer(cx),
            SendButtonAction::StopTimer => self.stop_timer_by_user(cx),
        }
    }

    /// Validates the interval and the current draft, then starts repeating sends.
    /// The first send happens immediately; later ones follow at a fixed rate.
    pub fn start_timer(&mut self, cx: &mut Context<Self>) {
        let tab_id = self.active_tab_id;
        let locale = self.current_locale(cx);
        {
            let tab = self.active_tab();
            if !tab.timer.enabled || tab.timer.running || !tab.state.connection.is_connected() {
                return;
            }
        }
        let raw_interval = self
            .active_tab()
            .timer_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string());
        let send_text = self
            .active_tab()
            .send_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string());

        let interval =
            interval_for_start(raw_interval.as_deref(), self.active_tab().timer.interval_ms);
        let Ok(interval) = interval else {
            self.active_tab_mut().send_error = Some(
                timer_interval_error(locale, MIN_TIMER_INTERVAL_MS, MAX_TIMER_INTERVAL_MS).into(),
            );
            cx.notify();
            return;
        };

        {
            let tab = self.active_tab_mut();
            if let Some(text) = send_text {
                tab.state.send.text = text;
            }
            if let Err(err) = tab.state.prepare_send_bytes() {
                tab.send_error = Some(send_payload_error_text(
                    locale,
                    tab.state.is_simulated_session,
                    &err,
                ));
                cx.notify();
                return;
            }
            tab.send_error = None;
            tab.timer.interval_ms = interval;
            tab.timer.running = true;
            tab.timer_run_id += 1;
            tab.timer_pending = 0;
        }
        self.persist_timer_interval(interval, cx);
        let task = self.spawn_timer_task(tab_id, cx);
        self.active_tab_mut().timer_task = Some(task);
        cx.notify();
    }

    /// Pause: stops sending but keeps the capsule enabled.
    pub fn stop_timer_by_user(&mut self, cx: &mut Context<Self>) {
        let interval = {
            let tab = self.active_tab_mut();
            tab.stop_timer_for(StopCause::Paused);
            tab.timer.interval_ms
        };
        self.persist_timer_interval(interval, cx);
        cx.notify();
    }

    fn spawn_timer_task(&self, tab_id: u64, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let mut deadline = Instant::now();
            loop {
                let now = Instant::now();
                if deadline > now {
                    cx.background_executor().timer(deadline - now).await;
                }
                let next = this.update(cx, |app, cx| app.timer_tick(tab_id, cx));
                let Ok(Some(interval_ms)) = next else {
                    break;
                };
                deadline = next_deadline(
                    deadline,
                    Instant::now(),
                    Duration::from_millis(u64::from(interval_ms)),
                );
            }
        })
    }

    /// Runs one timed-send cycle against the tab's current draft. Returns the
    /// interval to wait before the next cycle, or `None` once the timer stopped.
    fn timer_tick(&mut self, tab_id: u64, cx: &mut Context<Self>) -> Option<u32> {
        let locale = self.current_locale(cx);
        let send_text = self
            .tab_mut(tab_id)?
            .send_input
            .clone()
            .map(|input| input.read(cx).value().to_string());
        let tab = self.tab_mut(tab_id)?;
        if !tab.timer.running {
            return None;
        }
        if let Some(text) = send_text {
            tab.state.send.text = text;
        }

        if tab.state.is_simulated_session {
            let result = tab
                .state
                .submit_send_with_timestamp(current_timestamp_hms_ms());
            let next = match result {
                Ok(_) => {
                    tab.send_error = None;
                    Some(tab.timer.interval_ms)
                }
                Err(err) => {
                    tab.stop_timer_for(stop_cause_for(&err));
                    tab.send_error = Some(send_payload_error_text(locale, true, &err));
                    None
                }
            };
            cx.notify();
            return next;
        }

        if should_skip_cycle(tab.timer_pending) {
            // The port is not keeping up: skip this cycle rather than overflow the queue.
            return Some(tab.timer.interval_ms);
        }
        match tick_outcome(tab.state.prepare_send_bytes()) {
            TickOutcome::Send(bytes) => {
                self.enqueue_real_tx(tab_id, bytes, SendErrorSlot::Timer, cx);
            }
            TickOutcome::Stop(err) => {
                let tab = self.tab_mut(tab_id)?;
                tab.stop_timer_for(stop_cause_for(&err));
                tab.send_error = Some(send_payload_error_text(locale, false, &err));
                cx.notify();
                return None;
            }
        }
        let tab = self.tab_mut(tab_id)?;
        tab.timer.running.then_some(tab.timer.interval_ms)
    }

    fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window
            && handle
                .update(cx, |_, window, _| {
                    window.activate_window();
                })
                .is_ok()
        {
            return;
        }

        let bounds = Bounds::centered(None, size(px(620.), px(460.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(520.), px(400.))),
            is_resizable: true,
            ..TitleBar::window_options()
        };

        let config_store = self.config_store.clone();
        if let Ok((handle, _)) = open_window(options, cx, move |window, cx| {
            let store = config_store.clone();
            cx.new(move |cx| SettingsWindow::new(store, window, cx))
        }) {
            self.settings_window = Some(handle);
        }
    }

    fn sample_all_throughputs(&mut self, cx: &mut Context<Self>) {
        self.last_throughput_sample_at = Instant::now();
        for tab in &mut self.tabs {
            let rx = tab.state.counters.rx_bytes;
            let tx = tab.state.counters.tx_bytes;
            tab.state.throughput.sample(rx, tx);
        }
        self.notify_throughput_window(false, cx);
    }

    fn notify_throughput_window(&mut self, tab_changed: bool, cx: &mut Context<Self>) {
        let Some((_, weak_view)) = &self.throughput_window else {
            return;
        };
        let Some(view) = weak_view.upgrade() else {
            self.throughput_window = None;
            return;
        };
        let span = view.read(cx).span();
        let snapshot = self.active_throughput_snapshot(span);
        view.update(cx, |view, cx| {
            view.apply_snapshot(snapshot, tab_changed, cx)
        });
    }

    pub fn active_throughput_snapshot(&self, span: ThroughputSpan) -> ThroughputSnapshot {
        let tab = self.active_tab();
        let config = tab.state.config.clone();
        let session_title = if config.port.is_empty() {
            format!("Session {}", tab.id)
        } else {
            config.port.clone()
        };
        let theoretical_max = theoretical_max_bytes_per_sec(&config);
        ThroughputSnapshot {
            session_title,
            config,
            counters: tab.state.counters,
            samples: tab.state.throughput.window_samples(span),
            summary: tab.state.throughput.summary(span),
            theoretical_max,
            sampled_at: self.last_throughput_sample_at,
        }
    }

    pub fn clear_active_throughput(&mut self) {
        self.active_tab_mut().state.throughput.clear();
    }

    fn open_throughput(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((handle, _)) = &self.throughput_window
            && handle
                .update(cx, |_, window, _| {
                    window.activate_window();
                })
                .is_ok()
        {
            return;
        }

        let bounds = Bounds::centered(None, size(px(640.), px(420.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(480.), px(340.))),
            is_resizable: true,
            ..TitleBar::window_options()
        };

        let weak_app = cx.entity().downgrade();
        let config_store = self.config_store.clone();
        let initial_snapshot = self.active_throughput_snapshot(ThroughputSpan::default());
        let mut created_view: Option<WeakEntity<ThroughputWindow>> = None;
        if let Ok((handle, _)) = open_window(options, cx, |_, cx| {
            let view = cx.new(|cx| {
                ThroughputWindow::new(
                    weak_app.clone(),
                    config_store.clone(),
                    initial_snapshot.clone(),
                    cx,
                )
            });
            created_view = Some(view.downgrade());
            view
        }) && let Some(weak_view) = created_view
        {
            self.throughput_window = Some((handle, weak_view));
        }
    }

    fn build_tab_rail(&self, theme: NanoTheme, cx: &mut Context<Self>) -> TabRail {
        let weak = cx.entity().downgrade();
        let on_select_tab = Rc::new(move |tab_id: u64, window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.select_tab(tab_id, window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_close_tab = Rc::new(move |tab_id: u64, window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.close_tab(tab_id, window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_new_tab = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.new_tab(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_open_settings = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.open_settings(window, cx));
        });

        let locale = self.current_locale(cx);
        TabRail::new(
            theme,
            locale,
            self.tab_items(locale),
            on_select_tab,
            on_close_tab,
            on_new_tab,
            on_open_settings,
            self.update_manager.read(cx).has_update(),
        )
        .new_shortcut(
            self.config_store
                .read(cx)
                .config()
                .shortcuts
                .display(ShortcutAction::NewSession),
        )
    }

    fn build_top_bar(&self, script_width: Pixels) -> impl IntoElement {
        div()
            .id("top-bar-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(34.))
            .child(
                div()
                    .id("terminal-title-drag")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right(TAB_RAIL_WIDTH + script_width)
                    .h_full()
                    .occlude()
                    .cursor_default()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        gpui_kit::base::GlobalState::suppress_text_selection(cx);
                    }),
            )
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .w(TAB_RAIL_WIDTH)
                    .h_full()
                    .occlude()
                    .child(
                        TitleBar::new()
                            .bg(gpui_kit::transparent_black())
                            .border_b_0(),
                    ),
            )
    }
}

pub(crate) fn image_format_for_path(path: &Path) -> Option<ImageFormat> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("png") {
        Some(ImageFormat::Png)
    } else if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
        Some(ImageFormat::Jpeg)
    } else if ext.eq_ignore_ascii_case("webp") {
        Some(ImageFormat::Webp)
    } else if ext.eq_ignore_ascii_case("gif") {
        Some(ImageFormat::Gif)
    } else if ext.eq_ignore_ascii_case("bmp") {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

fn load_background_image(path: &Path) -> Option<Arc<Image>> {
    let format = image_format_for_path(path)?;
    let bytes = std::fs::read(path).ok()?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

impl Render for SashaApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.current_locale(cx);
        self.sync_placeholders_if_locale_changed(locale, window, cx);
        self.theme = self.theme_manager.read(cx).nano();
        let (bg_image, bg_opacity) = self.sync_background_image(cx);
        let has_bg_image = bg_image.is_some() && bg_opacity > 0;
        let surface_theme = if has_bg_image {
            self.theme
                .with_surface_opacity(self.theme_manager.read(cx).surface_opacity())
        } else {
            self.theme
        };
        let bg_layer = self.build_background_layer(bg_image, bg_opacity);
        let tab_rail = self.build_tab_rail(surface_theme, cx);
        let active_id = self.active_tab_id;

        if self.active_tab().page == AppPage::Start {
            let manual_port_input = self.ensure_start_manual_port_input(active_id, window, cx);
            let baud_input = self.ensure_start_baud_input(active_id, window, cx);
            let start_page_state = self.active_tab().start_page.clone();

            let weak = cx.entity().downgrade();
            let on_refresh_ports = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.refresh_start_ports(cx));
            });
            let weak = cx.entity().downgrade();
            let on_select_port = Rc::new(move |port: String, _: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.select_start_port(port, cx));
            });
            let weak = cx.entity().downgrade();
            let on_enable_manual_port = Rc::new(move |window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.enable_start_manual_port(window, cx));
            });
            let weak = cx.entity().downgrade();
            let on_toggle_baud_dropdown = Rc::new(move |window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.toggle_start_baud_dropdown(window, cx));
            });
            let weak = cx.entity().downgrade();
            let on_select_baud = Rc::new(move |baud: u32, _: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.select_start_baud(baud, cx));
            });
            let weak = cx.entity().downgrade();
            let on_select_data_bits =
                Rc::new(move |bits: DataBits, _: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| this.select_start_data_bits(bits, cx));
                });
            let weak = cx.entity().downgrade();
            let on_select_parity = Rc::new(move |parity: Parity, _: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.select_start_parity(parity, cx));
            });
            let weak = cx.entity().downgrade();
            let on_select_stop_bits =
                Rc::new(move |stop_bits: StopBits, _: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| this.select_start_stop_bits(stop_bits, cx));
                });
            let weak = cx.entity().downgrade();
            let on_select_flow = Rc::new(move |flow: FlowControl, _: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.select_start_flow_control(flow, cx));
            });
            let weak = cx.entity().downgrade();
            let on_toggle_start_dtr = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.toggle_start_line(ControlLine::Dtr, cx));
            });
            let weak = cx.entity().downgrade();
            let on_toggle_start_rts = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.toggle_start_line(ControlLine::Rts, cx));
            });
            let weak = cx.entity().downgrade();
            let on_connect = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.connect_from_start_page(cx));
            });

            return v_flex()
                .id("sasha-root")
                .relative()
                .size_full()
                .bg(self.theme.bg_canvas)
                .text_color(self.theme.text_primary)
                .when_some(bg_layer, |this, layer| this.child(layer))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if this.handle_tab_shortcut(event, window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .child(
                    h_flex()
                        .size_full()
                        .child(
                            div().flex_1().min_w_0().h_full().child(
                                StartPage::new(
                                    surface_theme,
                                    locale,
                                    start_page_state,
                                    manual_port_input,
                                    baud_input,
                                    on_refresh_ports,
                                    on_select_port,
                                    on_enable_manual_port,
                                    on_toggle_baud_dropdown,
                                    on_select_baud,
                                    on_select_data_bits,
                                    on_select_parity,
                                    on_select_stop_bits,
                                    on_connect,
                                )
                                .flow_control_callbacks(
                                    on_select_flow,
                                    on_toggle_start_dtr,
                                    on_toggle_start_rts,
                                ),
                            ),
                        )
                        .child(tab_rail),
                )
                .child(self.build_top_bar(px(0.)))
                .into_any_element();
        }

        let (log_font_size, log_buffer_limit) = {
            let cfg = self.config_store.read(cx).config();
            (cfg.log_font_size, cfg.log_buffer_limit)
        };
        {
            let tab = self.active_tab_mut();
            let log_len = tab.state.logs.len();
            tab.log_selection.observe_log_len(log_len);
            tab.sync_log_list_state(log_font_size);
        }
        let filter_on = self.active_tab().log_filter.is_active();
        let (frame_mounted, frame_transition) = self.frame_state(active_id, filter_on, cx);
        // The line sits at the content's final edge, so wait until the content has got there.
        let frame_edge = (filter_on && frame_transition.is_none())
            .then(|| self.ensure_filter_frame_edge(surface_theme.accent_blue, cx));
        let send_input = self.ensure_send_input(active_id, window, cx);
        let timer_input = self.ensure_timer_input(active_id, window, cx);
        let timer_draft_pending = {
            let committed = self.active_tab().timer.interval_ms;
            draft_is_pending(&timer_input.read(cx).value(), committed)
        };
        let search_open = self.active_tab().log_search.open;
        let search_input = if search_open {
            Some(self.ensure_search_input(active_id, window, cx))
        } else {
            None
        };
        let filter_open = self.active_tab().log_filter.open;
        let filter_input = if filter_open {
            Some(self.ensure_filter_input(active_id, window, cx))
        } else {
            None
        };
        let tab = self.active_tab();
        let filter_active = tab.log_filter.is_active();
        let filter_draft = tab.log_filter.draft.clone();
        let filter_invalid = tab.log_filter.invalid;
        let filter_pending = tab.log_filter.has_pending();
        let filter_total = tab.state.logs.len();
        let filter_visible = tab.log_projection.rows.len();
        let search_matches = if search_open {
            Rc::new(tab.log_search.matches.clone())
        } else {
            Rc::new(Vec::new())
        };
        let active_search_match = if search_open {
            tab.log_search.active_match
        } else {
            None
        };
        let search_count_label = tab.log_search.count_label().map(SharedString::from);
        let search_has_matches = !tab.log_search.matches.is_empty();
        let log_focus = tab.log_focus.clone();
        let log_list_state = tab.log_list_state.clone();
        let logs = tab.log_projection.entries(&tab.state.logs);
        let receive_state = tab.state.receive.clone();
        let simulated_notice = tab.state.simulated_notice_for(locale);
        let send_state = tab.state.send.clone();
        let send_collapsed = send_state.collapsed;
        let motion = tab.motion;
        let is_connected = tab.state.connection.is_connected();
        let is_simulated = tab.state.is_simulated_session;
        let send_error = tab.send_error.clone();
        let timer_state = tab.timer;
        let timer_flag = tab.timer_flag;
        let config = tab.state.config.clone();
        let connection = tab.state.connection;
        let conn_action_label = if tab.connecting {
            tr(locale, "status.action_connecting")
        } else {
            tab.state.connection_action_label_for(locale)
        };
        let counters = tab.state.counters;
        let buffer_used = tab.state.log_buffer_used_bytes();
        let active_picker = tab.state.active_picker;
        let picker_overlay = active_picker.zip(self.picker_input.clone());
        let markers = Rc::new(tab.state.markers.clone());
        let weak_mark = cx.entity().downgrade();
        let on_mark = Rc::new(
            move |entry_id: u64, position: Point<Pixels>, window: &mut Window, cx: &mut App| {
                let _ = weak_mark.update(cx, |this, cx| {
                    this.open_marker_editor(entry_id, position, window, cx)
                });
            },
        );
        let weak_jump = cx.entity().downgrade();
        let on_jump = Rc::new(move |entry_id: u64, _: &mut Window, cx: &mut App| {
            let _ = weak_jump.update(cx, |this, cx| this.jump_to_log_entry(entry_id, cx));
        });
        let weak_disable_auto_scroll = cx.entity().downgrade();
        let on_disable_auto_scroll = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak_disable_auto_scroll
                .update(cx, |this, cx| this.disable_receive_auto_scroll(cx));
        });
        let marker_items: Vec<MarkerPanelItem> = {
            let tab = self.active_tab();
            let visible_ids: Vec<u64> = tab.log_projection.rows.iter().map(|row| row.id).collect();
            panel_items(&tab.state.markers, &visible_ids)
                .into_iter()
                .filter_map(|(entry_id, visible)| {
                    let marker = tab.state.markers.get(entry_id)?;
                    let index = tab
                        .state
                        .logs
                        .binary_search_by_key(&entry_id, |entry| entry.id)
                        .ok()?;
                    let entry = &tab.state.logs[index];
                    Some(MarkerPanelItem {
                        entry_id,
                        name: marker.name.clone(),
                        color: marker.color,
                        label: format!("{} {}", entry.timestamp, entry.direction.tag()),
                        filtered: !visible,
                    })
                })
                .collect()
        };
        let marker_collapsed = self.active_tab().marker_collapsed;
        // A pending "remove all" confirmation is only shown while there is something to remove.
        let marker_pending_clear =
            self.active_tab().marker_pending_clear && !marker_items.is_empty();
        let weak = cx.entity().downgrade();
        let on_marker_toggle = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                let tab = this.active_tab_mut();
                tab.marker_collapsed = !tab.marker_collapsed;
                cx.notify();
            });
        });
        let weak = cx.entity().downgrade();
        let on_marker_expand = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.script_preferred_width = SCRIPT_PANEL_WIDTH;
                    tab.marker_collapsed = false;
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_marker_jump = Rc::new(move |entry_id: u64, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.jump_to_log_entry(entry_id, cx));
        });
        let weak = cx.entity().downgrade();
        let on_marker_edit = Rc::new(
            move |entry_id: u64, position: Point<Pixels>, window: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| {
                    this.open_marker_editor(entry_id, position, window, cx)
                });
            },
        );
        let weak = cx.entity().downgrade();
        let on_marker_remove = Rc::new(move |entry_id: u64, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.remove_marker(entry_id, cx));
        });
        let weak = cx.entity().downgrade();
        let on_marker_request_clear = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.active_tab_mut().marker_pending_clear = true;
                cx.notify();
            });
        });
        let weak = cx.entity().downgrade();
        let on_marker_confirm_clear = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.clear_markers(cx));
        });
        let weak = cx.entity().downgrade();
        let on_marker_cancel_clear = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.active_tab_mut().marker_pending_clear = false;
                cx.notify();
            });
        });
        let marker_panel = MarkerPanel {
            theme: surface_theme,
            locale,
            items: marker_items,
            collapsed: marker_collapsed,
            compact: panel_is_compact(panel_width(
                self.active_tab().script_preferred_width,
                (window.viewport_size().width - TAB_RAIL_WIDTH).max(px(0.)),
            )),
            on_expand_dock: on_marker_expand,
            pending_clear: marker_pending_clear,
            on_toggle_section: on_marker_toggle,
            on_jump: on_marker_jump,
            on_edit: on_marker_edit,
            on_delete: on_marker_remove,
            on_request_clear: on_marker_request_clear,
            on_confirm_clear: on_marker_confirm_clear,
            on_cancel_clear: on_marker_cancel_clear,
        };
        let marker_overlay = self
            .marker_editor
            .as_ref()
            .filter(|editor| editor.tab_id == active_id)
            .map(|editor| {
                let weak = cx.entity().downgrade();
                let on_pick_color =
                    Rc::new(move |color: MarkerColor, _: &mut Window, cx: &mut App| {
                        let _ = weak.update(cx, |this, cx| this.pick_marker_color(color, cx));
                    });
                let weak = cx.entity().downgrade();
                let on_confirm = Rc::new(move |_: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| this.confirm_marker(cx));
                });
                let weak = cx.entity().downgrade();
                let on_cancel = Rc::new(move |_: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| this.close_marker_editor(cx));
                });
                let weak = cx.entity().downgrade();
                let on_delete = Rc::new(move |_: &mut Window, cx: &mut App| {
                    let _ = weak.update(cx, |this, cx| this.delete_marker(cx));
                });
                MarkerEditor {
                    theme: self.theme,
                    locale,
                    position: editor.position,
                    color: editor.color,
                    existing: editor.existing,
                    input: editor.input.clone(),
                    error: editor.error.clone(),
                    on_pick_color,
                    on_confirm,
                    on_cancel,
                    on_delete,
                }
            });
        let script_items = tab.script_panel_items(locale);
        let main_width = (window.viewport_size().width - TAB_RAIL_WIDTH).max(px(0.));
        let script_panel_width = panel_width(tab.script_preferred_width, main_width);
        let weak = cx.entity().downgrade();
        let on_collapse_scripts = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == active_id) {
                    tab.script_preferred_width = SCRIPT_PANEL_MIN_WIDTH;
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_load_scripts = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.load_scripts(active_id, cx));
        });
        let weak = cx.entity().downgrade();
        let on_expand_scripts = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == active_id) {
                    tab.script_preferred_width = SCRIPT_PANEL_WIDTH;
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_open_scripts = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.script_preferred_width = SCRIPT_PANEL_WIDTH;
                    tab.scripts_collapsed = false;
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_open_quick = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.script_preferred_width = SCRIPT_PANEL_WIDTH;
                    tab.quick_collapsed = false;
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_run_script = Rc::new(move |path: PathBuf, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.run_script(active_id, path, cx));
        });
        let weak = cx.entity().downgrade();
        let on_stop_script = Rc::new(move |run_id: u64, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.stop_script(active_id, run_id, cx));
        });
        let weak = cx.entity().downgrade();
        let on_remove_script = Rc::new(move |path: PathBuf, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.remove_script(active_id, path, cx));
        });
        let weak = cx.entity().downgrade();
        let on_toggle_scripts_section = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.scripts_collapsed = !tab.scripts_collapsed;
                    cx.notify();
                }
            });
        });
        let scripts_collapsed = tab.scripts_collapsed;
        let quick_collapsed = tab.quick_collapsed;
        let quick_query = tab.quick_query.clone();
        let quick_collapsed_groups = tab.quick_collapsed_groups.clone();
        let quick_error = tab.quick_error.clone();
        let quick_pending_delete = tab.quick_pending_delete.clone();
        let script_panel = ScriptPanel {
            theme: surface_theme,
            locale,
            items: script_items,
            load_error: tab.script_load_error.clone(),
            compact: panel_is_compact(script_panel_width),
            collapsed: scripts_collapsed,
            max_height_limited: !quick_collapsed,
            on_load: on_load_scripts,
            on_collapse: on_collapse_scripts,
            on_expand: on_expand_scripts,
            on_open_section: on_open_scripts,
            on_toggle_section: on_toggle_scripts_section,
            on_run: on_run_script,
            on_stop: on_stop_script,
            on_remove: on_remove_script,
        };

        let quick_search_input = self.ensure_quick_search_input(active_id, window, cx);
        let search_keys = self
            .config_store
            .read(cx)
            .config()
            .shortcuts
            .display(ShortcutAction::Search);
        let (quick_commands, skipped_count, quick_send_mods) = {
            let store = self.config_store.read(cx);
            (
                store.config().quick_commands.clone(),
                store.skipped_quick_commands(),
                store
                    .config()
                    .shortcuts
                    .modifiers_for(ShortcutAction::QuickSend)
                    .map(|mods| mods.display())
                    .unwrap_or_default(),
            )
        };
        let quick_rows = visible_rows(&quick_commands, &quick_query, &quick_collapsed_groups);
        let skipped_notice =
            (skipped_count > 0).then(|| quick_skipped(locale, skipped_count).into());
        let weak = cx.entity().downgrade();
        let on_quick_send = Rc::new(move |id: String, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.quick_pending_delete = None;
                }
                this.send_quick_command(&id, cx);
            });
        });
        let weak = cx.entity().downgrade();
        let on_quick_toggle_group = Rc::new(move |group: String, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.quick_pending_delete = None;
                    if !tab.quick_collapsed_groups.remove(&group) {
                        tab.quick_collapsed_groups.insert(group);
                    }
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_quick_toggle_section = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.quick_collapsed = !tab.quick_collapsed;
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_quick_new = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.open_quick_editor(None, cx);
            });
        });
        let weak = cx.entity().downgrade();
        let on_quick_edit = Rc::new(move |id: String, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.open_quick_editor(Some(id), cx);
            });
        });
        let weak = cx.entity().downgrade();
        let on_quick_delete = Rc::new(move |id: String, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.quick_pending_delete = Some(id);
                    cx.notify();
                }
            });
        });
        let weak = cx.entity().downgrade();
        let on_quick_confirm_delete = Rc::new(move |id: String, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                this.delete_quick_command(active_id, id, cx);
            });
        });
        let weak = cx.entity().downgrade();
        let on_quick_cancel_delete = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(tab) = this.tab_mut(active_id) {
                    tab.quick_pending_delete = None;
                    cx.notify();
                }
            });
        });
        let quick_send_panel = QuickSendPanel {
            theme: surface_theme,
            locale,
            commands: quick_commands,
            rows: quick_rows,
            search_input: quick_search_input,
            error: quick_error,
            skipped_notice,
            collapsed: quick_collapsed,
            compact: panel_is_compact(script_panel_width),
            shortcut_mods: quick_send_mods,
            pending_delete: quick_pending_delete,
            on_send: on_quick_send,
            on_toggle_group: on_quick_toggle_group,
            on_toggle_section: on_quick_toggle_section,
            on_expand_dock: on_open_quick,
            on_new: on_quick_new,
            on_edit: on_quick_edit,
            on_delete: on_quick_delete,
            on_confirm_delete: on_quick_confirm_delete,
            on_cancel_delete: on_quick_cancel_delete,
        };

        let weak_for_pick = cx.entity().downgrade();
        let on_pick = Rc::new(
            move |kind: ActivePickerKind, window: &mut Window, cx: &mut App| {
                let _ = weak_for_pick.update(cx, |this, cx| {
                    this.open_picker(kind, window, cx);
                });
            },
        );

        let weak_for_conn = cx.entity().downgrade();
        let on_toggle_connection = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak_for_conn.update(cx, |this, cx| {
                this.toggle_connection(cx);
            });
        });

        let weak_for_line = cx.entity().downgrade();
        let on_toggle_line = Rc::new(move |line: ControlLine, _: &mut Window, cx: &mut App| {
            let _ = weak_for_line.update(cx, |this, cx| {
                this.toggle_modem_line(line, cx);
            });
        });
        let signal_controls = SignalControls {
            signals: self.active_tab().state.modem_signals,
            interactive: !is_simulated && is_connected,
            simulated: is_simulated,
            compact: (main_width - script_panel_width) < px(820.),
            on_toggle: on_toggle_line,
        };

        let weak_for_preset = cx.entity().downgrade();
        let on_select_preset = Rc::new(move |value: String, _: &mut Window, cx: &mut App| {
            let _ = weak_for_preset.update(cx, |this, cx| {
                this.apply_picker_value(&value, cx);
            });
        });

        let weak = cx.entity().downgrade();
        let on_rx_hex = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_receive_hex(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_timestamp = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_receive_timestamp(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_pause = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_receive_pause(cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_auto_scroll = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_receive_auto_scroll(cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_clear = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.clear_receive_logs(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_save = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.save_receive_logs(cx));
        });
        let stream_active = self
            .active_tab()
            .state
            .stream_output
            .as_ref()
            .is_some_and(|output| output.is_active());
        let stream_tooltip: SharedString = if self.active_tab().stream_busy {
            tr_shared(locale, "stream.busy")
        } else if let Some(output) = &self.active_tab().state.stream_output {
            format!("{}: {}", tr(locale, "stream.stop"), output.path().display()).into()
        } else {
            tr_shared(locale, "stream.start")
        };
        let stream_error = self.active_tab().stream_error.clone();
        let save_log_error = self.active_tab().save_log_error.clone();
        let weak = cx.entity().downgrade();
        let on_rx_stream = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_stream_output(cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_search = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_search(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_filter = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_log_filter(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_filter_control = Rc::new(move |control, window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.change_log_filter(control, window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_search_prev = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.jump_search_match(false, cx));
        });
        let weak = cx.entity().downgrade();
        let on_search_next = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.jump_search_match(true, cx));
        });
        let weak = cx.entity().downgrade();
        let on_search_close = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.close_search(window, cx));
        });

        let weak = cx.entity().downgrade();
        let on_tx_display_hex = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_send_display_hex(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_tx_collapse = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_send_collapsed(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_tx_send_hex = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_send_format_hex(cx));
        });
        let weak = cx.entity().downgrade();
        let on_tx_line_ending = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.cycle_send_line_ending(cx));
        });
        let weak = cx.entity().downgrade();
        let on_tx_submit = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.press_send_button(cx));
        });
        let weak = cx.entity().downgrade();
        let on_timer_toggle = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_timer_enabled(cx));
        });
        let timer_controls = TimerControls {
            state: timer_state,
            input: timer_input,
            enabled_flag: timer_flag,
            draft_pending: timer_draft_pending,
            on_toggle: on_timer_toggle,
        };

        v_flex()
            .id("sasha-root")
            .relative()
            .size_full()
            .bg(self.theme.bg_canvas)
            .text_color(self.theme.text_primary)
            .when_some(bg_layer, |this, layer| this.child(layer))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<ResizeScriptPanel>, window, cx| {
                    let tab_id = event.drag(cx).tab_id;
                    let main_width = window.viewport_size().width - TAB_RAIL_WIDTH;
                    let preferred = dragged_panel_width(event.event.position.x, main_width);
                    if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                        tab.script_preferred_width = preferred;
                        cx.notify();
                    }
                },
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.active_tab().state.active_picker.is_some() {
                        this.cancel_picker(cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &Copy, window, cx| {
                if this.editor_owns_copy(window, cx) {
                    return;
                }
                if !this.copy_log_selection(window, cx) {
                    cx.propagate();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this
                    .marker_editor
                    .as_ref()
                    .is_some_and(|editor| editor.tab_id == this.active_tab_id)
                    && event.keystroke.key.eq_ignore_ascii_case("escape")
                {
                    this.close_marker_editor(cx);
                    cx.stop_propagation();
                    return;
                }
                if this.handle_tab_shortcut(event, window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.handle_quick_shortcut(event, cx) {
                    cx.stop_propagation();
                    return;
                }

                let is_copy = event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.shift
                    && !event.keystroke.modifiers.alt
                    && event.keystroke.key.eq_ignore_ascii_case("c");
                if is_copy
                    && !this.editor_owns_copy(window, cx)
                    && this.copy_log_selection(window, cx)
                {
                    cx.stop_propagation();
                    return;
                }

                let is_search_shortcut = matches!(
                    this.shortcut_for(event, cx),
                    Some((ShortcutAction::Search, _))
                );
                if is_search_shortcut {
                    this.open_search(window, cx);
                    cx.stop_propagation();
                    return;
                }

                if event.keystroke.key.eq_ignore_ascii_case("f3")
                    && this.active_tab().log_search.open
                {
                    this.jump_search_match(!event.keystroke.modifiers.shift, cx);
                    cx.stop_propagation();
                    return;
                }

                if this.active_tab().state.active_picker.is_none() {
                    if this.active_tab().log_search.open {
                        match event.keystroke.key.as_str() {
                            "escape" => {
                                this.close_search(window, cx);
                                cx.stop_propagation();
                            }
                            "enter" if event.keystroke.modifiers.shift => {
                                let search_focused = this
                                    .active_tab()
                                    .search_input
                                    .as_ref()
                                    .is_some_and(|input| {
                                        input.read(cx).focus_handle(cx).is_focused(window)
                                    });
                                if search_focused {
                                    this.jump_search_match(false, cx);
                                    cx.stop_propagation();
                                }
                            }
                            _ => {}
                        }
                    }
                    return;
                }

                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.cancel_picker(cx);
                        cx.stop_propagation();
                    }
                    "enter" => {
                        let input_focused = this.picker_input.as_ref().is_some_and(|input| {
                            input.read(cx).focus_handle(cx).is_focused(window)
                        });
                        if !input_focused {
                            this.apply_picker_input(cx);
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            }))
            .child(
                h_flex()
                    .size_full()
                    .child(
                        v_flex()
                            .id("workspace-main")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(animate_frame_inset(
                                // Everything above the status bar. While filtering it sits on
                                // an accent fill whose top edge (the title bar band) carries the
                                // filter summary; the content on top has four rounded corners,
                                // so the fill shows through at each. The status bar stays outside
                                // the frame.
                                v_flex()
                                    .id("workspace-frame")
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .when(frame_mounted, |this| {
                                        this.bg(surface_theme.accent_blue)
                                            .rounded_tl(px(FILTER_FRAME_RADIUS))
                                            .pt(px(FILTER_FRAME_TOP))
                                            .px(px(FILTER_FRAME_SIDE))
                                            .pb(px(FILTER_FRAME_SIDE))
                                            .children(frame_edge)
                                            .child(FilterFrameLabel {
                                                theme: surface_theme,
                                                locale,
                                                visible: filter_visible,
                                                total: filter_total,
                                            })
                                    })
                                    .child(animate_content_corners(
                                        v_flex()
                                            .id("workspace-content")
                                            .flex_1()
                                            .min_h_0()
                                            .w_full()
                                            .when(frame_mounted, |this| {
                                                this.bg(surface_theme.bg_canvas)
                                                    .rounded(px(FILTER_FRAME_RADIUS))
                                            })
                                            .child(
                                                v_flex()
                                                    .id("receive-area")
                                                    .relative()
                                                    .flex_1()
                                                    .min_h_0()
                                                    .w_full()
                                                    .child(LogView::new(
                                                        self.theme,
                                                        locale,
                                                        logs,
                                                        receive_state.clone(),
                                                        simulated_notice.into(),
                                                        log_font_size,
                                                        log_focus,
                                                        log_list_state,
                                                        search_matches,
                                                        active_search_match,
                                                        markers,
                                                        on_mark,
                                                        on_jump,
                                                        on_disable_auto_scroll,
                                                    ))
                                                    .when_some(search_input, |this, input| {
                                                        this.child(
                                                            div()
                                                                .absolute()
                                                                .bottom(px(8.))
                                                                .right(px(20.))
                                                                .child(LogSearchBar::new(
                                                                    self.theme,
                                                                    locale,
                                                                    input,
                                                                    search_count_label,
                                                                    search_has_matches,
                                                                    on_search_prev,
                                                                    on_search_next,
                                                                    on_search_close,
                                                                )),
                                                        )
                                                    }),
                                            )
                                            .children(stream_error.map(|error| {
                                                div()
                                                    .px_4()
                                                    .text_xs()
                                                    .text_color(self.theme.text_primary)
                                                    .child(format!(
                                                        "{}: {}",
                                                        tr(locale, "stream.error"),
                                                        error
                                                    ))
                                            }))
                                            .children(save_log_error.map(|error| {
                                                div()
                                                    .px_4()
                                                    .text_xs()
                                                    .text_color(self.theme.text_primary)
                                                    .child(format!(
                                                        "{}: {}",
                                                        tr(locale, "stream.save_log_error"),
                                                        error
                                                    ))
                                            }))
                                            .child(
                                                ReceiveToolbar::new(
                                                    surface_theme,
                                                    locale,
                                                    receive_state,
                                                    send_state.send_hex,
                                                    send_state.line_ending,
                                                    search_open,
                                                    motion.rx_hex,
                                                    motion.rx_timestamp,
                                                    motion.rx_pause,
                                                    motion.rx_auto_scroll,
                                                    motion.tx_send_hex,
                                                    motion.tx_line_ending,
                                                    on_rx_hex,
                                                    on_rx_timestamp,
                                                    on_rx_pause,
                                                    on_rx_auto_scroll,
                                                    on_rx_clear,
                                                    on_rx_save,
                                                    on_rx_search,
                                                    on_tx_send_hex,
                                                    on_tx_line_ending,
                                                )
                                                .search_shortcut(search_keys.clone())
                                                .send_collapse(
                                                    send_collapsed,
                                                    on_tx_collapse.clone(),
                                                )
                                                .log_filter(
                                                    filter_open,
                                                    filter_active,
                                                    on_rx_filter,
                                                )
                                                .filter_panel(filter_input.map(|input| {
                                                    LogFilterBar {
                                                        theme: surface_theme,
                                                        locale,
                                                        input,
                                                        filter: filter_draft,
                                                        invalid: filter_invalid,
                                                        pending: filter_pending,
                                                        on_control: on_filter_control,
                                                    }
                                                }))
                                                .stream_output(
                                                    stream_active,
                                                    stream_tooltip,
                                                    on_rx_stream,
                                                ),
                                            )
                                            // The send card overlaps the strip's lower edge so the strip reads as
                                            // a drawer tucked underneath it.
                                            // When collapsed, the toolbar sits on top and the strip hangs below it.
                                            .child(
                                                div()
                                                    .w_full()
                                                    .flex_shrink_0()
                                                    .mt(px(if send_collapsed { 0. } else { -12. }))
                                                    // Lifts the card off the status bar's divider.
                                                    .mb(px(8.))
                                                    .child(
                                                        SendCard::new(
                                                            surface_theme,
                                                            locale,
                                                            send_state,
                                                            is_connected,
                                                            is_simulated,
                                                            motion.tx_display_hex,
                                                            motion.connection,
                                                            send_input,
                                                            send_error,
                                                            on_tx_display_hex,
                                                            on_tx_submit,
                                                        )
                                                        .timer(timer_controls)
                                                        .when(send_collapsed, |card| {
                                                            card.collapsed(on_tx_collapse)
                                                        }),
                                                    ),
                                            ),
                                        frame_transition,
                                    )),
                                frame_transition,
                            ))
                            .child(BottomBar::new(
                                self.theme,
                                locale,
                                config,
                                connection,
                                conn_action_label,
                                motion.connection,
                                counters,
                                bottom_buffer_label(
                                    locale,
                                    &format_buffer_usage(buffer_used as u64),
                                    &format_log_buffer_limit(log_buffer_limit),
                                ),
                                active_picker,
                                on_pick,
                                on_toggle_connection,
                                signal_controls,
                                {
                                    let weak = cx.entity().downgrade();
                                    Rc::new(move |window: &mut Window, cx: &mut App| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.open_throughput(window, cx)
                                        });
                                    })
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("script-panel-dock")
                            .relative()
                            .w(script_panel_width)
                            .h_full()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .child(
                                v_flex()
                                    .size_full()
                                    .bg(surface_theme.bg_canvas)
                                    .border_l_1()
                                    .border_color(self.theme.border_card)
                                    .child(script_panel)
                                    .child(quick_send_panel)
                                    .child(marker_panel),
                            )
                            .child(
                                resize_handle("script-resize", Axis::Horizontal)
                                    .inside(HandleEdge::Leading)
                                    .on_drag(
                                        ResizeScriptPanel { tab_id: active_id },
                                        |drag, _, _, cx| cx.new(|_| drag.clone()),
                                    ),
                            ),
                    )
                    .child(tab_rail),
            )
            .child(self.build_top_bar(script_panel_width))
            .when_some(marker_overlay, |this, overlay| this.child(overlay))
            .when_some(picker_overlay, |this, (kind, input)| {
                this.child(
                    div()
                        .id("parameter-picker-layer")
                        .absolute()
                        .top(px(40.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(ParameterPickerPopover::new(
                            self.theme,
                            locale,
                            kind,
                            is_simulated,
                            self.picker_port_presets.clone(),
                            motion.picker_open_epoch,
                            self.active_tab().state.config.clone(),
                            (kind == ActivePickerKind::BufferLimit)
                                .then(|| format_log_buffer_limit(log_buffer_limit)),
                            input,
                            self.picker_error.clone(),
                            on_select_preset,
                        )),
                )
            })
            .into_any_element()
    }
}
