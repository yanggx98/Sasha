use std::rc::Rc;

use gpui_kit::base::TextSelection;
use gpui_kit::base::input::Copy;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::{
    ActivePickerKind, LogSelectionModel, SendPayloadError, SessionState, current_timestamp_hms_ms,
};
use crate::ui::bottom_bar::BottomBar;
use crate::ui::log_view::LogView;
use crate::ui::motion::UiMotionState;
use crate::ui::parameter_picker::ParameterPickerPopover;
use crate::ui::receive_toolbar::ReceiveToolbar;
use crate::ui::send_card::SendCard;
use crate::ui::theme::NanoTheme;
use crate::ui::title_bar::AppTitleBar;

/// Root single-session application view.
/// Owns `SessionState` as the shared state boundary for TASK-002 and TASK-003.
pub struct SashaApp {
    pub state: SessionState,
    pub theme: NanoTheme,
    motion: UiMotionState,
    picker_input: Option<Entity<InputState>>,
    _picker_subscription: Option<Subscription>,
    picker_error: Option<SharedString>,
    send_input: Option<Entity<TextareaState>>,
    _send_subscription: Option<Subscription>,
    send_error: Option<SharedString>,
    log_focus: FocusHandle,
    log_selection: LogSelectionModel,
}

impl SashaApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = SessionState::new_simulated_demo();
        let motion = UiMotionState::new(
            state.receive.hex_mode,
            state.receive.show_timestamp,
            state.receive.paused,
            state.send.hex_mode,
            state.connection.is_connected(),
        );
        let log_selection = LogSelectionModel::new(
            state.receive.show_timestamp,
            state.receive.hex_mode,
            state.logs.len(),
        );
        Self {
            state,
            theme: NanoTheme::default(),
            motion,
            picker_input: None,
            _picker_subscription: None,
            picker_error: None,
            send_input: None,
            _send_subscription: None,
            send_error: None,
            log_focus: cx.focus_handle(),
            log_selection,
        }
    }

    fn ensure_send_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        if let Some(existing) = &self.send_input {
            return existing.clone();
        }

        let initial_text = self.state.send.text.clone();
        let entity = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("输入发送内容（模拟）")
                .default_value(initial_text)
        });
        let entity_for_sub = entity.clone();
        let sub = cx.subscribe_in(
            &entity,
            window,
            move |this, _, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    this.state.send.text = entity_for_sub.read(cx).value().to_string();
                    if this.send_error.is_some() {
                        this.send_error = None;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter {
                    secondary: true, ..
                } => {
                    this.submit_send(cx);
                }
                _ => {}
            },
        );

        self.send_input = Some(entity.clone());
        self._send_subscription = Some(sub);
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
                InputEvent::Change => {
                    if this.picker_error.is_some() {
                        this.picker_error = None;
                        cx.notify();
                    }
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
        if self.state.active_picker == Some(kind) {
            self.cancel_picker(cx);
            return;
        }

        self.state.active_picker = Some(kind);
        self.motion.picker_open_epoch += 1;
        self.picker_error = None;

        let current_val = self.state.config.current_value_str(kind);
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
        let Some(kind) = self.state.active_picker else {
            return;
        };

        match self.state.config.apply_picker_value(kind, raw) {
            Ok(()) => {
                self.state.active_picker = None;
                self.picker_error = None;
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
        if self.state.active_picker.is_some() || self.picker_error.is_some() {
            self.state.active_picker = None;
            self.picker_error = None;
            cx.notify();
        }
    }

    pub fn toggle_simulated_connection(&mut self, cx: &mut Context<Self>) {
        self.state.toggle_simulated_connection();
        self.motion
            .connection
            .set(self.state.connection.is_connected());
        if self.state.connection.is_connected() {
            self.send_error = None;
        }
        cx.notify();
    }

    pub fn toggle_receive_hex(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.toggle_receive_hex();
        self.motion.rx_hex.set(self.state.receive.hex_mode);
        self.sync_log_selection(window, cx);
        cx.notify();
    }

    pub fn toggle_receive_timestamp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.toggle_receive_timestamp();
        self.motion
            .rx_timestamp
            .set(self.state.receive.show_timestamp);
        self.sync_log_selection(window, cx);
        cx.notify();
    }

    pub fn toggle_receive_pause(&mut self, cx: &mut Context<Self>) {
        self.state.toggle_receive_pause();
        self.motion.rx_pause.set(self.state.receive.paused);
        cx.notify();
    }

    pub fn clear_receive_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.clear_receive_logs();
        self.sync_log_selection(window, cx);
        cx.notify();
    }

    fn sync_log_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let invalidate = self.log_selection.sync_display(
            self.state.receive.show_timestamp,
            self.state.receive.hex_mode,
            self.state.logs.len(),
        );
        if invalidate {
            TextSelection::clear(window, cx);
        }
    }

    fn editor_owns_copy(&self, window: &Window, cx: &App) -> bool {
        let send_focused = self
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
        self.log_selection.set_selected_text(&selected);
        let Some(text) = self.log_selection.copy_payload() else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    pub fn save_receive_logs(&mut self, cx: &mut Context<Self>) {
        let content_to_save = self.state.visible_log_export_text();
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
        self.state
            .inject_next_deterministic_simulated_rx(current_timestamp_hms_ms());
        cx.notify();
    }

    pub fn toggle_send_hex(&mut self, cx: &mut Context<Self>) {
        self.state.toggle_send_hex();
        self.motion.tx_hex.set(self.state.send.hex_mode);
        self.send_error = None;
        cx.notify();
    }

    pub fn submit_send(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = &self.send_input {
            self.state.send.text = input.read(cx).value().to_string();
        }

        match self
            .state
            .submit_send_with_timestamp(current_timestamp_hms_ms())
        {
            Ok(_) => {
                self.send_error = None;
            }
            Err(SendPayloadError::Disconnected) => {
                self.send_error = Some("模拟未连接：请先点击底栏“模拟连接”".into());
            }
            Err(SendPayloadError::EmptyInput) => {
                self.send_error = Some("发送内容为空，未提交".into());
            }
            Err(SendPayloadError::InvalidHex(msg)) => {
                self.send_error = Some(msg.into());
            }
        }
        cx.notify();
    }
}

impl Render for SashaApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.log_selection.observe_log_len(self.state.logs.len());
        let send_input = self.ensure_send_input(window, cx);
        let log_focus = self.log_focus.clone();

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
                this.toggle_simulated_connection(cx);
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
        let on_rx_clear = Rc::new(move |window: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.clear_receive_logs(window, cx));
        });
        let weak = cx.entity().downgrade();
        let on_rx_save = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.save_receive_logs(cx));
        });

        let weak = cx.entity().downgrade();
        let on_tx_hex = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.toggle_send_hex(cx));
        });
        let weak = cx.entity().downgrade();
        let on_tx_submit = Rc::new(move |_: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |this, cx| this.submit_send(cx));
        });

        let picker_overlay = self.state.active_picker.zip(self.picker_input.clone());

        v_flex()
            .id("sasha-root")
            .relative()
            .size_full()
            .bg(self.theme.bg_canvas)
            .text_color(self.theme.text_primary)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.state.active_picker.is_some() {
                        this.cancel_picker(cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &Copy, window, cx| {
                if this.editor_owns_copy(window, cx) {
                    // The focused field already saw Copy. Swallow it here so the
                    // window root cannot replace that clipboard with the log.
                    return;
                }
                if !this.copy_log_selection(window, cx) {
                    cx.propagate();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Deterministic simulated RX event injection path for verification (F9 or Ctrl+Shift+R).
                let is_sim_rx_shortcut = event.keystroke.key.eq_ignore_ascii_case("f9")
                    || (event.keystroke.modifiers.control
                        && event.keystroke.modifiers.shift
                        && event.keystroke.key.eq_ignore_ascii_case("r"));
                if is_sim_rx_shortcut {
                    this.inject_simulated_rx_event(cx);
                    cx.stop_propagation();
                    return;
                }

                // Fallback when the Copy action is not dispatched. The action
                // handler above runs first and stops the key when it copies.
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

                if this.state.active_picker.is_none() {
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
            .child(AppTitleBar::new(self.theme, self.state.is_simulated_session))
            .child(
                v_flex()
                    .id("receive-area")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(ReceiveToolbar::new(
                        self.theme,
                        self.state.receive.clone(),
                        self.motion.rx_hex,
                        self.motion.rx_timestamp,
                        self.motion.rx_pause,
                        on_rx_hex,
                        on_rx_timestamp,
                        on_rx_pause,
                        on_rx_clear,
                        on_rx_save,
                    ))
                    .child(LogView::new(
                        self.theme,
                        self.state.logs.clone(),
                        self.state.receive.clone(),
                        self.state.simulated_notice,
                        log_focus,
                    )),
            )
            .child(SendCard::new(
                self.theme,
                self.state.send.clone(),
                self.state.connection.is_connected(),
                self.motion.tx_hex,
                self.motion.connection,
                send_input,
                self.send_error.clone(),
                on_tx_hex,
                on_tx_submit,
            ))
            .child(BottomBar::new(
                self.theme,
                self.state.config.clone(),
                self.state.connection,
                self.motion.connection,
                self.state.counters,
                self.state.active_picker,
                on_pick,
                on_toggle_connection,
            ))
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
                            self.motion.picker_open_epoch,
                            self.state.config.clone(),
                            input,
                            self.picker_error.clone(),
                            on_select_preset,
                        )),
                )
            })
    }
}
