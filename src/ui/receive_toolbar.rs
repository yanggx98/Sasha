use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::{ReceiveViewState, SendLineEnding};
use crate::ui::motion::{TOGGLE_TRANSITION_MS, TransitionFlag, animate_toggle_colors, lerp_color};
use crate::ui::theme::NanoTheme;

pub type ToolbarActionCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Top-right receive toolbar row (REQ-003, REQ-011, CR-002).
/// Displays the 5 tool slots (`H`, Timestamp, Pause, Clear, Save) as an
/// independent right-aligned row above the log view with smooth state-driven
/// highlighting on toggles (`H`, Timestamp, Pause), one-shot behavior on
/// Clear/Save, and hover tooltips on all controls.
#[derive(IntoElement)]
pub struct ReceiveToolbar {
    theme: NanoTheme,
    locale: Locale,
    state: ReceiveViewState,
    send_hex: bool,
    line_ending: SendLineEnding,
    rx_hex_flag: TransitionFlag,
    rx_timestamp_flag: TransitionFlag,
    rx_pause_flag: TransitionFlag,
    rx_auto_scroll_flag: TransitionFlag,
    tx_send_hex_flag: TransitionFlag,
    tx_line_ending_flag: TransitionFlag,
    on_toggle_hex: ToolbarActionCallback,
    on_toggle_timestamp: ToolbarActionCallback,
    on_toggle_pause: ToolbarActionCallback,
    on_toggle_auto_scroll: ToolbarActionCallback,
    on_clear: ToolbarActionCallback,
    on_save: ToolbarActionCallback,
    on_toggle_send_hex: ToolbarActionCallback,
    on_cycle_line_ending: ToolbarActionCallback,
}

impl ReceiveToolbar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        locale: Locale,
        state: ReceiveViewState,
        send_hex: bool,
        line_ending: SendLineEnding,
        rx_hex_flag: TransitionFlag,
        rx_timestamp_flag: TransitionFlag,
        rx_pause_flag: TransitionFlag,
        rx_auto_scroll_flag: TransitionFlag,
        tx_send_hex_flag: TransitionFlag,
        tx_line_ending_flag: TransitionFlag,
        on_toggle_hex: ToolbarActionCallback,
        on_toggle_timestamp: ToolbarActionCallback,
        on_toggle_pause: ToolbarActionCallback,
        on_toggle_auto_scroll: ToolbarActionCallback,
        on_clear: ToolbarActionCallback,
        on_save: ToolbarActionCallback,
        on_toggle_send_hex: ToolbarActionCallback,
        on_cycle_line_ending: ToolbarActionCallback,
    ) -> Self {
        Self {
            theme,
            locale,
            state,
            send_hex,
            line_ending,
            rx_hex_flag,
            rx_timestamp_flag,
            rx_pause_flag,
            rx_auto_scroll_flag,
            tx_send_hex_flag,
            tx_line_ending_flag,
            on_toggle_hex,
            on_toggle_timestamp,
            on_toggle_pause,
            on_toggle_auto_scroll,
            on_clear,
            on_save,
            on_toggle_send_hex,
            on_cycle_line_ending,
        }
    }

    fn tool_button(
        &self,
        id: &'static str,
        tooltip: SharedString,
        toggle_flag: Option<TransitionFlag>,
        on_click: ToolbarActionCallback,
        content: AnyElement,
    ) -> AnyElement {
        let base = div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .h(px(22.))
            .min_w(px(22.))
            .px_1()
            .rounded(px(6.))
            .hover(|s| {
                s.bg(self.theme.bg_hover)
                    .text_color(self.theme.text_primary)
            })
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_click(window, cx);
            })
            .child(content);

        match toggle_flag {
            Some(flag) => {
                let (fg_from, fg_to) =
                    flag.color_pair(self.theme.text_secondary, self.theme.accent_blue_hover);
                animate_toggle_colors(base, id, flag, TOGGLE_TRANSITION_MS, move |el, delta| {
                    el.text_color(lerp_color(fg_from, fg_to, delta))
                })
            }
            None => base
                .text_color(self.theme.text_secondary)
                .into_any_element(),
        }
    }

    fn text_tool_button(
        &self,
        id: &'static str,
        label: &'static str,
        tooltip: SharedString,
        toggle_flag: Option<TransitionFlag>,
        on_click: ToolbarActionCallback,
    ) -> AnyElement {
        self.tool_button(id, tooltip, toggle_flag, on_click, label.into_any_element())
    }

    fn icon_tool_button(
        &self,
        id: &'static str,
        icon: LucideIcon,
        tooltip: SharedString,
        toggle_flag: Option<TransitionFlag>,
        on_click: ToolbarActionCallback,
    ) -> AnyElement {
        self.tool_button(
            id,
            tooltip,
            toggle_flag,
            on_click,
            Icon::new(icon).size(px(14.)).into_any_element(),
        )
    }
}

impl RenderOnce for ReceiveToolbar {
    fn render(self, _window: &mut Window, _: &mut App) -> impl IntoElement {
        // Recessed "drawer" strip tucked underneath the send card: inset from the
        // card edges, darker surface, rounded top. The lower part is covered by
        // the send card (which overlaps it via a negative top margin).
        let strip = h_flex()
            .id("receive-toolbar")
            .w_full()
            .justify_start()
            .items_center()
            .px_2()
            .pt(px(4.))
            .pb(px(16.))
            .gap_1()
            .rounded_t(px(10.))
            .bg(self.theme.bg_strip)
            .border_1()
            .border_b_0()
            .border_color(self.theme.border_card);
        let locale = self.locale;
        div().w_full().flex_shrink_0().px_4().child(
            strip
                .child(self.text_tool_button(
                    "rx-tool-hex",
                    "H↓",
                    tr_shared(locale, "tooltip.rx_hex"),
                    Some(TransitionFlag {
                        active: self.state.hex_mode,
                        epoch: self.rx_hex_flag.epoch,
                    }),
                    self.on_toggle_hex.clone(),
                ))
                .child(self.icon_tool_button(
                    "rx-tool-timestamp",
                    LucideIcon::Clock,
                    tr_shared(locale, "tooltip.rx_timestamp"),
                    Some(TransitionFlag {
                        active: self.state.show_timestamp,
                        epoch: self.rx_timestamp_flag.epoch,
                    }),
                    self.on_toggle_timestamp.clone(),
                ))
                .child(div().w(px(1.)).h(px(14.)).mx_1().bg(self.theme.border_card))
                .child(self.icon_tool_button(
                    "rx-tool-pause",
                    LucideIcon::Pause,
                    tr_shared(locale, "tooltip.rx_pause"),
                    Some(TransitionFlag {
                        active: self.state.paused,
                        epoch: self.rx_pause_flag.epoch,
                    }),
                    self.on_toggle_pause.clone(),
                ))
                .child(self.icon_tool_button(
                    "rx-tool-auto-scroll",
                    LucideIcon::ArrowDownToLine,
                    tr_shared(locale, "tooltip.rx_autoscroll"),
                    Some(TransitionFlag {
                        active: self.state.auto_scroll,
                        epoch: self.rx_auto_scroll_flag.epoch,
                    }),
                    self.on_toggle_auto_scroll.clone(),
                ))
                .child(self.icon_tool_button(
                    "rx-tool-clear",
                    LucideIcon::Trash,
                    tr_shared(locale, "tooltip.rx_clear"),
                    None,
                    self.on_clear.clone(),
                ))
                .child(self.icon_tool_button(
                    "rx-tool-save",
                    LucideIcon::Download,
                    tr_shared(locale, "tooltip.rx_save"),
                    None,
                    self.on_save.clone(),
                ))
                .child(div().w(px(1.)).h(px(14.)).mx_1().bg(self.theme.border_card))
                .child(self.text_tool_button(
                    "tx-tool-send-hex",
                    "H↑",
                    tr_shared(locale, "tooltip.tx_hex_send"),
                    Some(TransitionFlag {
                        active: self.send_hex,
                        epoch: self.tx_send_hex_flag.epoch,
                    }),
                    self.on_toggle_send_hex.clone(),
                ))
                .child(self.text_tool_button(
                    "tx-tool-line-ending",
                    self.line_ending.label(),
                    match self.line_ending {
                        SendLineEnding::None => tr_shared(locale, "tooltip.line_ending_none"),
                        SendLineEnding::CrLf => tr_shared(locale, "tooltip.line_ending_crlf"),
                        SendLineEnding::Lf => tr_shared(locale, "tooltip.line_ending_lf"),
                    },
                    Some(TransitionFlag {
                        active: self.line_ending.is_active(),
                        epoch: self.tx_line_ending_flag.epoch,
                    }),
                    self.on_cycle_line_ending.clone(),
                )),
        )
    }
}
