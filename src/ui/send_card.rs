use std::rc::Rc;

use std::time::Duration;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Size, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::SendDraftState;
use crate::model::timed_send::{IconAction, TimedSendState, TimerMode, icon_click_action};
use crate::ui::motion::{
    STATUS_TRANSITION_MS, TOGGLE_TRANSITION_MS, TransitionFlag, animate_toggle_colors, lerp_color,
    transparent_like,
};
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::{contrast_foreground, send_card_shadow};

pub type SendActionCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Timed-send capsule shown immediately left of the send button.
pub struct TimerControls {
    pub state: TimedSendState,
    pub input: Entity<InputState>,
    /// Enabled on/off transition used to animate the capsule colors.
    pub enabled_flag: TransitionFlag,
    /// The interval field holds an uncommitted draft (differs from the applied value).
    pub draft_pending: bool,
    /// Click on the icon segment: enable / cancel timed-send mode.
    pub on_toggle: SendActionCallback,
}

/// Bottom floating send card (REQ-004, REQ-011).
/// Exposes the top-right `H` display toggle (controls input HEX formatting only;
/// send encoding is controlled independently in the bottom bar) and bottom-right
/// send button, with no extra send settings or resize handles.
#[derive(IntoElement)]
pub struct SendCard {
    theme: NanoTheme,
    locale: Locale,
    draft: SendDraftState,
    connected: bool,
    is_simulated: bool,
    tx_display_hex_flag: TransitionFlag,
    connection_flag: TransitionFlag,
    input: Entity<TextareaState>,
    error: Option<SharedString>,
    on_toggle_display_hex: SendActionCallback,
    on_submit: SendActionCallback,
    timer: Option<TimerControls>,
}

impl SendCard {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        locale: Locale,
        draft: SendDraftState,
        connected: bool,
        is_simulated: bool,
        tx_display_hex_flag: TransitionFlag,
        connection_flag: TransitionFlag,
        input: Entity<TextareaState>,
        error: Option<SharedString>,
        on_toggle_display_hex: SendActionCallback,
        on_submit: SendActionCallback,
    ) -> Self {
        Self {
            theme,
            locale,
            draft,
            connected,
            is_simulated,
            tx_display_hex_flag,
            connection_flag,
            input,
            error,
            on_toggle_display_hex,
            on_submit,
            timer: None,
        }
    }

    pub fn timer(mut self, timer: TimerControls) -> Self {
        self.timer = Some(timer);
        self
    }
}

impl RenderOnce for SendCard {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let locale = self.locale;
        let (shadow_alpha, shadow_y, shadow_blur) = send_card_shadow();
        let on_toggle_display_hex = self.on_toggle_display_hex.clone();
        let on_submit = self.on_submit.clone();
        let hex_display_tooltip = tr_shared(locale, "tooltip.tx_hex_display");
        let timer_mode = self
            .timer
            .as_ref()
            .map_or(TimerMode::Off, |t| t.state.mode());
        let submit_tooltip = match (self.is_simulated, self.connected) {
            _ if timer_mode == TimerMode::Armed => tr_shared(locale, "send.timer_play_tooltip"),
            _ if timer_mode == TimerMode::Running => tr_shared(locale, "send.timer_pause_tooltip"),
            (true, true) => tr_shared(locale, "send.btn_sim"),
            (true, false) => tr_shared(locale, "send.btn_sim_disconnected"),
            (false, true) => tr_shared(locale, "send.btn"),
            (false, false) => tr_shared(locale, "send.btn_disconnected"),
        };

        let tx_display_hex_flag = TransitionFlag {
            active: self.draft.display_hex,
            epoch: self.tx_display_hex_flag.epoch,
        };
        let (hex_bg_from, hex_bg_to) = tx_display_hex_flag.color_pair(
            transparent_like(self.theme.bg_toggle_active),
            self.theme.bg_toggle_active,
        );
        let (hex_fg_from, hex_fg_to) =
            tx_display_hex_flag.color_pair(self.theme.text_secondary, self.theme.accent_blue_hover);

        let hex_toggle_btn = animate_toggle_colors(
            div()
                .id("send-card-hex-toggle")
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(24.))
                .rounded(px(6.))
                .hover(|s| {
                    s.bg(self.theme.bg_hover)
                        .text_color(self.theme.text_primary)
                })
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .tooltip(move |window, cx| {
                    Tooltip::new(hex_display_tooltip.clone()).build(window, cx)
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_toggle_display_hex(window, cx);
                })
                .child("H"),
            "send-card-hex-toggle",
            tx_display_hex_flag,
            TOGGLE_TRANSITION_MS,
            move |el, delta| {
                el.bg(lerp_color(hex_bg_from, hex_bg_to, delta))
                    .text_color(lerp_color(hex_fg_from, hex_fg_to, delta))
            },
        );

        let conn_flag = TransitionFlag {
            active: self.connected,
            epoch: self.connection_flag.epoch,
        };
        let (submit_bg_from, submit_bg_to) =
            conn_flag.color_pair(self.theme.bg_hover, self.theme.accent_blue);
        let (submit_fg_from, submit_fg_to) = conn_flag.color_pair(
            self.theme.text_muted,
            contrast_foreground(self.theme.accent_blue),
        );
        let submit_hover_bg = if self.connected {
            self.theme.accent_blue_hover
        } else {
            self.theme.bg_hover
        };

        // Normal send / play (armed) / pause (running).
        let submit_icon = match timer_mode {
            TimerMode::Off => LucideIcon::ArrowUp,
            TimerMode::Armed => LucideIcon::Play,
            TimerMode::Running => LucideIcon::Pause,
        };
        let submit_base = div()
            .id("send-card-submit")
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .w(px(36.))
            .h(px(28.))
            .rounded(px(8.))
            .tooltip(move |window, cx| Tooltip::new(submit_tooltip.clone()).build(window, cx))
            .child(Icon::new(submit_icon).size(px(15.)));
        let submit_btn = animate_toggle_colors(
            submit_base
                .hover(move |s| s.bg(submit_hover_bg))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_submit(window, cx);
                }),
            "send-card-submit",
            conn_flag,
            STATUS_TRANSITION_MS,
            move |el, delta| {
                el.bg(lerp_color(submit_bg_from, submit_bg_to, delta))
                    .text_color(lerp_color(submit_fg_from, submit_fg_to, delta))
            },
        );

        let timer_row = self
            .timer
            .map(|timer| build_timer_row(timer, self.theme, locale, self.connected));

        div().flex_shrink_0().w_full().px_3().pb_1().child(
            v_flex()
                .id("floating-send-card")
                .w_full()
                .min_h(px(96.))
                .justify_between()
                .px_3()
                .py_2()
                .gap_1p5()
                .rounded(px(12.))
                .bg(self.theme.bg_card)
                .shadow(vec![BoxShadow {
                    color: gpui_kit::black().opacity(shadow_alpha),
                    offset: point(px(0.), px(shadow_y)),
                    blur_radius: px(shadow_blur),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .border_1()
                .border_color(if self.error.is_some() {
                    self.theme.error_text
                } else {
                    self.theme.border_card
                })
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                // The card already supplies the content inset;
                                // cancel the textarea's extra leading padding.
                                .ml(-Size::default().input_px())
                                .mt(-Size::default().input_py())
                                .text_sm()
                                .text_color(self.theme.text_primary)
                                .child(
                                    Textarea::new(&self.input)
                                        .appearance(false)
                                        .bordered(false)
                                        .h(px(52.)),
                                ),
                        )
                        .child(hex_toggle_btn),
                )
                .when_some(self.error, |this, err| {
                    this.child(
                        div()
                            .id("send-card-error")
                            .w_full()
                            .px_2()
                            .py_1()
                            .rounded(px(6.))
                            .bg(self.theme.error_bg)
                            .text_xs()
                            .text_color(self.theme.error_text)
                            .child(err),
                    )
                })
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().min_w_0())
                        .children(timer_row)
                        .child(submit_btn),
                ),
        )
    }
}

/// Fixed segment widths (px) of the timed-send capsule `[icon | interval | ms]`.
const TIMER_ICON_W: f32 = 28.;
const TIMER_DIVIDER_W: f32 = 1.;
const TIMER_INPUT_W: f32 = 64.;
const TIMER_UNIT_W: f32 = 24.;
/// Total width including the 1px border on each side.
const TIMER_CAPSULE_W: f32 = TIMER_ICON_W + TIMER_DIVIDER_W + TIMER_INPUT_W + TIMER_UNIT_W + 2.;

/// Always-visible capsule `[clock | interval ms]`. Clicking the clock segment only
/// enables/cancels timed-send mode (the send button then plays/pauses); the
/// interval segment stays editable at all times.
fn build_timer_row(
    timer: TimerControls,
    theme: NanoTheme,
    locale: Locale,
    connected: bool,
) -> AnyElement {
    let TimerControls {
        state,
        input,
        enabled_flag,
        draft_pending,
        on_toggle,
    } = timer;

    let action = icon_click_action(state.enabled, connected);
    let clickable = action != IconAction::Ignore;
    let icon_tooltip = match action {
        IconAction::Enable => tr_shared(locale, "send.timer_enable_tooltip"),
        IconAction::Disable => tr_shared(locale, "send.timer_disable_tooltip"),
        IconAction::Ignore => tr_shared(locale, "send.timer_toggle_disconnected"),
    };

    // Disabled: grey outline on a transparent fill. Enabled: solid accent fill with a
    // contrast-chosen foreground. Both ends animate through `animate_toggle_colors`.
    let on_fg = contrast_foreground(theme.accent_blue);
    let off_fg = if clickable {
        theme.text_secondary
    } else {
        theme.text_muted
    };
    let flag = TransitionFlag {
        active: state.enabled,
        epoch: enabled_flag.epoch,
    };
    let (bg_from, bg_to) = flag.color_pair(transparent_like(theme.accent_blue), theme.accent_blue);
    let (border_from, border_to) = flag.color_pair(theme.border_card, theme.accent_blue);
    let (fg_from, fg_to) = flag.color_pair(off_fg, on_fg);
    let divider = if state.enabled {
        let mut c = on_fg;
        c.a = 0.35;
        c
    } else {
        theme.border_card
    };
    // Pending draft: a soft tint behind the interval segment until it is committed.
    let pending_bg = if draft_pending {
        let mut c = if state.enabled {
            on_fg
        } else {
            theme.accent_blue
        };
        c.a = if state.enabled { 0.22 } else { 0.18 };
        c
    } else {
        transparent_like(theme.accent_blue)
    };
    let icon_hover_bg = if state.enabled {
        theme.accent_blue_hover
    } else {
        theme.bg_hover
    };

    let icon_segment = div()
        .id("send-card-timer-toggle")
        .relative()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(TIMER_ICON_W))
        .h_full()
        .when(clickable, |el| {
            el.cursor_pointer().hover(move |s| s.bg(icon_hover_bg))
        })
        .tooltip(move |window, cx| Tooltip::new(icon_tooltip.clone()).build(window, cx))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            if clickable {
                on_toggle(window, cx);
            }
        })
        .child(Icon::new(LucideIcon::Clock).size(px(14.)))
        .when(state.running, |el| {
            el.child(
                div()
                    .absolute()
                    .top(px(4.))
                    .right(px(4.))
                    .size(px(5.))
                    .rounded_full()
                    .bg(on_fg)
                    .with_animation(
                        "send-card-timer-dot",
                        Animation::new(Duration::from_millis(1400))
                            .repeat()
                            .with_easing(|t| 0.5 - 0.5 * (std::f32::consts::TAU * t).cos()),
                        |dot, delta| dot.opacity(0.3 + 0.7 * delta),
                    ),
            )
        });

    // "ms" follows the capsule foreground (never the accent blue).
    let unit_color = if state.enabled { on_fg } else { off_fg };

    // The capsule is a block-level flex container, so without a fixed width it
    // would stretch to its parent; pin the width and forbid shrinking.
    let capsule = h_flex()
        .id("send-card-timer-capsule")
        .flex_shrink_0()
        .w(px(TIMER_CAPSULE_W))
        .items_center()
        .h(px(28.))
        .rounded(px(8.))
        .border_1()
        .overflow_hidden()
        .text_xs()
        .when(!clickable, |el| el.opacity(0.6))
        .child(icon_segment)
        .child(
            div()
                .w(px(TIMER_DIVIDER_W))
                .h(px(14.))
                .flex_shrink_0()
                .bg(divider),
        )
        .child(
            div()
                .w(px(TIMER_INPUT_W))
                .h_full()
                .flex()
                .items_center()
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .bg(pending_bg)
                .child(
                    Input::new(&input)
                        .appearance(false)
                        .disabled(!connected && !state.running),
                ),
        )
        .child(
            div()
                .w(px(TIMER_UNIT_W))
                .flex()
                .items_center()
                .pl_1()
                .flex_shrink_0()
                .text_color(unit_color)
                .child(tr_shared(locale, "send.timer_unit")),
        );

    animate_toggle_colors(
        capsule,
        "send-card-timer-capsule",
        flag,
        TOGGLE_TRANSITION_MS,
        move |el, delta| {
            el.bg(lerp_color(bg_from, bg_to, delta))
                .border_color(lerp_color(border_from, border_to, delta))
                .text_color(lerp_color(fg_from, fg_to, delta))
        },
    )
}

// Compact: leaves the send button room even in a narrow window.
const _: () = assert!(TIMER_CAPSULE_W <= 130.);

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn timer_capsule_stays_compact_and_input_segment_is_narrow() {
        assert_eq!(TIMER_INPUT_W, 64.);
        assert_eq!(
            TIMER_CAPSULE_W,
            TIMER_ICON_W + TIMER_DIVIDER_W + TIMER_INPUT_W + TIMER_UNIT_W + 2.
        );
    }
}
