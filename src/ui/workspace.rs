use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use gpui_kit::base::TextSelection;
use gpui_kit::base::input::Copy;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::{TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::log::format_hex_input_with_cursor;
use crate::model::{
    ActivePickerKind, DataBits, LogSelectionModel, Parity, SendPayloadError, SessionState,
    StopBits, current_timestamp_hms_ms,
};
use crate::serial::{SerialConnection, SerialEvent, enumerate_serial_ports};
use crate::ui::bottom_bar::BottomBar;
use crate::ui::log_view::LogView;
use crate::ui::motion::UiMotionState;
use crate::ui::parameter_picker::ParameterPickerPopover;
use crate::ui::receive_toolbar::ReceiveToolbar;
use crate::ui::send_card::SendCard;
use crate::ui::start_page::{StartPage, StartPageState};
use crate::ui::tab_rail::{TabItem, TabRail};
use crate::ui::theme::NanoTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPage {
    Start,
    Workspace,
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
    serial_rx_task: Option<Task<()>>,
    send_input: Option<Entity<TextareaState>>,
    _send_subscription: Option<Subscription>,
    send_error: Option<SharedString>,
    log_focus: FocusHandle,
    log_list_state: ListState,
    last_log_count: usize,
    last_tail_id: u64,
    last_tail_len: usize,
    last_display_flags: (bool, bool),
    log_selection: LogSelectionModel,
}

impl SessionTab {
    fn new(id: u64, cx: &mut Context<SashaApp>) -> Self {
        let state = SessionState::new_simulated_demo();
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
            start_page: StartPageState::new(),
            start_manual_port_input: None,
            _start_manual_port_sub: None,
            start_baud_input: None,
            _start_baud_sub: None,
            serial_conn: None,
            serial_rx_task: None,
            send_input: None,
            _send_subscription: None,
            send_error: None,
            log_focus: cx.focus_handle(),
            log_list_state: ListState::new(0, ListAlignment::Top, px(200.)),
            last_log_count: 0,
            last_tail_id: 0,
            last_tail_len: 0,
            last_display_flags: (true, false),
            log_selection,
        }
    }

    fn tab_item(&self, active_id: u64) -> TabItem {
        let title: SharedString = if self.page == AppPage::Start {
            "新连接".into()
        } else if self.state.is_simulated_session {
            "模拟演示".into()
        } else {
            self.state.config.port.clone().into()
        };
        TabItem {
            id: self.id,
            title,
            is_active: self.id == active_id,
            is_connected: self.page == AppPage::Workspace && self.state.connection.is_connected(),
            is_simulated: self.page == AppPage::Workspace && self.state.is_simulated_session,
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
        self.last_log_count = 0;
        self.last_tail_id = 0;
        self.last_tail_len = 0;
        self.log_list_state.reset(0);
        self.log_selection = LogSelectionModel::new(
            self.state.receive.show_timestamp,
            self.state.receive.hex_mode,
            self.state.logs.len(),
        );
    }

    fn sync_log_list_state(&mut self) {
        let log_count = self.state.logs.len();
        let notice_count = usize::from(!self.state.simulated_notice.is_empty());
        let total_items = log_count + notice_count;
        let tail_id = self.state.logs.last().map_or(0, |e| e.id);
        let tail_len = self.state.logs.last().map_or(0, |e| e.bytes.len());
        let display_flags = (
            self.state.receive.show_timestamp,
            self.state.receive.hex_mode,
        );
        let content_changed = log_count != self.last_log_count
            || tail_id != self.last_tail_id
            || tail_len != self.last_tail_len;

        if display_flags != self.last_display_flags
            || log_count < self.last_log_count
            || (log_count == self.last_log_count && tail_id != self.last_tail_id)
        {
            self.log_list_state.reset(total_items);
        } else if log_count > self.last_log_count {
            let start = self.last_log_count.saturating_sub(1);
            let old_end = self.last_log_count + notice_count;
            let new_count = total_items - start;
            self.log_list_state.splice(start..old_end, new_count);
        } else if tail_len != self.last_tail_len && log_count > 0 {
            let last_ix = log_count - 1;
            self.log_list_state.splice(last_ix..last_ix + 1, 1);
        } else if self.log_list_state.item_count() != total_items {
            self.log_list_state.reset(total_items);
        }

        if self.state.receive.auto_scroll && content_changed {
            self.log_list_state.scroll_to_end();
        }

        self.last_log_count = log_count;
        self.last_tail_id = tail_id;
        self.last_tail_len = tail_len;
        self.last_display_flags = display_flags;
    }

    fn disconnect_real_serial(&mut self, error: Option<String>) {
        self.serial_rx_task = None;
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

pub struct SashaApp {
    tabs: Vec<SessionTab>,
    active_tab_id: u64,
    next_tab_id: u64,
    theme: NanoTheme,
    picker_port_presets: Vec<String>,
    picker_input: Option<Entity<InputState>>,
    _picker_subscription: Option<Subscription>,
    picker_error: Option<SharedString>,
}

impl SashaApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let initial_tab = SessionTab::new(1, cx);
        Self {
            tabs: vec![initial_tab],
            active_tab_id: 1,
            next_tab_id: 2,
            theme: NanoTheme::default(),
            picker_port_presets: Vec::new(),
            picker_input: None,
            _picker_subscription: None,
            picker_error: None,
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

    fn tab_items(&self) -> Vec<TabItem> {
        self.tabs
            .iter()
            .map(|t| t.tab_item(self.active_tab_id))
            .collect()
    }

    fn dismiss_picker_state(&mut self) {
        self.picker_error = None;
        for tab in &mut self.tabs {
            tab.state.active_picker = None;
        }
    }

    fn select_tab(&mut self, tab_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_id == tab_id || !self.tabs.iter().any(|t| t.id == tab_id) {
            return;
        }
        self.dismiss_picker_state();
        TextSelection::clear(window, cx);
        self.active_tab_id = tab_id;
        cx.notify();
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_picker_state();
        TextSelection::clear(window, cx);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = SessionTab::new(id, cx);
        self.tabs.push(tab);
        self.active_tab_id = id;
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

    fn handle_tab_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mods = &event.keystroke.modifiers;
        if !mods.control || mods.alt || mods.platform {
            return false;
        }

        if event.keystroke.key.eq_ignore_ascii_case("t") && !mods.shift {
            self.new_tab(window, cx);
            return true;
        }
        if event.keystroke.key.eq_ignore_ascii_case("w") && !mods.shift {
            let active_id = self.active_tab_id;
            self.close_tab(active_id, window, cx);
            return true;
        }
        if event.keystroke.key.eq_ignore_ascii_case("tab") {
            let delta = if mods.shift { -1 } else { 1 };
            self.cycle_tab(delta, window, cx);
            return true;
        }
        if !mods.shift
            && let Some(digit) = event.keystroke.key.parse::<usize>().ok()
            && (1..=9).contains(&digit)
        {
            let target_idx = if digit == 9 {
                self.tabs.len().saturating_sub(1)
            } else {
                digit - 1
            };
            if let Some(tab) = self.tabs.get(target_idx) {
                let id = tab.id;
                self.select_tab(id, window, cx);
            }
            return true;
        }
        false
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

        let entity = cx.new(|cx| InputState::new(window, cx).placeholder("输入端口名（如 COM3）"));
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

        let entity = cx.new(|cx| {
            InputState::new(window, cx).placeholder("输入正整数波特率（如 230400），回车确认")
        });
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
                    if let Some(tab) = this.tab_mut(tab_id) {
                        let _ = tab.start_page.apply_custom_baud(&raw);
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
        self.active_tab_mut()
            .start_page
            .refresh_ports(enumerate_serial_ports());
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

    fn toggle_start_frame_expanded(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.start_page.frame_expanded = !tab.start_page.frame_expanded;
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

        let config = self.active_tab().start_page.config.clone();
        match SerialConnection::open(&config) {
            Ok((conn, rx)) => {
                let tab = self.active_tab_mut();
                tab.disconnect_real_serial(None);
                tab.state = SessionState::new_real_session(config, true);
                tab.sync_motion_and_selection_from_state();
                tab.send_input = None;
                tab._send_subscription = None;
                tab.send_error = None;
                tab.start_page.connect_error = None;
                tab.serial_conn = Some(conn);
                tab.page = AppPage::Workspace;
                self.start_serial_rx_loop(tab_id, rx, cx);
                cx.notify();
            }
            Err(err) => {
                self.active_tab_mut().start_page.connect_error = Some(err);
                cx.notify();
            }
        }
    }

    fn enter_simulated_from_start_page(&mut self, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        tab.disconnect_real_serial(None);
        tab.state = SessionState::new_simulated_demo();
        tab.sync_motion_and_selection_from_state();
        tab.send_input = None;
        tab._send_subscription = None;
        tab.send_error = None;
        tab.page = AppPage::Workspace;
        cx.notify();
    }

    fn start_serial_rx_loop(
        &mut self,
        tab_id: u64,
        rx: Receiver<SerialEvent>,
        cx: &mut Context<Self>,
    ) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;

                let mut rx_chunks: Vec<Vec<u8>> = Vec::new();
                let mut disconnected_err: Option<String> = None;
                let mut channel_closed = false;

                loop {
                    match rx.try_recv() {
                        Ok(SerialEvent::Rx(bytes)) => rx_chunks.push(bytes),
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

                let should_break = disconnected_err.is_some() || channel_closed;
                if !rx_chunks.is_empty() || should_break {
                    let updated = this.update(cx, |app, cx| {
                        let is_active = app.active_tab_id == tab_id;
                        let Some(tab) = app.tab_mut(tab_id) else {
                            return false;
                        };
                        if !rx_chunks.is_empty() {
                            let ts = current_timestamp_hms_ms();
                            for chunk in rx_chunks {
                                tab.state.append_rx_bytes(&chunk, &ts);
                            }
                            tab.log_selection.observe_log_len(tab.state.logs.len());
                        }
                        if let Some(err) = disconnected_err {
                            tab.disconnect_real_serial(Some(err));
                            cx.notify();
                        } else if channel_closed && tab.state.connection.is_connected() {
                            tab.disconnect_real_serial(Some("串口读取通道已关闭".to_string()));
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
        let config = {
            let tab = self.active_tab_mut();
            tab.disconnect_real_serial(None);
            tab.state.config.clone()
        };
        match SerialConnection::open(&config) {
            Ok((conn, rx)) => {
                let tab = self.active_tab_mut();
                tab.serial_conn = Some(conn);
                tab.state.set_connected(true);
                tab.motion.connection.set(true);
                tab.send_error = None;
                self.start_serial_rx_loop(tab_id, rx, cx);
            }
            Err(err) => {
                let tab = self.active_tab_mut();
                tab.state.set_connected(false);
                tab.motion.connection.set(false);
                tab.send_error = Some(err.into());
            }
        }
        cx.notify();
    }

    fn disconnect_real_serial(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.active_tab_mut().disconnect_real_serial(error);
        cx.notify();
    }

    fn handle_serial_disconnected(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.disconnect_real_serial(error, cx);
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
        let placeholder = if is_sim {
            "输入发送内容（模拟）"
        } else {
            "输入发送内容"
        };
        let entity = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(placeholder)
                .default_value(initial_text)
                .caret_style(px(4.), px(2.))
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

        let entity =
            cx.new(|cx| InputState::new(window, cx).placeholder("输入参数值或从下方选择预设"));
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

        let (is_sim, current_val) = {
            let tab = self.active_tab_mut();
            tab.state.active_picker = Some(kind);
            tab.motion.picker_open_epoch += 1;
            (
                tab.state.is_simulated_session,
                tab.state.config.current_value_str(kind),
            )
        };
        self.picker_error = None;
        if kind == ActivePickerKind::Port && !is_sim {
            self.picker_port_presets = enumerate_serial_ports()
                .into_iter()
                .map(|p| p.name)
                .collect();
        }

        let input = self.ensure_picker_input(window, cx);
        input.update(cx, |state, cx| {
            state.set_placeholder(kind.placeholder(), window, cx);
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

        let result = self
            .active_tab_mut()
            .state
            .config
            .apply_picker_value(kind, raw);
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
        tab.motion
            .rx_auto_scroll
            .set(tab.state.receive.auto_scroll);
        if tab.state.receive.auto_scroll {
            tab.log_list_state.scroll_to_end();
        }
        cx.notify();
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

    fn editor_owns_copy(&self, window: &Window, cx: &App) -> bool {
        let send_focused = self
            .active_tab()
            .send_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        let picker_focused = self
            .picker_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        send_focused || picker_focused
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
        let content_to_save = self.active_tab().state.visible_log_export_text();
        let suggested_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&suggested_dir, Some("sasha-log.txt"));

        cx.spawn(async move |_, _| {
            if let Ok(Ok(Some(path))) = rx.await {
                let _ = std::fs::write(&path, content_to_save.as_bytes());
            }
        })
        .detach();
    }

    pub fn inject_simulated_rx_event(&mut self, cx: &mut Context<Self>) {
        self.active_tab_mut()
            .state
            .inject_next_deterministic_simulated_rx(current_timestamp_hms_ms());
        cx.notify();
    }

    pub fn toggle_send_display_hex(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let send_input = self.active_tab().send_input.clone();
        if let Some(input) = &send_input {
            let value = input.read(cx).value().to_string();
            self.active_tab_mut().state.send.text = value;
        }

        let res = self.active_tab_mut().state.toggle_send_display_hex();
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

    pub fn submit_send(&mut self, cx: &mut Context<Self>) {
        let send_input = self.active_tab().send_input.clone();
        if let Some(input) = &send_input {
            let value = input.read(cx).value().to_string();
            self.active_tab_mut().state.send.text = value;
        }

        let is_simulated = self.active_tab().state.is_simulated_session;
        if is_simulated {
            let res = self
                .active_tab_mut()
                .state
                .submit_send_with_timestamp(current_timestamp_hms_ms());
            match res {
                Ok(_) => {
                    self.active_tab_mut().send_error = None;
                }
                Err(SendPayloadError::Disconnected) => {
                    self.active_tab_mut().send_error =
                        Some("模拟未连接：请先点击底栏“模拟连接”".into());
                }
                Err(SendPayloadError::EmptyInput) => {
                    self.active_tab_mut().send_error = Some("发送内容为空，未提交".into());
                }
                Err(SendPayloadError::InvalidHex(msg)) => {
                    self.active_tab_mut().send_error = Some(msg.into());
                }
            }
            cx.notify();
            return;
        }

        let prepared = self.active_tab().state.prepare_send_bytes();
        match prepared {
            Ok(bytes) => {
                let tab = self.active_tab_mut();
                if let Some(conn) = tab.serial_conn.as_mut() {
                    match conn.write_bytes(&bytes) {
                        Ok(()) => {
                            tab.state
                                .record_tx_bytes(&bytes, &current_timestamp_hms_ms());
                            tab.send_error = None;
                        }
                        Err(err) => {
                            tab.send_error = Some(err.into());
                        }
                    }
                } else {
                    self.handle_serial_disconnected(
                        Some("串口未连接：请先点击底栏“连接”".to_string()),
                        cx,
                    );
                    return;
                }
            }
            Err(SendPayloadError::Disconnected) => {
                self.active_tab_mut().send_error = Some("串口未连接：请先点击底栏“连接”".into());
            }
            Err(SendPayloadError::EmptyInput) => {
                self.active_tab_mut().send_error = Some("发送内容为空，未提交".into());
            }
            Err(SendPayloadError::InvalidHex(msg)) => {
                self.active_tab_mut().send_error = Some(msg.into());
            }
        }
        cx.notify();
    }

    fn build_tab_rail(&self, cx: &mut Context<Self>) -> TabRail {
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

        TabRail::new(
            self.theme,
            self.tab_items(),
            on_select_tab,
            on_close_tab,
            on_new_tab,
        )
    }

    fn build_top_bar(&self) -> impl IntoElement {
        div()
            .id("top-bar-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(34.))
            .occlude()
            .cursor_default()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                gpui_kit::base::GlobalState::suppress_text_selection(cx);
                // Windows needs the unhandled non-client mouse-down to start
                // native title-bar dragging. Occlusion already blocks content below.
            })
            .child(TitleBar::new().bg(gpui_kit::transparent_black()).border_b_0())
    }
}

impl Render for SashaApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_rail = self.build_tab_rail(cx);
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
            let on_toggle_frame_expanded = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.toggle_start_frame_expanded(cx));
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
            let on_connect = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.connect_from_start_page(cx));
            });
            let weak = cx.entity().downgrade();
            let on_enter_simulated = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = weak.update(cx, |this, cx| this.enter_simulated_from_start_page(cx));
            });

            return v_flex()
                .id("sasha-root")
                .relative()
                .size_full()
                .bg(self.theme.bg_canvas)
                .text_color(self.theme.text_primary)
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if this.handle_tab_shortcut(event, window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .child(
                    h_flex()
                        .size_full()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .child(StartPage::new(
                                    self.theme,
                                    start_page_state,
                                    manual_port_input,
                                    baud_input,
                                    on_refresh_ports,
                                    on_select_port,
                                    on_enable_manual_port,
                                    on_toggle_baud_dropdown,
                                    on_select_baud,
                                    on_toggle_frame_expanded,
                                    on_select_data_bits,
                                    on_select_parity,
                                    on_select_stop_bits,
                                    on_connect,
                                    on_enter_simulated,
                                )),
                        )
                        .child(tab_rail),
                )
                .child(self.build_top_bar())
                .into_any_element();
        }

        {
            let tab = self.active_tab_mut();
            let log_len = tab.state.logs.len();
            tab.log_selection.observe_log_len(log_len);
            tab.sync_log_list_state();
        }
        let send_input = self.ensure_send_input(active_id, window, cx);
        let tab = self.active_tab();
        let log_focus = tab.log_focus.clone();
        let log_list_state = tab.log_list_state.clone();
        let logs = tab.state.logs.clone();
        let receive_state = tab.state.receive.clone();
        let simulated_notice = tab.state.simulated_notice;
        let send_state = tab.state.send.clone();
        let motion = tab.motion;
        let is_connected = tab.state.connection.is_connected();
        let is_simulated = tab.state.is_simulated_session;
        let send_error = tab.send_error.clone();
        let config = tab.state.config.clone();
        let connection = tab.state.connection;
        let conn_status_label = tab.state.connection_status_label();
        let conn_action_label = tab.state.connection_action_label();
        let counters = tab.state.counters;
        let active_picker = tab.state.active_picker;
        let picker_overlay = active_picker.zip(self.picker_input.clone());

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

        let weak = cx.entity().downgrade();
        let on_tx_display_hex = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_send_display_hex(window, cx));
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
            let _ = weak.update(cx, |this, cx| this.submit_send(cx));
        });

        v_flex()
            .id("sasha-root")
            .relative()
            .size_full()
            .bg(self.theme.bg_canvas)
            .text_color(self.theme.text_primary)
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
                if this.handle_tab_shortcut(event, window, cx) {
                    cx.stop_propagation();
                    return;
                }

                // Deterministic simulated RX event injection path for verification (F9 or Ctrl+Shift+R).
                let is_sim_rx_shortcut = event.keystroke.key.eq_ignore_ascii_case("f9")
                    || (event.keystroke.modifiers.control
                        && event.keystroke.modifiers.shift
                        && event.keystroke.key.eq_ignore_ascii_case("r"));
                if is_sim_rx_shortcut && this.active_tab().state.is_simulated_session {
                    this.inject_simulated_rx_event(cx);
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

                if this.active_tab().state.active_picker.is_none() {
                    return;
                }

                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.cancel_picker(cx);
                        cx.stop_propagation();
                    }
                    "enter" => {
                        let input_focused = this
                            .picker_input
                            .as_ref()
                            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
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
                            .child(
                                v_flex()
                                    .id("receive-area")
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .child(LogView::new(
                                        self.theme,
                                        logs,
                                        receive_state.clone(),
                                        simulated_notice,
                                        log_focus,
                                        log_list_state,
                                    )),
                            )
                            .child(ReceiveToolbar::new(
                                self.theme,
                                receive_state,
                                send_state.send_hex,
                                send_state.line_ending,
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
                                on_tx_send_hex,
                                on_tx_line_ending,
                            ))
                            // The send card overlaps the strip's lower edge so the strip reads as
                            // a drawer tucked underneath it.
                            .child(
                                div()
                                    .w_full()
                                    .flex_shrink_0()
                                    .mt(px(-12.))
                                    .child(SendCard::new(
                                        self.theme,
                                        send_state,
                                        is_connected,
                                        is_simulated,
                                        motion.tx_display_hex,
                                        motion.connection,
                                        send_input,
                                        send_error,
                                        on_tx_display_hex,
                                        on_tx_submit,
                                    )),
                            )
                            .child(BottomBar::new(
                                self.theme,
                                config,
                                connection,
                                conn_status_label,
                                conn_action_label,
                                motion.connection,
                                counters,
                                active_picker,
                                on_pick,
                                on_toggle_connection,
                            )),
                    )
                    .child(tab_rail),
            )
            .child(self.build_top_bar())
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
                            kind,
                            is_simulated,
                            self.picker_port_presets.clone(),
                            motion.picker_open_epoch,
                            self.active_tab().state.config.clone(),
                            input,
                            self.picker_error.clone(),
                            on_select_preset,
                        )),
                )
            })
            .into_any_element()
    }
}
