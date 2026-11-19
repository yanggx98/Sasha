use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::i18n::{Locale, filter_banner_label, tr_shared};
use crate::model::log_filter::{FilterDirection, LogFilter};
use crate::ui::motion::lerp_color;
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::contrast_foreground;

#[derive(Clone, Copy)]
pub enum FilterControl {
    CycleDirection,
    CaseSensitive,
    Regex,
    Exclude,
    Reset,
    Apply,
}

pub type FilterCallback = Rc<dyn Fn(FilterControl, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct LogFilterBar {
    pub theme: NanoTheme,
    pub locale: Locale,
    pub input: Entity<InputState>,
    pub filter: LogFilter,
    pub invalid: bool,
    /// The draft differs from the filter in effect.
    pub pending: bool,
    pub on_control: FilterCallback,
}

impl LogFilterBar {
    fn button(
        &self,
        id: &'static str,
        label_key: &'static str,
        tooltip_key: &'static str,
        active: bool,
        control: FilterControl,
    ) -> impl IntoElement {
        let callback = self.on_control.clone();
        let tooltip = tr_shared(self.locale, tooltip_key);
        let theme = self.theme;
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .h(px(22.))
            .min_w(px(if matches!(control, FilterControl::CycleDirection) {
                42.
            } else {
                22.
            }))
            .px_1()
            .rounded(px(6.))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if active {
                theme.accent_blue_hover
            } else {
                theme.text_secondary
            })
            .hover(move |s| s.bg(theme.bg_hover))
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                callback(control, window, cx);
            })
            .child(tr_shared(self.locale, label_key))
    }
}

/// Height of the frame's top edge: the title bar band at the top of the window.
pub const FILTER_FRAME_TOP: f32 = 34.;
/// Thickness of the frame's left, right and bottom edges.
pub const FILTER_FRAME_SIDE: f32 = 6.;
/// Radius of the frame's outer top-left corner and of all four corners of the content inside.
pub const FILTER_FRAME_RADIUS: f32 = 8.;

/// How long the content takes to shrink into the frame (or grow back out of it).
pub const FILTER_FRAME_TRANSITION_MS: u64 = 220;

/// One run of the frame switching on (`entering`) or off. `key` is unique per run and per tab,
/// so each run is a fresh animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTransition {
    pub key: u64,
    pub entering: bool,
}

/// How far into the frame the content is: 0 = filling the area, 1 = fully inset.
fn frame_progress(entering: bool, delta: f32) -> f32 {
    let delta = delta.clamp(0.0, 1.0);
    if entering { delta } else { 1.0 - delta }
}

fn frame_animation() -> Animation {
    Animation::new(Duration::from_millis(FILTER_FRAME_TRANSITION_MS))
        .with_easing(gpui_kit::base::animation::ease_out_cubic)
}

/// Grows the frame's padding from nothing to its full size, so the content inside it shrinks
/// away from the edges and the accent fill is revealed around it. Without a transition the
/// element is left as styled.
pub fn animate_frame_inset<E>(element: E, transition: Option<FrameTransition>) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let Some(FrameTransition { key, entering }) = transition else {
        return element.into_any_element();
    };
    element
        .with_animation(
            ElementId::NamedInteger("filter-frame-inset".into(), key),
            frame_animation(),
            move |element, delta| {
                let inset = frame_progress(entering, delta);
                element
                    .pt(px(FILTER_FRAME_TOP * inset))
                    .px(px(FILTER_FRAME_SIDE * inset))
                    .pb(px(FILTER_FRAME_SIDE * inset))
            },
        )
        .into_any_element()
}

/// Rounds the content's corners up from square as it shrinks into the frame (and back down as
/// it grows out of it).
pub fn animate_content_corners<E>(element: E, transition: Option<FrameTransition>) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let Some(FrameTransition { key, entering }) = transition else {
        return element.into_any_element();
    };
    element
        .with_animation(
            ElementId::NamedInteger("filter-frame-corners".into(), key),
            frame_animation(),
            move |element, delta| {
                element.rounded(px(FILTER_FRAME_RADIUS * frame_progress(entering, delta)))
            },
        )
        .into_any_element()
}

/// How far toward white the accent is pulled to make the colour of the glow and the breath.
const FILTER_FX_WASH_WHITE: f32 = 0.7;
const FILTER_FX_BREATH_MS: u64 = 3200;
const FILTER_FX_RISE_MS: u64 = 3600;
/// How far the frame's accent shifts toward the wash colour at the peak of a breath.
const FILTER_FX_GLOW_ALPHA: f32 = 0.24;
/// Peak opacity of the glow that rises through the frame.
const FILTER_FX_RISE_ALPHA: f32 = 0.60;
/// Height of that glow, as a fraction of the frame's height.
const FILTER_FX_RISE_HEIGHT: f32 = 0.42;

/// Breathing curve over one period: 0 at both ends, 1 in the middle, smooth at the loop
/// point (a plain ease would kink there).
fn breath_curve(t: f32) -> f32 {
    0.5 - 0.5 * (t * std::f32::consts::TAU).cos()
}

/// Top edge of the rising glow, as a fraction of the frame's height. The glow stays inside the
/// frame the whole time: its bottom edge on the frame's bottom at `progress == 0`, its top edge
/// on the frame's top at `1`. Staying inside means nothing has to be clipped (the frame's
/// top-left corner is round and clipping can only be rectangular).
fn rise_top(progress: f32) -> f32 {
    (1.0 - FILTER_FX_RISE_HEIGHT) * (1.0 - progress)
}

/// Opacity of the rising glow: 0 at both ends and 1 in the middle, so it fades in and out in
/// place instead of sliding in from outside the frame.
fn rise_opacity(progress: f32) -> f32 {
    (progress * std::f32::consts::PI).sin()
}

/// The glow that rises through the frame: a band that fades in over its upper half and out over
/// its lower half, so it has no hard edge.
fn rising_glow(id: &'static str, wash: Hsla) -> impl IntoElement {
    let leading = linear_gradient(
        180.,
        linear_color_stop(wash.opacity(0.), 0.),
        linear_color_stop(wash.opacity(FILTER_FX_RISE_ALPHA), 1.),
    );
    let trailing = linear_gradient(
        180.,
        linear_color_stop(wash.opacity(FILTER_FX_RISE_ALPHA), 0.),
        linear_color_stop(wash.opacity(0.), 1.),
    );
    div()
        .absolute()
        .left_0()
        .w_full()
        .h(relative(FILTER_FX_RISE_HEIGHT))
        .flex()
        .flex_col()
        .child(div().w_full().flex_1().bg(leading))
        .child(div().w_full().flex_1().bg(trailing))
        .with_animation(
            id,
            Animation::new(Duration::from_millis(FILTER_FX_RISE_MS)).repeat(),
            |glow, progress| {
                glow.top(relative(rise_top(progress)))
                    .opacity(rise_opacity(progress))
            },
        )
}

/// The filter frame's accent fill and its continuous effects: the whole fill breathes (its
/// colour drifts toward the wash and back) while a glow keeps rising through it from the
/// bottom to the top.
///
/// This is its own view on purpose. GPUI redraws the view that owns a running animation on
/// every frame; owned by the workspace, that would rebuild the whole window 60 times a second
/// for as long as a filter is on. Here only this small view is redrawn.
pub struct FilterFrameFx {
    accent: Hsla,
}

impl FilterFrameFx {
    pub fn new(accent: Hsla) -> Self {
        Self { accent }
    }

    pub fn set_accent(&mut self, accent: Hsla, cx: &mut Context<Self>) {
        if self.accent != accent {
            self.accent = accent;
            cx.notify();
        }
    }
}

impl Render for FilterFrameFx {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let accent = self.accent;
        // The glow and the breathing both lighten the frame: the accent pulled toward white.
        // (Not the label's colour: that is black on a pale accent, and a glow has to brighten.)
        let wash = lerp_color(accent, white(), FILTER_FX_WASH_WHITE);

        // This is the frame's whole accent fill, painted beneath the content: the edges and the
        // pockets that the content's rounded corners uncover are the same surface, so they
        // breathe together. (A glow laid over just the edges would leave the pockets behind.)
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded_tl(px(FILTER_FRAME_RADIUS))
            .bg(accent)
            .child(rising_glow("filter-frame-rise", wash))
            .with_animation(
                "filter-frame-breath",
                Animation::new(Duration::from_millis(FILTER_FX_BREATH_MS))
                    .repeat()
                    .with_easing(breath_curve),
                move |fill, glow| fill.bg(lerp_color(accent, wash, glow * FILTER_FX_GLOW_ALPHA)),
            )
    }
}

/// Filter summary shown on the top edge of the accent frame (see `workspace-frame`). Place
/// it in the frame, which must be `relative`; it sits in the band above the content.
#[derive(IntoElement)]
pub struct FilterFrameLabel {
    pub theme: NanoTheme,
    pub locale: Locale,
    pub visible: usize,
    pub total: usize,
}

impl RenderOnce for FilterFrameLabel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let hidden = self.total.saturating_sub(self.visible);
        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(FILTER_FRAME_TOP))
            .px_4()
            .flex()
            .items_center()
            .justify_center()
            .text_color(contrast_foreground(self.theme.accent_blue))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().min_w_0().truncate().child(filter_banner_label(
                self.locale,
                self.visible,
                self.total,
                hidden,
            )))
    }
}

impl RenderOnce for LogFilterBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let direction = self.filter.direction;
        let separator = || {
            div()
                .flex_shrink_0()
                .w(px(1.))
                .h(px(14.))
                .mx_0p5()
                .bg(self.theme.border_card)
        };
        let row = h_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .items_center()
            .child(self.button(
                "filter-direction",
                match direction {
                    FilterDirection::All => "filter.all",
                    FilterDirection::Rx => "filter.rx",
                    FilterDirection::Tx => "filter.tx",
                },
                "filter.direction_tooltip",
                direction != FilterDirection::All,
                FilterControl::CycleDirection,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(48.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .px_1p5()
                    .rounded(px(6.))
                    .bg(self.theme.bg_card)
                    .border_1()
                    .border_color(if self.invalid {
                        self.theme.error_text
                    } else {
                        self.theme.border_card
                    })
                    .text_sm()
                    .text_color(self.theme.text_primary)
                    .child(Input::new(&self.input).appearance(false).cleanable(true)),
            )
            .child(self.button(
                "filter-case",
                "filter.case_label",
                "filter.case_tooltip",
                self.filter.case_sensitive,
                FilterControl::CaseSensitive,
            ))
            .child(self.button(
                "filter-regex",
                "filter.regex_label",
                "filter.regex_tooltip",
                self.filter.regex,
                FilterControl::Regex,
            ))
            .child(self.button(
                "filter-exclude",
                "filter.exclude_label",
                "filter.exclude_tooltip",
                self.filter.exclude,
                FilterControl::Exclude,
            ))
            .child(separator())
            .child(self.button(
                "filter-reset",
                "filter.reset_label",
                "filter.reset_tooltip",
                false,
                FilterControl::Reset,
            ))
            .child(self.button(
                "filter-apply",
                "filter.apply_label",
                "filter.apply_tooltip",
                self.pending,
                FilterControl::Apply,
            ));

        // Content of the toolbar's expanding drawer; the toolbar owns its
        // padding, surface and border. This stays outside the title drag area.
        v_flex()
            .id("log-filter-bar")
            .w_full()
            .flex_shrink_0()
            .py_0p5()
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(row)
            .children(self.invalid.then(|| {
                div()
                    .text_xs()
                    .text_color(self.theme.error_text)
                    .child(tr_shared(self.locale, "filter.invalid_regex"))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn breath_curve_is_a_smooth_loop_between_zero_and_one() {
        assert!(breath_curve(0.0).abs() < 1e-6);
        assert!((breath_curve(0.5) - 1.0).abs() < 1e-6);
        assert!(breath_curve(1.0).abs() < 1e-5);
        // Same value (and slope) at the loop point, so there is no jump when it repeats.
        assert!((breath_curve(0.001) - breath_curve(0.999)).abs() < 1e-4);
        for step in 0..=100 {
            let value = breath_curve(step as f32 / 100.0);
            assert!((0.0..=1.0).contains(&value));
        }
    }

    #[core::prelude::v1::test]
    fn rising_glow_stays_inside_the_frame_and_moves_up() {
        // Bottom edge (top + height) on the frame's bottom at the start, top edge on its top at
        // the end: it never leaves the frame, so it needs no clipping.
        assert!((rise_top(0.0) + FILTER_FX_RISE_HEIGHT - 1.0).abs() < 1e-6);
        assert!(rise_top(1.0).abs() < 1e-6);
        for step in 0..=100 {
            let top = rise_top(step as f32 / 100.0);
            assert!(top >= -1e-6 && top + FILTER_FX_RISE_HEIGHT <= 1.0 + 1e-6);
        }
        // It moves up, not down.
        assert!(rise_top(0.5) < rise_top(0.25));
    }

    #[core::prelude::v1::test]
    fn frame_progress_runs_forward_when_entering_and_back_when_leaving() {
        assert_eq!(frame_progress(true, 0.0), 0.0);
        assert_eq!(frame_progress(true, 1.0), 1.0);
        assert_eq!(frame_progress(false, 0.0), 1.0);
        assert_eq!(frame_progress(false, 1.0), 0.0);
        assert_eq!(frame_progress(true, 2.0), 1.0);
        assert_eq!(frame_progress(false, -1.0), 1.0);
    }

    #[core::prelude::v1::test]
    fn rising_glow_fades_in_and_out_in_place() {
        assert!(rise_opacity(0.0).abs() < 1e-6);
        assert!(rise_opacity(1.0).abs() < 1e-5);
        assert!((rise_opacity(0.5) - 1.0).abs() < 1e-6);
    }
}
