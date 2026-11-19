use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::*;

use crate::model::MarkerColor;
use crate::ui::marker_editor::marker_color_hsla;
use crate::ui::theme::NanoTheme;

/// Width of the log navigation minimap replacing the vertical scrollbar.
pub const MINIMAP_WIDTH: Pixels = px(14.);

/// Minimum height of the viewport thumb so it remains easy to grab on huge logs.
const MIN_THUMB_HEIGHT: Pixels = px(24.);

/// Horizontal inset for the thumb and markers inside the minimap column.
const INNER_INSET_X: Pixels = px(2.);

/// Visual height of a single marker bar.
const MARKER_BAR_HEIGHT: Pixels = px(3.);

/// Vertical hit radius around a marker center when clicking to jump.
const MARKER_HIT_HALF_HEIGHT: Pixels = px(4.);

/// Tolerance in pixels for considering the scroll position to be at the bottom.
const BOTTOM_EPSILON: Pixels = px(1.);

/// Called with a log entry id when a minimap marker is clicked.
pub type JumpCallback = Rc<dyn Fn(u64, &mut Window, &mut App)>;

/// Called when manual minimap interaction moves the viewport away from the bottom.
pub type DisableAutoScrollCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// A visible marker projected onto the minimap's vertical axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapMarker {
    pub entry_id: u64,
    pub fraction: f32,
    pub color: MarkerColor,
}

/// Pure geometric layout of the minimap thumb and scroll mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapMetrics {
    pub track_height: Pixels,
    pub max_scroll_y: Pixels,
    pub thumb_top: Pixels,
    pub thumb_height: Pixels,
}

impl MinimapMetrics {
    /// Computes thumb geometry from the track height, viewport height, and list scroll state.
    /// Returns `None` when the content fits inside the viewport (nothing to scroll).
    pub fn compute(
        track_height: Pixels,
        viewport_height: Pixels,
        max_scroll_y: Pixels,
        scroll_pos: Pixels,
    ) -> Option<Self> {
        if track_height <= px(0.) || viewport_height <= px(0.) || max_scroll_y <= px(0.) {
            return None;
        }
        let content_height = viewport_height + max_scroll_y;
        let raw_thumb = track_height * (viewport_height / content_height);
        let thumb_height = raw_thumb.max(MIN_THUMB_HEIGHT).min(track_height);
        let thumb_travel = (track_height - thumb_height).max(px(0.));
        let fraction = (scroll_pos.clamp(px(0.), max_scroll_y) / max_scroll_y).clamp(0.0, 1.0);
        let thumb_top = thumb_travel * fraction;
        Some(Self {
            track_height,
            max_scroll_y,
            thumb_top,
            thumb_height,
        })
    }

    pub fn thumb_travel(&self) -> Pixels {
        (self.track_height - self.thumb_height).max(px(0.))
    }

    pub fn contains_thumb(&self, local_y: Pixels) -> bool {
        local_y >= self.thumb_top && local_y <= self.thumb_top + self.thumb_height
    }

    /// Converts a pointer `local_y` and `grab_offset_y` (distance from thumb top) into a
    /// positive scroll offset in `0..=max_scroll_y`.
    pub fn scroll_pos_for_drag(&self, local_y: Pixels, grab_offset_y: Pixels) -> Pixels {
        let travel = self.thumb_travel();
        if travel <= px(0.) {
            return px(0.);
        }
        let target_top = (local_y - grab_offset_y).clamp(px(0.), travel);
        let fraction = (target_top / travel).clamp(0.0, 1.0);
        self.max_scroll_y * fraction
    }

    /// Converts a track click at `local_y` into `(scroll_pos, grab_offset_y)` that centers
    /// the thumb on the clicked point and lets the user keep dragging seamlessly.
    pub fn scroll_pos_for_track_click(&self, local_y: Pixels) -> (Pixels, Pixels) {
        let grab_offset = self.thumb_height * 0.5;
        let scroll_pos = self.scroll_pos_for_drag(local_y, grab_offset);
        (scroll_pos, grab_offset)
    }
}

/// Finds the marker closest to `local_y` within the click hit radius.
pub fn hit_test_marker(
    markers: &[MinimapMarker],
    track_height: Pixels,
    local_y: Pixels,
) -> Option<u64> {
    if track_height <= px(0.) {
        return None;
    }
    let mut best: Option<(u64, Pixels)> = None;
    for marker in markers {
        let center_y = track_height * marker.fraction.clamp(0.0, 1.0);
        let dist = (local_y - center_y).abs();
        if dist <= MARKER_HIT_HALF_HEIGHT {
            match best {
                Some((_, best_dist)) if dist >= best_dist => {}
                _ => best = Some((marker.entry_id, dist)),
            }
        }
    }
    best.map(|(id, _)| id)
}

#[derive(Default)]
pub struct MinimapInteraction {
    /// Active drag grab offset relative to the top of the thumb.
    drag_grab_offset: Option<Pixels>,
    hovered_track: bool,
    hovered_thumb: bool,
    hovered_marker: bool,
}

/// Interactive minimap strip that replaces the vertical scrollbar in `LogView`.
///
/// Renders the viewport thumb and visible log markers, and handles thumb dragging,
/// track click-to-jump, marker click-to-jump, and scroll wheel forwarding.
pub struct LogMinimap {
    id: ElementId,
    theme: NanoTheme,
    list_state: ListState,
    markers: Rc<Vec<MinimapMarker>>,
    line_height: Pixels,
    on_jump: JumpCallback,
    on_disable_auto_scroll: DisableAutoScrollCallback,
}

impl LogMinimap {
    pub fn new(
        theme: NanoTheme,
        list_state: ListState,
        markers: Vec<MinimapMarker>,
        line_height: Pixels,
        on_jump: JumpCallback,
        on_disable_auto_scroll: DisableAutoScrollCallback,
    ) -> Self {
        Self {
            id: ElementId::Name("log-minimap".into()),
            theme,
            list_state,
            markers: Rc::new(markers),
            line_height,
            on_jump,
            on_disable_auto_scroll,
        }
    }

    fn current_metrics(list_state: &ListState, track_height: Pixels) -> Option<MinimapMetrics> {
        let viewport_height = list_state.viewport_bounds().size.height;
        let max_scroll_y = list_state.max_offset_for_scrollbar().y.max(px(0.));
        let scroll_pos =
            (-list_state.scroll_px_offset_for_scrollbar().y).clamp(px(0.), max_scroll_y);
        MinimapMetrics::compute(track_height, viewport_height, max_scroll_y, scroll_pos)
    }

    fn apply_scroll_pos(
        list_state: &ListState,
        target_scroll_pos: Pixels,
        max_scroll_y: Pixels,
        on_disable_auto_scroll: &DisableAutoScrollCallback,
        window: &mut Window,
        cx: &mut App,
    ) {
        let clamped = target_scroll_pos.clamp(px(0.), max_scroll_y);
        list_state.set_offset_from_scrollbar(point(px(0.), -clamped));
        if clamped < max_scroll_y - BOTTOM_EPSILON {
            on_disable_auto_scroll(window, cx);
        }
        window.refresh();
    }
}

impl IntoElement for LogMinimap {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LogMinimap {
    type RequestLayoutState = Rc<RefCell<MinimapInteraction>>;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = MINIMAP_WIDTH.into();
        style.size.height = relative(1.0).into();
        style.flex_shrink = 0.0;
        let layout_id = window.request_layout(style, [], cx);

        let interaction = match id {
            Some(global_id) => window.with_element_state(
                global_id,
                |retained: Option<Rc<RefCell<MinimapInteraction>>>, _| {
                    let state = retained.unwrap_or_default();
                    (state.clone(), state)
                },
            ),
            None => Rc::new(RefCell::new(MinimapInteraction::default())),
        };

        (layout_id, interaction)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        interaction: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let track_height = bounds.size.height;
        let metrics = Self::current_metrics(&self.list_state, track_height);
        let state = interaction.borrow();
        let is_dragging = state.drag_grab_offset.is_some();
        let hovered_track = state.hovered_track;
        let hovered_thumb = state.hovered_thumb;
        let hovered_marker = state.hovered_marker;
        drop(state);

        // Subtle track highlight only while hovered or dragging.
        if (hovered_track || is_dragging) && metrics.is_some() {
            window.paint_quad(PaintQuad {
                bounds,
                background: self.theme.text_muted.alpha(0.06).into(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });
        }

        // Viewport thumb.
        if let Some(m) = metrics {
            let thumb_color = if is_dragging {
                self.theme.text_secondary.alpha(0.75)
            } else if hovered_thumb {
                self.theme.text_secondary.alpha(0.58)
            } else if hovered_track {
                self.theme.text_muted.alpha(0.45)
            } else {
                self.theme.text_muted.alpha(0.32)
            };

            let thumb_width = (bounds.size.width - INNER_INSET_X * 2.).max(px(2.));
            let thumb_bounds = Bounds {
                origin: point(bounds.left() + INNER_INSET_X, bounds.top() + m.thumb_top),
                size: size(thumb_width, m.thumb_height),
            };
            window.paint_quad(PaintQuad {
                bounds: thumb_bounds,
                background: thumb_color.into(),
                corner_radii: Corners::all(px(3.)),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });
        }

        // Visible markers painted above the thumb so they remain distinct and clickable.
        let marker_width = (bounds.size.width - INNER_INSET_X * 2.).max(px(2.));
        for marker in self.markers.iter() {
            let center_y = track_height * marker.fraction.clamp(0.0, 1.0);
            let top_y = (center_y - MARKER_BAR_HEIGHT * 0.5)
                .clamp(px(0.), (track_height - MARKER_BAR_HEIGHT).max(px(0.)));
            let marker_bounds = Bounds {
                origin: point(bounds.left() + INNER_INSET_X, bounds.top() + top_y),
                size: size(marker_width, MARKER_BAR_HEIGHT),
            };
            window.paint_quad(PaintQuad {
                bounds: marker_bounds,
                background: marker_color_hsla(marker.color, 1.0).into(),
                corner_radii: Corners::all(px(1.5)),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });
        }

        if hovered_marker {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }

        // Mouse down: marker jump takes precedence over thumb drag / track click.
        {
            let interaction = interaction.clone();
            let list_state = self.list_state.clone();
            let markers = self.markers.clone();
            let on_jump = self.on_jump.clone();
            let on_disable_auto_scroll = self.on_disable_auto_scroll.clone();
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                if !hitbox.is_hovered(window) {
                    return;
                }
                let local_y = event.position.y - bounds.top();
                if let Some(entry_id) = hit_test_marker(&markers, bounds.size.height, local_y) {
                    cx.stop_propagation();
                    on_jump(entry_id, window, cx);
                    return;
                }
                let Some(m) = Self::current_metrics(&list_state, bounds.size.height) else {
                    return;
                };
                cx.stop_propagation();
                if m.contains_thumb(local_y) {
                    let grab_offset = local_y - m.thumb_top;
                    interaction.borrow_mut().drag_grab_offset = Some(grab_offset);
                    window.refresh();
                } else {
                    let (target_pos, grab_offset) = m.scroll_pos_for_track_click(local_y);
                    interaction.borrow_mut().drag_grab_offset = Some(grab_offset);
                    Self::apply_scroll_pos(
                        &list_state,
                        target_pos,
                        m.max_scroll_y,
                        &on_disable_auto_scroll,
                        window,
                        cx,
                    );
                }
            });
        }

        // Mouse move: update active drag even when pointer leaves the narrow strip,
        // otherwise maintain hover state for thumb / track / marker feedback.
        {
            let interaction = interaction.clone();
            let list_state = self.list_state.clone();
            let markers = self.markers.clone();
            let on_disable_auto_scroll = self.on_disable_auto_scroll.clone();
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let grab_offset = interaction.borrow().drag_grab_offset;
                if let Some(grab_offset) = grab_offset {
                    if !event.dragging() {
                        interaction.borrow_mut().drag_grab_offset = None;
                        window.refresh();
                        return;
                    }
                    if let Some(m) = Self::current_metrics(&list_state, bounds.size.height) {
                        let local_y = event.position.y - bounds.top();
                        let target_pos = m.scroll_pos_for_drag(local_y, grab_offset);
                        Self::apply_scroll_pos(
                            &list_state,
                            target_pos,
                            m.max_scroll_y,
                            &on_disable_auto_scroll,
                            window,
                            cx,
                        );
                    }
                    return;
                }

                let in_track = hitbox.is_hovered(window);
                let (in_thumb, in_marker) = if in_track {
                    let local_y = event.position.y - bounds.top();
                    let in_marker =
                        hit_test_marker(&markers, bounds.size.height, local_y).is_some();
                    let in_thumb = Self::current_metrics(&list_state, bounds.size.height)
                        .is_some_and(|m| m.contains_thumb(local_y));
                    (in_thumb, in_marker)
                } else {
                    (false, false)
                };

                let mut state = interaction.borrow_mut();
                if state.hovered_track != in_track
                    || state.hovered_thumb != in_thumb
                    || state.hovered_marker != in_marker
                {
                    state.hovered_track = in_track;
                    state.hovered_thumb = in_thumb;
                    state.hovered_marker = in_marker;
                    window.refresh();
                }
            });
        }

        // Mouse up: end drag anywhere in the window.
        {
            let interaction = interaction.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                let mut state = interaction.borrow_mut();
                if state.drag_grab_offset.take().is_some() {
                    window.refresh();
                }
            });
        }

        // Scroll wheel over the minimap forwards directly to the log list.
        {
            let list_state = self.list_state.clone();
            let line_height = self.line_height;
            let on_disable_auto_scroll = self.on_disable_auto_scroll.clone();
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }
                let max_scroll_y = list_state.max_offset_for_scrollbar().y.max(px(0.));
                if max_scroll_y <= px(0.) {
                    return;
                }
                let delta_y = event.delta.pixel_delta(line_height).y;
                if delta_y == px(0.) {
                    return;
                }
                let current_pos =
                    (-list_state.scroll_px_offset_for_scrollbar().y).clamp(px(0.), max_scroll_y);
                let next_pos = (current_pos - delta_y).clamp(px(0.), max_scroll_y);
                cx.stop_propagation();
                Self::apply_scroll_pos(
                    &list_state,
                    next_pos,
                    max_scroll_y,
                    &on_disable_auto_scroll,
                    window,
                    cx,
                );
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::prelude::*;
    use gpui::{ListAlignment, ListState, Render, TestAppContext, div, list, point, px, size};

    use super::{MIN_THUMB_HEIGHT, MinimapMarker, MinimapMetrics, hit_test_marker};
    use crate::model::MarkerColor;

    #[test]
    fn metrics_returns_none_when_content_fits_viewport() {
        assert_eq!(
            MinimapMetrics::compute(px(400.), px(400.), px(0.), px(0.)),
            None
        );
        assert_eq!(
            MinimapMetrics::compute(px(0.), px(400.), px(200.), px(0.)),
            None
        );
    }

    #[test]
    fn metrics_enforces_min_thumb_height_on_huge_logs() {
        let m = MinimapMetrics::compute(px(400.), px(400.), px(100_000.), px(0.)).unwrap();
        assert_eq!(m.thumb_height, MIN_THUMB_HEIGHT);
        assert_eq!(m.thumb_top, px(0.));

        let bottom =
            MinimapMetrics::compute(px(400.), px(400.), px(100_000.), px(100_000.)).unwrap();
        assert_eq!(bottom.thumb_height, MIN_THUMB_HEIGHT);
        assert_eq!(bottom.thumb_top, px(400.) - MIN_THUMB_HEIGHT);
    }

    #[test]
    fn metrics_maps_drag_and_track_click_to_bounds() {
        let m = MinimapMetrics::compute(px(400.), px(200.), px(600.), px(0.)).unwrap();
        // viewport 200 / content 800 = 25% => thumb 100px, travel 300px.
        assert_eq!(m.thumb_height, px(100.));
        assert_eq!(m.thumb_travel(), px(300.));

        // Dragging to top / middle / bottom.
        assert_eq!(m.scroll_pos_for_drag(px(0.), px(10.)), px(0.));
        assert_eq!(m.scroll_pos_for_drag(px(160.), px(10.)), px(300.));
        assert_eq!(m.scroll_pos_for_drag(px(999.), px(10.)), px(600.));

        // Track click centers thumb on clicked y.
        let (mid_pos, grab) = m.scroll_pos_for_track_click(px(200.));
        assert_eq!(grab, px(50.));
        assert_eq!(mid_pos, px(300.));
    }

    #[test]
    fn marker_hit_test_picks_nearest_within_radius() {
        let markers = vec![
            MinimapMarker {
                entry_id: 10,
                fraction: 0.25,
                color: MarkerColor::Red,
            },
            MinimapMarker {
                entry_id: 20,
                fraction: 0.26,
                color: MarkerColor::Blue,
            },
        ];
        // Track 400px => centers at 100px and 104px.
        assert_eq!(hit_test_marker(&markers, px(400.), px(99.)), Some(10));
        assert_eq!(hit_test_marker(&markers, px(400.), px(105.)), Some(20));
        assert_eq!(hit_test_marker(&markers, px(400.), px(200.)), None);
    }

    #[gpui::test]
    fn minimap_scroll_mapping_reaches_top_and_bottom(cx: &mut TestAppContext) {
        let mut cx = cx.add_empty_window();
        let list_state =
            ListState::new(40, ListAlignment::Top, px(10.)).with_uniform_item_height(px(30.));

        struct TestView(ListState);
        impl Render for TestView {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl IntoElement {
                list(self.0.clone(), |_, _, _| {
                    div().h(px(30.)).w_full().into_any()
                })
                .w_full()
                .h_full()
            }
        }

        let draw = |state: &ListState, cx: &mut gpui::VisualTestContext| {
            cx.draw(point(px(0.), px(0.)), size(px(100.), px(120.)), |_, cx| {
                cx.new(|_| TestView(state.clone())).into_any_element()
            });
        };

        list_state.scroll_to_end();
        draw(&list_state, &mut cx);
        let _ = list_state.clone().with_uniform_item_height(px(30.));
        draw(&list_state, &mut cx);

        let metrics_bottom = super::LogMinimap::current_metrics(&list_state, px(120.))
            .expect("list taller than viewport must produce thumb metrics");
        assert!(
            metrics_bottom.thumb_top > px(0.),
            "when scrolled to end, thumb must be near the bottom"
        );

        let disabled = Rc::new(Cell::new(false));
        let disabled_cb = disabled.clone();
        let cb: super::DisableAutoScrollCallback = Rc::new(move |_, _| {
            disabled_cb.set(true);
        });

        // Simulate dragging to the top.
        let top_pos = metrics_bottom.scroll_pos_for_drag(px(0.), px(0.));
        cx.update(|window, cx| {
            super::LogMinimap::apply_scroll_pos(
                &list_state,
                top_pos,
                metrics_bottom.max_scroll_y,
                &cb,
                window,
                cx,
            );
        });
        assert_eq!(list_state.logical_scroll_top().item_ix, 0);
        assert!(
            disabled.get(),
            "scrolling away from bottom disables auto-scroll"
        );

        // Simulate dragging back to the bottom.
        disabled.set(false);
        let bottom_pos = metrics_bottom.scroll_pos_for_drag(px(120.), px(0.));
        cx.update(|window, cx| {
            super::LogMinimap::apply_scroll_pos(
                &list_state,
                bottom_pos,
                metrics_bottom.max_scroll_y,
                &cb,
                window,
                cx,
            );
        });
        assert!(
            list_state.logical_scroll_top().item_ix > 30,
            "dragging to bottom should reach the end of the list"
        );
        assert!(
            !disabled.get(),
            "scrolling to bottom should not disable auto-scroll"
        );
    }
}
