use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::Locale;
use crate::model::{ActivePickerKind, SerialConfig, SessionCounters, SimulatedConnectionState};
use crate::ui::motion::{STATUS_TRANSITION_MS, TransitionFlag, animate_toggle_colors, lerp_color};
use crate::ui::theme::NanoTheme;

pub type PickParamCallback = Rc<dyn Fn(ActivePickerKind, &mut Window, &mut App)>;
pub type ToggleConnectionCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Bottom parameter & status bar (REQ-005, REQ-011).
/// Directly displays clickable port, baud rate, data bits, parity, stop bits,
/// connection action/status, and RX/TX byte counters.
#[derive(IntoElement)]
pub struct BottomBar {
    theme: NanoTheme,
    locale: Locale,
    config: SerialConfig,
    connection: SimulatedConnectionState,
    status_label: SharedString,
    action_label: SharedString,
    connection_flag: TransitionFlag,
    counters: SessionCounters,
    buffer_label: SharedString,
    active_picker: Option<ActivePickerKind>,
    on_pick: PickParamCallback,
    on_toggle_connection: ToggleConnectionCallback,
}

impl BottomBar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        locale: Locale,
        config: SerialConfig,
        connection: SimulatedConnectionState,
        status_label: impl Into<SharedString>,
        action_label: impl Into<SharedString>,
        connection_flag: TransitionFlag,
        counters: SessionCounters,
        buffer_label: impl Into<SharedString>,
        active_picker: Option<ActivePickerKind>,
        on_pick: PickParamCallback,
        on_toggle_connection: ToggleConnectionCallback,
    ) -> Self {
        Self {
            theme,
            locale,
            config,
            connection,
            status_label: status_label.into(),
            action_label: action_label.into(),
            connection_flag,
            counters,
            buffer_label: buffer_label.into(),
            active_picker,
            on_pick,
            on_toggle_connection,
        }
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
        let (badge_bg_from, badge_bg_to) =
            conn_flag.color_pair(self.theme.sim_badge_bg, self.theme.status_connected.alpha(0.12));
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

        let status_badge = animate_toggle_colors(
            div()
                .id("sim-connection-status")
                .px_1p5()
                .py_0p5()
                .rounded(px(4.))
                .text_xs()
                .child(self.status_label.clone()),
            "sim-connection-status-anim",
            conn_flag,
            STATUS_TRANSITION_MS,
            move |el, delta| {
                el.bg(lerp_color(badge_bg_from, badge_bg_to, delta))
                    .text_color(lerp_color(conn_from, conn_to, delta))
            },
        );

        h_flex()
            .id("bottom-status-bar")
            .flex_shrink_0()
            .w_full()
            .h(px(34.))
            .px_3()
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
                    .child(connection_btn)
                    .child(status_badge),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(self.theme.text_secondary)
                    .child(self.param_item(
                        "param-buffer",
                        ActivePickerKind::BufferLimit,
                        self.buffer_label.clone(),
                    ))
                    .child(format!("RX: {} B", self.counters.rx_bytes))
                    .child(format!("TX: {} B", self.counters.tx_bytes)),
            )
    }
}
