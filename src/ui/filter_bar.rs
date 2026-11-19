use std::rc::Rc;
use std::time::{Duration, Instant};

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

/// Thickness of the loop's line.
const FILTER_EDGE_LINE: f32 = 2.;
/// One lap of the bright segment around the content's edge.
const FILTER_EDGE_LAP_MS: u128 = 2600;
/// How much of the perimeter the bright segment covers.
const FILTER_EDGE_SEGMENT: f32 = 0.22;
/// The segment is drawn in this many pieces, each a little brighter than the one behind it, so
/// that its tail fades out.
const FILTER_EDGE_PIECES: usize = 24;
/// Opacity of the dim track the segment runs along.
const FILTER_EDGE_TRACK_ALPHA: f32 = 0.20;
/// How far toward white the accent is pulled for the line.
const FILTER_EDGE_WHITE: f32 = 0.85;

/// The point `d` along the perimeter of a `w` × `h` rectangle with corner radius `r`, as an
/// offset from the rectangle's top-left. Clockwise, starting where the top edge begins (just past
/// the top-left corner). `d` wraps around the perimeter.
fn rounded_rect_point(w: f32, h: f32, r: f32, d: f32) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let straight_x = (w - 2. * r).max(0.);
    let straight_y = (h - 2. * r).max(0.);
    let arc = FRAC_PI_2 * r;
    let mut d = d.rem_euclid(2. * (straight_x + straight_y) + 4. * arc);

    if d < straight_x {
        return (r + d, 0.);
    }
    d -= straight_x;
    if d < arc {
        let a = -FRAC_PI_2 + d / r;
        return (w - r + r * a.cos(), r + r * a.sin());
    }
    d -= arc;
    if d < straight_y {
        return (w, r + d);
    }
    d -= straight_y;
    if d < arc {
        let a = d / r;
        return (w - r + r * a.cos(), h - r + r * a.sin());
    }
    d -= arc;
    if d < straight_x {
        return (w - r - d, h);
    }
    d -= straight_x;
    if d < arc {
        let a = FRAC_PI_2 + d / r;
        return (r + r * a.cos(), h - r + r * a.sin());
    }
    d -= arc;
    if d < straight_y {
        return (0., h - r - d);
    }
    d -= straight_y;
    let a = PI + d / r;
    (r + r * a.cos(), r + r * a.sin())
}

/// Length of the perimeter that [`rounded_rect_point`] walks.
fn rounded_rect_perimeter(w: f32, h: f32, r: f32) -> f32 {
    2. * ((w - 2. * r).max(0.) + (h - 2. * r).max(0.)) + 2. * std::f32::consts::PI * r
}

/// Strokes the stretch of the perimeter from distance `from` to `to`.
fn stroke_perimeter(
    window: &mut Window,
    origin: Point<Pixels>,
    size: (f32, f32, f32),
    (from, to): (f32, f32),
    step: f32,
    color: Hsla,
) {
    let (w, h, r) = size;
    let pieces = (((to - from) / step).ceil() as usize).max(1);
    let mut path = PathBuilder::stroke(px(FILTER_EDGE_LINE));
    for i in 0..=pieces {
        let d = from + (to - from) * i as f32 / pieces as f32;
        let (x, y) = rounded_rect_point(w, h, r, d);
        let p = point(origin.x + px(x), origin.y + px(y));
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Paints the loop around the content card inside the frame `bounds`: a dim track along the
/// card's outer edge and, when `lap` is given (how far round it is, `0..1`), a bright segment
/// with a fading tail running along it. The line hugs the card, just outside it, so it follows
/// the card's rounded corners.
fn paint_edge_loop(bounds: Bounds<Pixels>, color: Hsla, lap: Option<f32>, window: &mut Window) {
    let half = FILTER_EDGE_LINE / 2.;
    let origin = point(
        bounds.left() + px(FILTER_FRAME_SIDE - half),
        bounds.top() + px(FILTER_FRAME_TOP - half),
    );
    let w = f32::from(bounds.size.width) - 2. * FILTER_FRAME_SIDE + FILTER_EDGE_LINE;
    let h = f32::from(bounds.size.height) - FILTER_FRAME_TOP - FILTER_FRAME_SIDE + FILTER_EDGE_LINE;
    let size = (w, h, FILTER_FRAME_RADIUS + half);
    let total = rounded_rect_perimeter(w, h, size.2);

    stroke_perimeter(
        window,
        origin,
        size,
        (0., total),
        4.,
        color.opacity(FILTER_EDGE_TRACK_ALPHA),
    );
    let Some(lap) = lap else { return };
    let length = total * FILTER_EDGE_SEGMENT;
    let head = lap * total;
    let piece = length / FILTER_EDGE_PIECES as f32;
    for k in 0..FILTER_EDGE_PIECES {
        let start = head - length + piece * k as f32;
        // Overlap neighbouring pieces a little so the joins leave no gap.
        let end = start + piece + 0.8;
        let brightness = ((k + 1) as f32 / FILTER_EDGE_PIECES as f32).powf(1.6);
        stroke_perimeter(
            window,
            origin,
            size,
            (start, end),
            3.,
            color.opacity(brightness),
        );
    }
}

/// A thin line that keeps running around the content card's outer edge while a filter is on, like
/// an indeterminate progress bar around the frame.
///
/// This is its own view on purpose. GPUI redraws the view that owns a running animation on
/// every frame; owned by the workspace, that would rebuild the whole window 60 times a second
/// for as long as a filter is on. Here only this small view is redrawn. When the system asks for
/// reduced motion, it draws just the still track.
pub struct FilterFrameEdge {
    accent: Hsla,
    started: Instant,
}

impl FilterFrameEdge {
    pub fn new(accent: Hsla) -> Self {
        Self {
            accent,
            started: Instant::now(),
        }
    }

    pub fn set_accent(&mut self, accent: Hsla, cx: &mut Context<Self>) {
        if self.accent != accent {
            self.accent = accent;
            cx.notify();
        }
    }
}

impl Render for FilterFrameEdge {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Pulled toward white, not the label's colour: that is black on a pale accent.
        let color = lerp_color(self.accent, white(), FILTER_EDGE_WHITE);
        let lap = (!cx.reduce_motion()).then(|| {
            window.request_animation_frame();
            (self.started.elapsed().as_millis() % FILTER_EDGE_LAP_MS) as f32
                / FILTER_EDGE_LAP_MS as f32
        });
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| paint_edge_loop(bounds, color, lap, window),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
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
    fn frame_progress_runs_forward_when_entering_and_back_when_leaving() {
        assert_eq!(frame_progress(true, 0.0), 0.0);
        assert_eq!(frame_progress(true, 1.0), 1.0);
        assert_eq!(frame_progress(false, 0.0), 1.0);
        assert_eq!(frame_progress(false, 1.0), 0.0);
        assert_eq!(frame_progress(true, 2.0), 1.0);
        assert_eq!(frame_progress(false, -1.0), 1.0);
    }

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[core::prelude::v1::test]
    fn the_loop_walks_the_rounded_rectangle_clockwise_from_the_top_edge() {
        let (w, h, r) = (100., 60., 10.);
        let arc = std::f32::consts::FRAC_PI_2 * r;
        let top = w - 2. * r;
        let side = h - 2. * r;
        // Start of the top edge, then its end.
        assert!(near(rounded_rect_point(w, h, r, 0.), (r, 0.)));
        assert!(near(rounded_rect_point(w, h, r, top), (w - r, 0.)));
        // After each corner: the right edge, bottom edge, left edge and top edge begin.
        assert!(near(rounded_rect_point(w, h, r, top + arc), (w, r)));
        assert!(near(
            rounded_rect_point(w, h, r, top + arc + side + arc),
            (w - r, h)
        ));
        assert!(near(
            rounded_rect_point(w, h, r, top + arc + side + arc + top + arc),
            (0., h - r)
        ));
        let lap = rounded_rect_perimeter(w, h, r);
        assert!(near(rounded_rect_point(w, h, r, lap), (r, 0.)));
        // Down the right side (y grows) and back up the left (y shrinks).
        assert!(
            rounded_rect_point(w, h, r, top + arc + 5.).1
                > rounded_rect_point(w, h, r, top + arc).1
        );
    }

    #[core::prelude::v1::test]
    fn the_loop_stays_on_the_outline_and_has_no_jumps() {
        let (w, h, r) = (120., 80., 9.);
        let lap = rounded_rect_perimeter(w, h, r);
        let mut previous = rounded_rect_point(w, h, r, 0.);
        let steps = 2000;
        for i in 1..=steps {
            let p = rounded_rect_point(w, h, r, lap * i as f32 / steps as f32);
            // Every point is on the rectangle's outline.
            assert!((0.0..=w).contains(&p.0) && (0.0..=h).contains(&p.1));
            // No two neighbouring samples are further apart than their spacing allows.
            let gap = ((p.0 - previous.0).powi(2) + (p.1 - previous.1).powi(2)).sqrt();
            assert!(
                gap <= lap / steps as f32 * 1.01,
                "jump of {gap} at step {i}"
            );
            previous = p;
        }
        // Negative distances wrap, so a tail starting "behind" the start still lands on the path.
        assert!(near(
            rounded_rect_point(w, h, r, -lap + 3.),
            rounded_rect_point(w, h, r, 3.)
        ));
    }
}
