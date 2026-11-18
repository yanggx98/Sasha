use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Size, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::SendDraftState;
use crate::ui::motion::{
    STATUS_TRANSITION_MS, TOGGLE_TRANSITION_MS, TransitionFlag, animate_toggle_colors, lerp_color,
    transparent_like,
};
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::{contrast_foreground, send_card_shadow};

pub type SendActionCallback = Rc<dyn Fn(&mut Window, &mut App)>;

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
        }
    }
}

impl RenderOnce for SendCard {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let locale = self.locale;
        let (shadow_alpha, shadow_y, shadow_blur) = send_card_shadow();
        let on_toggle_display_hex = self.on_toggle_display_hex.clone();
        let on_submit = self.on_submit.clone();
        let hex_display_tooltip = tr_shared(locale, "tooltip.tx_hex_display");
        let submit_tooltip = match (self.is_simulated, self.connected) {
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
        let (submit_fg_from, submit_fg_to) =
            conn_flag.color_pair(self.theme.text_muted, contrast_foreground(self.theme.accent_blue));
        let submit_hover_bg = if self.connected {
            self.theme.accent_blue_hover
        } else {
            self.theme.bg_hover
        };

        let submit_btn = animate_toggle_colors(
            div()
                .id("send-card-submit")
                .flex()
                .items_center()
                .justify_center()
                .w(px(36.))
                .h(px(28.))
                .rounded(px(8.))
                .hover(move |s| s.bg(submit_hover_bg))
                .cursor_pointer()
                .tooltip(move |window, cx| Tooltip::new(submit_tooltip.clone()).build(window, cx))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    on_submit(window, cx);
                })
                .child(Icon::new(LucideIcon::ArrowUp).size(px(15.))),
            "send-card-submit",
            conn_flag,
            STATUS_TRANSITION_MS,
            move |el, delta| {
                el.bg(lerp_color(submit_bg_from, submit_bg_to, delta))
                    .text_color(lerp_color(submit_fg_from, submit_fg_to, delta))
            },
        );

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
                .child(h_flex().w_full().justify_end().child(submit_btn)),
        )
    }
}
