use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr, tr_shared};
use crate::model::{
    ActivePickerKind, FlowControl, ModemSignals, SerialConfig, SessionCounters,
    SimulatedConnectionState,
};
use crate::session_service::ControlLine;
use crate::ui::motion::{STATUS_TRANSITION_MS, TransitionFlag, animate_toggle_colors, lerp_color};
use crate::ui::theme::NanoTheme;

pub type PickParamCallback = Rc<dyn Fn(ActivePickerKind, &mut Window, &mut App)>;
pub type ToggleConnectionCallback = Rc<dyn Fn(&mut Window, &mut App)>;
pub type ToggleLineCallback = Rc<dyn Fn(ControlLine, &mut Window, &mut App)>;
pub type OpenThroughputCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Modem signal controls rendered on the bottom bar's right side.
#[derive(Clone)]
pub struct SignalControls {
    /// Latest sampled CTS/DSR/CD/RI levels.
    pub signals: ModemSignals,
    /// `true` only for a connected real-hardware session.
    pub interactive: bool,
    /// `true` for the simulated demo session (controls are disabled).
    pub simulated: bool,
    /// Folds CTS/DSR/CD/RI into a single summary dot when width is tight.
    pub compact: bool,
    pub on_toggle: ToggleLineCallback,
}

/// Bottom parameter & status bar (REQ-005, REQ-011).
/// Directly displays clickable port, baud rate, data bits, parity, stop bits,
/// flow control, connection action, modem signal controls, and RX/TX byte counters.
#[derive(IntoElement)]
pub struct BottomBar {
    theme: NanoTheme,
    locale: Locale,
    config: SerialConfig,
    connection: SimulatedConnectionState,
    action_label: SharedString,
    connection_flag: TransitionFlag,
    counters: SessionCounters,
    buffer_label: SharedString,
    active_picker: Option<ActivePickerKind>,
    on_pick: PickParamCallback,
    on_toggle_connection: ToggleConnectionCallback,
    signal_controls: SignalControls,
    on_open_throughput: OpenThroughputCallback,
}

impl BottomBar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        locale: Locale,
        config: SerialConfig,
        connection: SimulatedConnectionState,
        action_label: impl Into<SharedString>,
        connection_flag: TransitionFlag,
        counters: SessionCounters,
        buffer_label: impl Into<SharedString>,
        active_picker: Option<ActivePickerKind>,
        on_pick: PickParamCallback,
        on_toggle_connection: ToggleConnectionCallback,
        signal_controls: SignalControls,
        on_open_throughput: OpenThroughputCallback,
    ) -> Self {
        Self {
            theme,
            locale,
            config,
            connection,
            action_label: action_label.into(),
            connection_flag,
            counters,
            buffer_label: buffer_label.into(),
            active_picker,
            on_pick,
            on_toggle_connection,
            signal_controls,
            on_open_throughput,
        }
    }

    /// DTR/RTS toggle chip. RTS is locked while RTS/CTS flow control owns the line.
    fn line_toggle(&self, id: &'static str, line: ControlLine) -> AnyElement {
        let theme = self.theme;
        let (label, level) = match line {
            ControlLine::Dtr => ("DTR", self.config.dtr),
            ControlLine::Rts => ("RTS", self.config.rts),
        };
        let rts_locked =
            line == ControlLine::Rts && self.config.flow_control == FlowControl::Hardware;
        let enabled = self.signal_controls.interactive && !rts_locked;
        let on_toggle = self.signal_controls.on_toggle.clone();
        let tooltip = if rts_locked {
            Some(tr_shared(self.locale, "signal.rts_locked"))
        } else if self.signal_controls.simulated {
            Some(tr_shared(self.locale, "signal.sim_disabled"))
        } else {
            None
        };

        div()
            .id(id)
            .px_1p5()
            .py_0p5()
            .rounded(px(4.))
            .text_xs()
            .when(level && enabled, |this| {
                this.bg(theme.bg_toggle_active)
                    .text_color(theme.accent_blue_hover)
            })
            .when(!level && enabled, |this| {
                this.text_color(theme.text_secondary)
            })
            .when(!enabled, |this| {
                this.text_color(theme.text_muted).opacity(0.5)
            })
            .when(enabled, |this| {
                this.hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_toggle(line, window, cx);
                    })
            })
            .when_some(tooltip, |this, text| {
                this.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
            })
            .child(label)
            .into_any_element()
    }

    /// Read-only indicator dot for one modem input line (`None` = unknown).
    fn signal_light(
        &self,
        id: &'static str,
        label: &'static str,
        level: Option<bool>,
    ) -> AnyElement {
        let theme = self.theme;
        let active = self.signal_controls.interactive;
        let color = match (active, level) {
            (true, Some(true)) => theme.status_connected,
            _ => theme.text_muted,
        };
        let dim = !active || level.is_none();
        let unknown = tr_shared(self.locale, "signal.unknown");
        h_flex()
            .id(id)
            .items_center()
            .gap_1()
            .when(dim, |this| this.opacity(0.5))
            .when(active && level.is_none(), |this| {
                this.tooltip(move |window, cx| Tooltip::new(unknown.clone()).build(window, cx))
            })
            .child(div().size(px(7.)).rounded_full().bg(color))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_secondary)
                    .child(label),
            )
            .into_any_element()
    }

    /// Compact summary dot for CTS/DSR/CD/RI used on narrow windows.
    fn signal_summary(&self) -> AnyElement {
        let theme = self.theme;
        let locale = self.locale;
        let signals = self.signal_controls.signals;
        let active = self.signal_controls.interactive;
        let any_high = [signals.cts, signals.dsr, signals.cd, signals.ri]
            .into_iter()
            .any(|v| v == Some(true));
        let all_unknown = [signals.cts, signals.dsr, signals.cd, signals.ri]
            .into_iter()
            .all(|v| v.is_none());
        let color = if active && any_high {
            theme.status_connected
        } else {
            theme.text_muted
        };
        let level_str = |v: Option<bool>| match v {
            Some(true) => tr(locale, "signal.high"),
            Some(false) => tr(locale, "signal.low"),
            None => tr(locale, "signal.unknown"),
        };
        let summary: SharedString = format!(
            "CTS: {} · DSR: {} · CD: {} · RI: {}",
            level_str(signals.cts),
            level_str(signals.dsr),
            level_str(signals.cd),
            level_str(signals.ri),
        )
        .into();

        h_flex()
            .id("signal-summary")
            .items_center()
            .gap_1()
            .when(!active || all_unknown, |this| this.opacity(0.5))
            .tooltip(move |window, cx| Tooltip::new(summary.clone()).build(window, cx))
            .child(div().size(px(7.)).rounded_full().bg(color))
            .into_any_element()
    }

    fn param_item(
        &self,
        id: &'static str,
        kind: ActivePickerKind,
        label: impl Into<SharedString>,
    ) -> AnyElement {
        let is_active = self.active_picker == Some(kind);
        let on_pick = self.on_pick.clone();
        let theme = self.theme;

        div()
            .id(id)
            .px_1p5()
            .py_0p5()
            .rounded(px(4.))
            .text_xs()
            .when(is_active, |this| {
                this.bg(theme.bg_toggle_active)
                    .text_color(theme.accent_blue_hover)
            })
            .when(!is_active, |this| this.text_color(theme.text_secondary))
            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_pick(kind, window, cx);
            })
            .child(label.into())
            .into_any_element()
    }
}

impl RenderOnce for BottomBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let conn_flag = TransitionFlag {
            active: self.connection.is_connected(),
            epoch: self.connection_flag.epoch,
        };
        let (conn_from, conn_to) =
            conn_flag.color_pair(self.theme.status_disconnected, self.theme.status_connected);
        let on_toggle = self.on_toggle_connection.clone();

        let connection_btn = animate_toggle_colors(
            h_flex()
                .id("param-connection")
                .items_center()
                .gap_1()
                .px_1p5()
                .py_0p5()
                .rounded(px(4.))
                .hover(|s| s.bg(self.theme.bg_hover))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_toggle(window, cx);
                })
                .child(Icon::new(LucideIcon::Link2).size(px(13.)))
                .child(div().text_xs().child(self.action_label.clone())),
            "param-connection-anim",
            conn_flag,
            STATUS_TRANSITION_MS,
            move |el, delta| el.text_color(lerp_color(conn_from, conn_to, delta)),
        );

        h_flex()
            .id("bottom-status-bar")
            .flex_shrink_0()
            .w_full()
            .h(px(34.))
            .px_3()
            .border_t_1()
            .border_color(self.theme.border_card)
            .items_center()
            .justify_between()
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(self.param_item(
                        "param-port",
                        ActivePickerKind::Port,
                        self.config.port.clone(),
                    ))
                    .child(self.param_item(
                        "param-baud",
                        ActivePickerKind::BaudRate,
                        self.config.baud_rate.to_string(),
                    ))
                    .child(self.param_item(
                        "param-data-bits",
                        ActivePickerKind::DataBits,
                        self.config.data_bits.label(),
                    ))
                    .child(self.param_item(
                        "param-parity",
                        ActivePickerKind::Parity,
                        self.config.parity.label_for(self.locale),
                    ))
                    .child(self.param_item(
                        "param-stop-bits",
                        ActivePickerKind::StopBits,
                        self.config.stop_bits.label(),
                    ))
                    .child(self.param_item(
                        "param-flow",
                        ActivePickerKind::FlowControl,
                        self.config.flow_control.label_for(self.locale),
                    ))
                    .child(connection_btn),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(self.theme.text_secondary)
                    .child(
                        h_flex()
                            .id("signal-controls")
                            .items_center()
                            .gap_1p5()
                            .child(self.line_toggle("signal-dtr", ControlLine::Dtr))
                            .child(self.line_toggle("signal-rts", ControlLine::Rts))
                            .when(self.signal_controls.compact, |this| {
                                this.child(self.signal_summary())
                            })
                            .when(!self.signal_controls.compact, |this| {
                                this.child(self.signal_light(
                                    "signal-cts",
                                    "CTS",
                                    self.signal_controls.signals.cts,
                                ))
                                .child(self.signal_light(
                                    "signal-dsr",
                                    "DSR",
                                    self.signal_controls.signals.dsr,
                                ))
                                .child(self.signal_light(
                                    "signal-cd",
                                    "CD",
                                    self.signal_controls.signals.cd,
                                ))
                                .child(self.signal_light(
                                    "signal-ri",
                                    "RI",
                                    self.signal_controls.signals.ri,
                                ))
                            }),
                    )
                    .child(self.param_item(
                        "param-buffer",
                        ActivePickerKind::BufferLimit,
                        self.buffer_label.clone(),
                    ))
                    .child({
                        let theme = self.theme;
                        let locale = self.locale;
                        let on_open_throughput = self.on_open_throughput.clone();
                        h_flex()
                            .id("bottom-bar-throughput")
                            .items_center()
                            .gap_2()
                            .px_1p5()
                            .py(px(2.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            .tooltip(move |window, cx| {
                                Tooltip::new(tr_shared(locale, "throughput.open_tooltip"))
                                    .build(window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_open_throughput(window, cx);
                            })
                            .child(
                                Icon::new(LucideIcon::Activity)
                                    .size(px(12.))
                                    .text_color(theme.text_muted),
                            )
                            .child(format!("RX: {} B", self.counters.rx_bytes))
                            .child(format!("TX: {} B", self.counters.tx_bytes))
                    }),
            )
    }
}
