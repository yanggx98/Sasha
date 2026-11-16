use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::*;

use crate::model::ReceiveViewState;
use crate::ui::motion::{
    TOGGLE_TRANSITION_MS, TransitionFlag, animate_toggle_colors, lerp_color, transparent_like,
};
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
    state: ReceiveViewState,
    rx_hex_flag: TransitionFlag,
    rx_timestamp_flag: TransitionFlag,
    rx_pause_flag: TransitionFlag,
    on_toggle_hex: ToolbarActionCallback,
    on_toggle_timestamp: ToolbarActionCallback,
    on_toggle_pause: ToolbarActionCallback,
    on_clear: ToolbarActionCallback,
    on_save: ToolbarActionCallback,
}

impl ReceiveToolbar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        state: ReceiveViewState,
        rx_hex_flag: TransitionFlag,
        rx_timestamp_flag: TransitionFlag,
        rx_pause_flag: TransitionFlag,
        on_toggle_hex: ToolbarActionCallback,
        on_toggle_timestamp: ToolbarActionCallback,
        on_toggle_pause: ToolbarActionCallback,
        on_clear: ToolbarActionCallback,
        on_save: ToolbarActionCallback,
    ) -> Self {
        Self {
            theme,
            state,
            rx_hex_flag,
            rx_timestamp_flag,
            rx_pause_flag,
            on_toggle_hex,
            on_toggle_timestamp,
            on_toggle_pause,
            on_clear,
            on_save,
        }
    }

    fn tool_button(
        &self,
        id: &'static str,
        tooltip: &'static str,
        toggle_flag: Option<TransitionFlag>,
        on_click: ToolbarActionCallback,
        content: AnyElement,
    ) -> AnyElement {
        let base = div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded_full()
            .hover(|s| s.bg(self.theme.bg_hover).text_color(self.theme.text_primary))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_click(window, cx);
            })
            .child(content);

        match toggle_flag {
            Some(flag) => {
                let (bg_from, bg_to) = flag.color_pair(
                    transparent_like(self.theme.bg_toggle_active),
                    self.theme.bg_toggle_active,
                );
                let (fg_from, fg_to) =
                    flag.color_pair(self.theme.text_secondary, self.theme.accent_blue_hover);
                animate_toggle_colors(base, id, flag, TOGGLE_TRANSITION_MS, move |el, delta| {
                    el.bg(lerp_color(bg_from, bg_to, delta))
                        .text_color(lerp_color(fg_from, fg_to, delta))
                })
            }
            None => base.text_color(self.theme.text_secondary).into_any_element(),
        }
    }

    fn text_tool_button(
        &self,
        id: &'static str,
        label: &'static str,
        tooltip: &'static str,
        toggle_flag: Option<TransitionFlag>,
        on_click: ToolbarActionCallback,
    ) -> AnyElement {
        self.tool_button(id, tooltip, toggle_flag, on_click, label.into_any_element())
    }

    fn icon_tool_button(
        &self,
        id: &'static str,
        icon: LucideIcon,
        tooltip: &'static str,
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        h_flex()
            .id("receive-toolbar")
            .w_full()
            .justify_end()
            .items_center()
            .px_3()
            .py(px(2.))
            .gap_1()
            .child(self.text_tool_button(
                "rx-tool-hex",
                "H",
                "HEX 显示切换",
                Some(TransitionFlag {
                    active: self.state.hex_mode,
                    epoch: self.rx_hex_flag.epoch,
                }),
                self.on_toggle_hex.clone(),
            ))
            .child(self.icon_tool_button(
                "rx-tool-timestamp",
                LucideIcon::Clock,
                "时间戳显示切换",
                Some(TransitionFlag {
                    active: self.state.show_timestamp,
                    epoch: self.rx_timestamp_flag.epoch,
                }),
                self.on_toggle_timestamp.clone(),
            ))
            .child(self.icon_tool_button(
                "rx-tool-pause",
                LucideIcon::Pause,
                "暂停/恢复接收显示",
                Some(TransitionFlag {
                    active: self.state.paused,
                    epoch: self.rx_pause_flag.epoch,
                }),
                self.on_toggle_pause.clone(),
            ))
            .child(self.icon_tool_button(
                "rx-tool-clear",
                LucideIcon::Trash,
                "清空当前显示日志",
                None,
                self.on_clear.clone(),
            ))
            .child(self.icon_tool_button(
                "rx-tool-save",
                LucideIcon::Download,
                "保存当前显示日志",
                None,
                self.on_save.clone(),
            ))
    }
}
