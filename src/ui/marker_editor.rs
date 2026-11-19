//! Floating form for adding, editing and removing a log marker.

use std::rc::Rc;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::MarkerColor;
use crate::ui::receive_toolbar::ToolbarActionCallback;
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::contrast_foreground;

pub type ColorCallback = Rc<dyn Fn(MarkerColor, &mut Window, &mut App)>;

/// Width of the floating form, used to keep it inside the window.
pub const MARKER_EDITOR_WIDTH: Pixels = px(268.);
/// Approximate height of the form, used to keep it inside the window.
pub const MARKER_EDITOR_HEIGHT: Pixels = px(190.);

/// A marker color as a GPUI color with the given opacity.
pub fn marker_color_hsla(color: MarkerColor, alpha: f32) -> Hsla {
    Hsla::from(rgb(color.rgb())).alpha(alpha)
}

/// The form currently open for one log entry.
pub struct MarkerEditorState {
    pub tab_id: u64,
    pub entry_id: u64,
    pub position: Point<Pixels>,
    pub color: MarkerColor,
    pub input: Entity<InputState>,
    /// The entry already has a marker: show Delete and prefill the form.
    pub existing: bool,
    pub error: Option<SharedString>,
}

#[derive(IntoElement)]
pub struct MarkerEditor {
    pub theme: NanoTheme,
    pub locale: Locale,
    pub position: Point<Pixels>,
    pub color: MarkerColor,
    pub existing: bool,
    pub input: Entity<InputState>,
    pub error: Option<SharedString>,
    pub on_pick_color: ColorCallback,
    pub on_confirm: ToolbarActionCallback,
    pub on_cancel: ToolbarActionCallback,
    pub on_delete: ToolbarActionCallback,
}

impl MarkerEditor {
    fn button(
        theme: NanoTheme,
        id: &'static str,
        label: SharedString,
        primary: bool,
        danger: bool,
        on_click: ToolbarActionCallback,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(26.))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .text_xs()
            .when(primary, |this| {
                this.bg(theme.accent_blue)
                    .text_color(contrast_foreground(theme.accent_blue))
                    .hover(move |s| s.bg(theme.accent_blue_hover))
            })
            .when(!primary, |this| {
                this.border_1()
                    .border_color(theme.border_card)
                    .text_color(if danger {
                        theme.error_text
                    } else {
                        theme.text_secondary
                    })
                    .hover(move |s| s.bg(theme.bg_hover))
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_click(window, cx);
            })
            .child(label)
    }
}

impl RenderOnce for MarkerEditor {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let locale = self.locale;
        let selected = self.color;

        let swatches = MarkerColor::ALL
            .into_iter()
            .enumerate()
            .map(|(idx, color)| {
                let on_pick = self.on_pick_color.clone();
                div()
                    .id(ElementId::named_usize("marker-color", idx))
                    .size(px(20.))
                    .rounded_full()
                    .border_2()
                    .border_color(if color == selected {
                        theme.text_primary
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .p(px(2.))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_pick(color, window, cx);
                    })
                    .child(
                        div()
                            .size_full()
                            .rounded_full()
                            .bg(marker_color_hsla(color, 1.0)),
                    )
            });

        let dismiss = {
            let on_cancel = self.on_cancel.clone();
            move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                cx.stop_propagation();
                on_cancel(window, cx);
            }
        };
        let swallow = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation();

        let title = if self.existing {
            tr_shared(locale, "marker.title_edit")
        } else {
            tr_shared(locale, "marker.title_new")
        };

        div()
            .id("marker-editor-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // Also swallows wheel events so the log cannot scroll away under the form.
            .occlude()
            .on_mouse_down(MouseButton::Left, dismiss.clone())
            .on_mouse_down(MouseButton::Right, dismiss)
            .child(
                v_flex()
                    .id("marker-editor")
                    .absolute()
                    .left(self.position.x)
                    .top(self.position.y)
                    .w(MARKER_EDITOR_WIDTH)
                    .p_3()
                    .gap_2()
                    .rounded(px(8.))
                    .bg(theme.bg_popover)
                    .border_1()
                    .border_color(theme.border_popover)
                    .shadow_lg()
                    .text_color(theme.text_primary)
                    .occlude()
                    .on_mouse_down(MouseButton::Left, swallow)
                    .on_mouse_down(MouseButton::Right, swallow)
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                    .child(
                        div()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme.border_card)
                            .child(Input::new(&self.input).appearance(false)),
                    )
                    .child(h_flex().gap_1p5().children(swatches))
                    .when_some(self.error, |this, err| {
                        this.child(div().text_xs().text_color(theme.error_text).child(err))
                    })
                    .child(
                        h_flex()
                            .gap_1p5()
                            .justify_end()
                            .when(self.existing, |this| {
                                this.child(Self::button(
                                    theme,
                                    "marker-delete",
                                    tr_shared(locale, "marker.delete"),
                                    false,
                                    true,
                                    self.on_delete.clone(),
                                ))
                                .child(div().flex_1())
                            })
                            .child(Self::button(
                                theme,
                                "marker-cancel",
                                tr_shared(locale, "marker.cancel"),
                                false,
                                false,
                                self.on_cancel.clone(),
                            ))
                            .child(Self::button(
                                theme,
                                "marker-ok",
                                tr_shared(locale, "marker.ok"),
                                true,
                                false,
                                self.on_confirm.clone(),
                            )),
                    ),
            )
    }
}
