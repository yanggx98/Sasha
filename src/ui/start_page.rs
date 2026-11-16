use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::config::{BAUD_RATE_PRESETS, parse_baud_rate};
use crate::model::{DataBits, Parity, SerialConfig, StopBits};
use crate::serial::{DetectedPort, enumerate_serial_ports};
use crate::ui::theme::NanoTheme;

pub type StartActionCallback = Rc<dyn Fn(&mut Window, &mut App)>;
pub type SelectStringCallback = Rc<dyn Fn(String, &mut Window, &mut App)>;
pub type SelectBaudCallback = Rc<dyn Fn(u32, &mut Window, &mut App)>;
pub type SelectDataBitsCallback = Rc<dyn Fn(DataBits, &mut Window, &mut App)>;
pub type SelectParityCallback = Rc<dyn Fn(Parity, &mut Window, &mut App)>;
pub type SelectStopBitsCallback = Rc<dyn Fn(StopBits, &mut Window, &mut App)>;

/// Formats data bits, parity, and stop bits into a compact summary like `8-N-1`.
pub fn format_frame_summary(data_bits: DataBits, parity: Parity, stop_bits: StopBits) -> String {
    let parity_code = match parity {
        Parity::None => "N",
        Parity::Odd => "O",
        Parity::Even => "E",
    };
    format!(
        "{}-{}-{}",
        data_bits.label(),
        parity_code,
        stop_bits.label()
    )
}

/// Pure UI state for the startup serial connection page.
#[derive(Debug, Clone)]
pub struct StartPageState {
    pub ports: Vec<DetectedPort>,
    pub config: SerialConfig,
    pub manual_port_mode: bool,
    pub baud_dropdown_open: bool,
    pub frame_expanded: bool,
    pub baud_error: Option<String>,
    pub connect_error: Option<String>,
}

impl StartPageState {
    /// Creates initial state by enumerating host serial ports.
    pub fn new() -> Self {
        Self::new_with_ports(enumerate_serial_ports())
    }

    /// Creates state from an explicit port list (useful for deterministic unit tests).
    pub fn new_with_ports(ports: Vec<DetectedPort>) -> Self {
        let config = SerialConfig {
            port: ports.first().map(|p| p.name.clone()).unwrap_or_default(),
            ..Default::default()
        };

        Self {
            ports,
            config,
            manual_port_mode: false,
            baud_dropdown_open: false,
            frame_expanded: false,
            baud_error: None,
            connect_error: None,
        }
    }

    pub fn refresh_ports(&mut self, ports: Vec<DetectedPort>) {
        self.ports = ports;
        if !self.manual_port_mode {
            let current_exists = self.ports.iter().any(|p| p.name == self.config.port);
            if !current_exists {
                self.config.port = self
                    .ports
                    .first()
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
            }
        }
        self.connect_error = None;
    }

    pub fn select_detected_port(&mut self, port_name: String) {
        self.manual_port_mode = false;
        self.config.port = port_name;
        self.connect_error = None;
    }

    pub fn enable_manual_port(&mut self) {
        self.manual_port_mode = true;
        self.connect_error = None;
    }

    pub fn set_manual_port_value(&mut self, raw: &str) {
        self.config.port = raw.trim().to_string();
        self.connect_error = None;
    }

    pub fn select_baud_preset(&mut self, baud: u32) {
        self.config.baud_rate = baud;
        self.baud_error = None;
        self.baud_dropdown_open = false;
        self.connect_error = None;
    }

    pub fn apply_custom_baud(&mut self, raw: &str) -> Result<u32, &'static str> {
        match parse_baud_rate(raw) {
            Ok(val) => {
                self.config.baud_rate = val;
                self.baud_error = None;
                self.baud_dropdown_open = false;
                self.connect_error = None;
                Ok(val)
            }
            Err(err) => {
                self.baud_error = Some(err.to_string());
                Err(err)
            }
        }
    }

    pub fn frame_summary(&self) -> String {
        format_frame_summary(
            self.config.data_bits,
            self.config.parity,
            self.config.stop_bits,
        )
    }
}

impl Default for StartPageState {
    fn default() -> Self {
        Self::new()
    }
}

/// Borderless startup guide page rendered directly on `theme.bg_canvas`.
#[derive(IntoElement)]
pub struct StartPage {
    theme: NanoTheme,
    state: StartPageState,
    manual_port_input: Entity<InputState>,
    baud_input: Entity<InputState>,
    on_refresh_ports: StartActionCallback,
    on_select_port: SelectStringCallback,
    on_enable_manual_port: StartActionCallback,
    on_toggle_baud_dropdown: StartActionCallback,
    on_select_baud: SelectBaudCallback,
    on_toggle_frame_expanded: StartActionCallback,
    on_select_data_bits: SelectDataBitsCallback,
    on_select_parity: SelectParityCallback,
    on_select_stop_bits: SelectStopBitsCallback,
    on_connect: StartActionCallback,
    on_enter_simulated: StartActionCallback,
}

impl StartPage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        state: StartPageState,
        manual_port_input: Entity<InputState>,
        baud_input: Entity<InputState>,
        on_refresh_ports: StartActionCallback,
        on_select_port: SelectStringCallback,
        on_enable_manual_port: StartActionCallback,
        on_toggle_baud_dropdown: StartActionCallback,
        on_select_baud: SelectBaudCallback,
        on_toggle_frame_expanded: StartActionCallback,
        on_select_data_bits: SelectDataBitsCallback,
        on_select_parity: SelectParityCallback,
        on_select_stop_bits: SelectStopBitsCallback,
        on_connect: StartActionCallback,
        on_enter_simulated: StartActionCallback,
    ) -> Self {
        Self {
            theme,
            state,
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
        }
    }

    fn current_badge(theme: NanoTheme) -> AnyElement {
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .whitespace_nowrap()
            .text_xs()
            .text_color(theme.accent_blue_hover)
            .child(
                Icon::new(LucideIcon::Check)
                    .size(px(12.))
                    .text_color(theme.accent_blue_hover),
            )
            .child("当前")
            .into_any_element()
    }
}

impl RenderOnce for StartPage {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let state = self.state;

        let on_refresh = self.on_refresh_ports.clone();
        let on_enable_manual = self.on_enable_manual_port.clone();
        let on_toggle_baud = self.on_toggle_baud_dropdown.clone();
        let on_toggle_frame = self.on_toggle_frame_expanded.clone();
        let on_connect = self.on_connect.clone();
        let on_enter_sim = self.on_enter_simulated.clone();

        let port_section = v_flex()
            .w_full()
            .gap_1p5()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child("串口设备"),
                    )
                    .child(
                        h_flex()
                            .id("start-refresh-ports")
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_0p5()
                            .rounded(px(6.))
                            .text_xs()
                            .text_color(theme.text_secondary)
                            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_refresh(window, cx);
                            })
                            .child(Icon::new(LucideIcon::RefreshCw).size(px(12.)))
                            .child("刷新"),
                    ),
            )
            .child(
                v_flex()
                    .id("start-port-list")
                    .w_full()
                    .p_1()
                    .gap_0p5()
                    .rounded(px(8.))
                    .bg(theme.bg_card)
                    .border_1()
                    .border_color(theme.border_card)
                    .when(state.ports.is_empty(), |this| {
                        this.child(
                            div()
                                .w_full()
                                .px_2p5()
                                .py_2()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child("未检测到可用串口，请插入设备后点击刷新"),
                        )
                    })
                    .children(state.ports.iter().enumerate().map(|(idx, port)| {
                        let is_selected = !state.manual_port_mode && state.config.port == port.name;
                        let port_name = port.name.clone();
                        let on_select = self.on_select_port.clone();

                        h_flex()
                            .id(("start-port-item", idx))
                            .w_full()
                            .min_h(px(30.))
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_2p5()
                            .py_1()
                            .rounded(px(6.))
                            .when(is_selected, |this| this.bg(theme.bg_toggle_active))
                            .hover(|s| s.bg(theme.bg_hover))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_select(port_name.clone(), window, cx);
                            })
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if is_selected {
                                                theme.text_primary
                                            } else {
                                                theme.text_secondary
                                            })
                                            .child(port.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_xs()
                                            .text_color(theme.text_muted)
                                            .child(port.description.clone()),
                                    ),
                            )
                            .when(is_selected, |this| this.child(Self::current_badge(theme)))
                    }))
                    .child(
                        h_flex()
                            .id("start-port-manual-row")
                            .w_full()
                            .min_h(px(30.))
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_2p5()
                            .py_1()
                            .rounded(px(6.))
                            .when(state.manual_port_mode, |this| {
                                this.bg(theme.bg_toggle_active)
                            })
                            .hover(|s| s.bg(theme.bg_hover))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_enable_manual(window, cx);
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if state.manual_port_mode {
                                        theme.text_primary
                                    } else {
                                        theme.text_secondary
                                    })
                                    .child("手动输入端口..."),
                            )
                            .when(state.manual_port_mode, |this| {
                                this.child(Self::current_badge(theme))
                            }),
                    )
                    .when(state.manual_port_mode, |this| {
                        this.child(
                            div()
                                .id("start-manual-port-input-box")
                                .w_full()
                                .mt_1()
                                .rounded(px(6.))
                                .bg(theme.bg_canvas)
                                .border_1()
                                .border_color(theme.accent_blue)
                                .child(
                                    Input::new(&self.manual_port_input)
                                        .appearance(false)
                                        .cleanable(true),
                                ),
                        )
                    }),
            );

        let baud_section = v_flex()
            .w_full()
            .gap_1p5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child("波特率"),
            )
            .child(
                h_flex()
                    .id("start-baud-trigger")
                    .w_full()
                    .h(px(34.))
                    .px_3()
                    .items_center()
                    .justify_between()
                    .rounded(px(8.))
                    .bg(theme.bg_card)
                    .border_1()
                    .border_color(if state.baud_dropdown_open {
                        theme.accent_blue
                    } else {
                        theme.border_card
                    })
                    .hover(|s| s.bg(theme.bg_hover))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_toggle_baud(window, cx);
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_primary)
                            .child(state.config.baud_rate.to_string()),
                    )
                    .child(div().text_xs().text_color(theme.text_secondary).child("▾")),
            )
            .when(state.baud_dropdown_open, |this| {
                let baud_border = if state.baud_error.is_some() {
                    theme.error_text
                } else {
                    theme.accent_blue
                };
                this.child(
                    v_flex()
                        .id("start-baud-dropdown")
                        .w_full()
                        .p_2()
                        .gap_1()
                        .rounded(px(8.))
                        .bg(theme.bg_popover)
                        .border_1()
                        .border_color(theme.border_popover)
                        .child(
                            div()
                                .id("start-baud-input-box")
                                .w_full()
                                .rounded(px(6.))
                                .bg(theme.bg_canvas)
                                .border_1()
                                .border_color(baud_border)
                                .child(
                                    Input::new(&self.baud_input)
                                        .appearance(false)
                                        .cleanable(true),
                                ),
                        )
                        .when_some(state.baud_error.clone(), |this, err| {
                            this.child(
                                div()
                                    .w_full()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.))
                                    .bg(theme.error_bg)
                                    .text_xs()
                                    .text_color(theme.error_text)
                                    .child(err),
                            )
                        })
                        .child(v_flex().w_full().gap_0p5().children(
                            BAUD_RATE_PRESETS.iter().enumerate().map(|(idx, &preset)| {
                                let is_current = preset == state.config.baud_rate;
                                let on_select = self.on_select_baud.clone();

                                h_flex()
                                    .id(("start-baud-preset", idx))
                                    .w_full()
                                    .min_h(px(26.))
                                    .items_center()
                                    .justify_between()
                                    .px_2()
                                    .rounded(px(6.))
                                    .when(is_current, |this| this.bg(theme.bg_toggle_active))
                                    .hover(|s| s.bg(theme.bg_hover))
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        on_select(preset, window, cx);
                                    })
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if is_current {
                                                theme.text_primary
                                            } else {
                                                theme.text_secondary
                                            })
                                            .child(preset.to_string()),
                                    )
                                    .when(is_current, |this| this.child(Self::current_badge(theme)))
                            }),
                        )),
                )
            });

        let frame_chevron = if state.frame_expanded { "▾" } else { "▸" };
        let frame_section = v_flex()
            .w_full()
            .gap_1p5()
            .child(
                h_flex()
                    .id("start-frame-toggle")
                    .w_full()
                    .h(px(32.))
                    .px_2p5()
                    .items_center()
                    .justify_between()
                    .rounded(px(8.))
                    .bg(theme.bg_card)
                    .border_1()
                    .border_color(theme.border_card)
                    .hover(|s| s.bg(theme.bg_hover))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_toggle_frame(window, cx);
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_secondary)
                            .child(format!("{frame_chevron} 帧格式（高级）")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(state.frame_summary()),
                    ),
            )
            .when(state.frame_expanded, |this| {
                this.child(
                    v_flex()
                        .id("start-frame-options")
                        .w_full()
                        .p_2p5()
                        .gap_2()
                        .rounded(px(8.))
                        .bg(theme.bg_card)
                        .border_1()
                        .border_color(theme.border_card)
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.text_secondary)
                                        .child("数据位"),
                                )
                                .child(h_flex().items_center().gap_1().children(
                                    DataBits::ALL.iter().enumerate().map(|(idx, &bits)| {
                                        let selected = state.config.data_bits == bits;
                                        let on_select = self.on_select_data_bits.clone();
                                        div()
                                            .id(("start-data-bits", idx))
                                            .px_2p5()
                                            .py_1()
                                            .rounded(px(6.))
                                            .text_xs()
                                            .when(selected, |this| {
                                                this.bg(theme.bg_toggle_active)
                                                    .text_color(theme.accent_blue_hover)
                                            })
                                            .when(!selected, |this| {
                                                this.text_color(theme.text_secondary)
                                            })
                                            .hover(|s| {
                                                s.bg(theme.bg_hover).text_color(theme.text_primary)
                                            })
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                move |_, window, cx| {
                                                    cx.stop_propagation();
                                                    on_select(bits, window, cx);
                                                },
                                            )
                                            .child(bits.label())
                                    }),
                                )),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.text_secondary)
                                        .child("校验位"),
                                )
                                .child(h_flex().items_center().gap_1().children(
                                    Parity::ALL.iter().enumerate().map(|(idx, &parity)| {
                                        let selected = state.config.parity == parity;
                                        let on_select = self.on_select_parity.clone();
                                        div()
                                            .id(("start-parity", idx))
                                            .px_2p5()
                                            .py_1()
                                            .rounded(px(6.))
                                            .text_xs()
                                            .when(selected, |this| {
                                                this.bg(theme.bg_toggle_active)
                                                    .text_color(theme.accent_blue_hover)
                                            })
                                            .when(!selected, |this| {
                                                this.text_color(theme.text_secondary)
                                            })
                                            .hover(|s| {
                                                s.bg(theme.bg_hover).text_color(theme.text_primary)
                                            })
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                move |_, window, cx| {
                                                    cx.stop_propagation();
                                                    on_select(parity, window, cx);
                                                },
                                            )
                                            .child(parity.label())
                                    }),
                                )),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.text_secondary)
                                        .child("停止位"),
                                )
                                .child(h_flex().items_center().gap_1().children(
                                    StopBits::ALL.iter().enumerate().map(|(idx, &stop_bits)| {
                                        let selected = state.config.stop_bits == stop_bits;
                                        let on_select = self.on_select_stop_bits.clone();
                                        div()
                                            .id(("start-stop-bits", idx))
                                            .px_2p5()
                                            .py_1()
                                            .rounded(px(6.))
                                            .text_xs()
                                            .when(selected, |this| {
                                                this.bg(theme.bg_toggle_active)
                                                    .text_color(theme.accent_blue_hover)
                                            })
                                            .when(!selected, |this| {
                                                this.text_color(theme.text_secondary)
                                            })
                                            .hover(|s| {
                                                s.bg(theme.bg_hover).text_color(theme.text_primary)
                                            })
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                move |_, window, cx| {
                                                    cx.stop_propagation();
                                                    on_select(stop_bits, window, cx);
                                                },
                                            )
                                            .child(stop_bits.label())
                                    }),
                                )),
                        ),
                )
            });

        let actions_section = v_flex()
            .w_full()
            .items_center()
            .gap_2p5()
            .when_some(state.connect_error.clone(), |this, err| {
                this.child(
                    div()
                        .id("start-connect-error")
                        .w_full()
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
                        .bg(theme.error_bg)
                        .text_xs()
                        .text_color(theme.error_text)
                        .child(err),
                )
            })
            .child(
                div()
                    .id("start-connect-button")
                    .w_full()
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .bg(theme.accent_blue)
                    .hover(|s| s.bg(theme.accent_blue_hover))
                    .cursor_pointer()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(gpui_kit::white())
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_connect(window, cx);
                    })
                    .child("连接并进入终端 →"),
            )
            .child(
                div()
                    .id("start-enter-simulated")
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .text_xs()
                    .text_color(theme.text_secondary)
                    .hover(|s| s.text_color(theme.text_primary))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_enter_sim(window, cx);
                    })
                    .child("先不连接，进入模拟会话"),
            );

        div()
            .id("start-page-canvas")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px_4()
            .pt(px(38.))
            .pb_4()
            .child(
                v_flex()
                    .id("start-page-content")
                    .w(px(440.))
                    .max_w_full()
                    .gap(px(14.))
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_primary)
                                    .child("连接串口设备"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.text_secondary)
                                    .child("选择可用端口并确认通信参数"),
                            ),
                    )
                    .child(port_section)
                    .child(baud_section)
                    .child(frame_section)
                    .child(actions_section),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{StartPageState, format_frame_summary};
    use crate::model::{DataBits, Parity, StopBits};
    use crate::serial::DetectedPort;

    #[test]
    fn frame_summary_reflects_data_bits_parity_and_stop_bits() {
        assert_eq!(
            format_frame_summary(DataBits::Eight, Parity::None, StopBits::One),
            "8-N-1"
        );
        assert_eq!(
            format_frame_summary(DataBits::Seven, Parity::Even, StopBits::OnePointFive),
            "7-E-1.5"
        );
        assert_eq!(
            format_frame_summary(DataBits::Six, Parity::Odd, StopBits::Two),
            "6-O-2"
        );
        assert_eq!(
            format_frame_summary(DataBits::Five, Parity::None, StopBits::One),
            "5-N-1"
        );
    }

    #[test]
    fn start_page_state_initializes_from_detected_ports_and_handles_refresh() {
        let empty = StartPageState::new_with_ports(Vec::new());
        assert_eq!(empty.config.port, "");
        assert!(!empty.manual_port_mode);
        assert_eq!(empty.frame_summary(), "8-N-1");

        let ports = vec![
            DetectedPort {
                name: "COM3".to_string(),
                description: "USB-SERIAL CH340".to_string(),
            },
            DetectedPort {
                name: "COM5".to_string(),
                description: "CP2102".to_string(),
            },
        ];
        let mut state = StartPageState::new_with_ports(ports);
        assert_eq!(state.config.port, "COM3");

        state.select_detected_port("COM5".to_string());
        assert_eq!(state.config.port, "COM5");

        // Refresh keeping COM5
        state.refresh_ports(vec![
            DetectedPort {
                name: "COM5".to_string(),
                description: "CP2102".to_string(),
            },
            DetectedPort {
                name: "COM7".to_string(),
                description: "FTDI".to_string(),
            },
        ]);
        assert_eq!(state.config.port, "COM5");

        // Refresh when COM5 disappears falls back to first available
        state.refresh_ports(vec![DetectedPort {
            name: "COM7".to_string(),
            description: "FTDI".to_string(),
        }]);
        assert_eq!(state.config.port, "COM7");

        // Manual port mode preserves custom port across refresh
        state.enable_manual_port();
        state.set_manual_port_value("COM99");
        state.refresh_ports(Vec::new());
        assert!(state.manual_port_mode);
        assert_eq!(state.config.port, "COM99");
    }

    #[test]
    fn start_page_baud_validation_updates_config_or_sets_error() {
        let mut state = StartPageState::new_with_ports(Vec::new());
        state.baud_dropdown_open = true;

        assert!(state.apply_custom_baud("0").is_err());
        assert!(state.baud_error.is_some());
        assert!(state.baud_dropdown_open);
        assert_eq!(state.config.baud_rate, 115200);

        assert!(state.apply_custom_baud("abc").is_err());
        assert!(state.baud_error.is_some());

        assert_eq!(state.apply_custom_baud(" 230400 "), Ok(230400));
        assert_eq!(state.config.baud_rate, 230400);
        assert!(state.baud_error.is_none());
        assert!(!state.baud_dropdown_open);

        state.baud_dropdown_open = true;
        state.select_baud_preset(921600);
        assert_eq!(state.config.baud_rate, 921600);
        assert!(!state.baud_dropdown_open);
    }
}
