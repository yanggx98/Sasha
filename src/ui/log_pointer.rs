//! Pointer handling for the log: selecting text by dragging and auto-scrolling
//! at the edges.
//!
//! The selection itself is [`LogSelection`]: entry ids and byte offsets that do
//! not depend on which rows are rendered, so it survives scrolling a
//! virtualized list any distance (`gpui-base`'s own selection is discarded as
//! soon as an end row stops rendering). Rows only report where they were
//! painted ([`RowGeom`]) and draw their share of the highlight; this layer maps
//! the pointer to a row and offset and drives the gesture.
//!
//! Auto-scroll moves the list directly from the pointer position rather than
//! synthesizing wheel events, which the window's overlays would swallow.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::*;

use crate::model::log_select::{LogPos, LogSelection};
use crate::ui::log_minimap::DisableAutoScrollCallback;

/// Auto-scroll starts this far inside the scroll region's edge.
const EDGE_ZONE: Pixels = px(24.);
/// Distance beyond the zone over which the speed ramps up to its maximum.
const OUTER_RAMP: Pixels = px(96.);
/// Pixels scrolled per tick at the zone's edge and at full speed.
const MIN_STEP: f32 = 6.;
const MAX_STEP: f32 = 44.;
/// Tick interval (~60 fps).
const TICK: Duration = Duration::from_millis(16);

/// Where one log row was painted this frame.
pub struct RowGeom {
    pub entry_id: u64,
    pub bounds: Bounds<Pixels>,
    pub layout: TextLayout,
    pub text: SharedString,
}

/// A tab's log selection together with the rows painted in the last frame.
/// Cloning shares both.
#[derive(Clone, Default)]
pub struct LogSelectHandle {
    pub model: Rc<RefCell<LogSelection>>,
    pub rows: Rc<RefCell<Vec<RowGeom>>>,
}

impl LogSelectHandle {
    /// Text of a painted row, for word and line boundaries.
    fn text_of(&self, entry_id: u64) -> Option<String> {
        self.rows
            .borrow()
            .iter()
            .find(|row| row.entry_id == entry_id)
            .map(|row| row.text.to_string())
    }

    /// Selection position under `point`, from the rows painted last frame.
    fn locate(&self, point: Point<Pixels>) -> Option<LogPos> {
        let rows = self.rows.borrow();
        let spans: Vec<(Pixels, Pixels)> = rows
            .iter()
            .map(|row| (row.bounds.top(), row.bounds.bottom()))
            .collect();
        let (ix, hit) = pick_row(&spans, point.y)?;
        let row = &rows[ix];
        let offset = match hit {
            RowHit::Above => 0,
            RowHit::Below => row.text.len(),
            RowHit::Inside => match row.layout.index_for_position(point) {
                Ok(offset) | Err(offset) => offset.min(row.text.len()),
            },
        };
        Some(LogPos::new(row.entry_id, offset))
    }

    fn begin(&self, point: Point<Pixels>, click_count: usize, extend: bool) -> bool {
        let Some(pos) = self.locate(point) else {
            return false;
        };
        self.model
            .borrow_mut()
            .begin(pos, click_count, extend, &|id| self.text_of(id));
        true
    }

    fn drag_to(&self, point: Point<Pixels>) {
        if let Some(pos) = self.locate(point) {
            self.model.borrow_mut().drag_to(pos, &|id| self.text_of(id));
        }
    }

    fn is_dragging(&self) -> bool {
        self.model.borrow().is_dragging()
    }
}

/// How a pointer height relates to the row picked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowHit {
    /// Above the row (only for the first row).
    Above,
    Inside,
    /// Below the row: in the gap after it, or past the last row.
    Below,
}

/// The row a pointer at height `y` belongs to. `spans` are the rows'
/// `(top, bottom)` in top-to-bottom order. Between two rows the nearer one wins.
pub(crate) fn pick_row(spans: &[(Pixels, Pixels)], y: Pixels) -> Option<(usize, RowHit)> {
    let (first_top, _) = *spans.first()?;
    if y < first_top {
        return Some((0, RowHit::Above));
    }
    let ix = spans.partition_point(|(top, _)| *top <= y) - 1;
    let (_, bottom) = spans[ix];
    if y < bottom {
        return Some((ix, RowHit::Inside));
    }
    match spans.get(ix + 1) {
        Some((next_top, _)) if *next_top - y < y - bottom => Some((ix + 1, RowHit::Above)),
        _ => Some((ix, RowHit::Below)),
    }
}

/// Pixels to scroll per tick for a pointer at `y` within `[top, bottom]`:
/// negative scrolls toward the top, positive toward the bottom, `None` when
/// the pointer is clear of both edges.
pub(crate) fn edge_scroll_delta(y: Pixels, top: Pixels, bottom: Pixels) -> Option<Pixels> {
    // Too short to have a calm middle: do not scroll at all.
    if bottom - top <= EDGE_ZONE * 2. {
        return None;
    }
    let up_trigger = top + EDGE_ZONE;
    let down_trigger = bottom - EDGE_ZONE;
    let ramp = EDGE_ZONE + OUTER_RAMP;
    let step = |distance: Pixels| {
        let t = (distance / ramp).clamp(0., 1.);
        MIN_STEP + t * (MAX_STEP - MIN_STEP)
    };
    if y < up_trigger {
        Some(px(-step(up_trigger - y)))
    } else if y > down_trigger {
        Some(px(step(y - down_trigger)))
    } else {
        None
    }
}

#[derive(Default)]
struct EdgeScrollState {
    pointer: Point<Pixels>,
    /// Scroll region of the log, refreshed every paint.
    top: Pixels,
    bottom: Pixels,
    select: Option<LogSelectHandle>,
    list_state: Option<ListState>,
    on_disable_auto_scroll: Option<DisableAutoScrollCallback>,
    window: Option<AnyWindowHandle>,
    /// Ticks while the pointer is at an edge; dropping it stops scrolling.
    task: Option<Task<()>>,
}

impl EdgeScrollState {
    fn stop(&mut self) {
        self.task = None;
    }

    fn delta(&self) -> Option<Pixels> {
        let dragging = self.select.as_ref().is_some_and(|s| s.is_dragging());
        if dragging {
            edge_scroll_delta(self.pointer.y, self.top, self.bottom)
        } else {
            None
        }
    }
}

/// Scrolls the list one step and re-evaluates the selection's free end under
/// the (possibly stationary) pointer.
fn scroll_step(
    delta: Pixels,
    pointer: Point<Pixels>,
    select: &LogSelectHandle,
    list_state: &ListState,
    on_disable_auto_scroll: &DisableAutoScrollCallback,
    window: &mut Window,
    cx: &mut App,
) {
    if !select.is_dragging() {
        return;
    }
    let max = list_state.max_offset_for_scrollbar().y.max(px(0.));
    if max <= px(0.) {
        return;
    }
    let current = (-list_state.scroll_px_offset_for_scrollbar().y).clamp(px(0.), max);
    let next = (current + delta).clamp(px(0.), max);
    if next == current {
        return;
    }
    list_state.set_offset_from_scrollbar(point(px(0.), -next));
    if next < max - px(1.) {
        on_disable_auto_scroll(window, cx);
    }
    // The rows moved under the pointer: the free end follows them (using last
    // frame's rows; the next tick sees the new ones).
    select.drag_to(pointer);
    window.refresh();
}

fn spawn_ticker(state: &mut EdgeScrollState, cx: &mut Context<EdgeScrollState>) {
    if state.task.is_some() {
        return;
    }
    state.task = Some(cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(TICK).await;
            let Ok(Some((delta, pointer, select, list_state, on_disable, window))) =
                this.update(cx, |state, _| {
                    Some((
                        state.delta()?,
                        state.pointer,
                        state.select.clone()?,
                        state.list_state.clone()?,
                        state.on_disable_auto_scroll.clone()?,
                        state.window?,
                    ))
                })
            else {
                // Released, left the edge, or the log went away: stop ticking.
                let _ = this.update(cx, |state, _| state.task = None);
                return;
            };
            let _ = cx.update_window(window, |_, window, cx| {
                scroll_step(
                    delta,
                    pointer,
                    &select,
                    &list_state,
                    &on_disable,
                    window,
                    cx,
                );
            });
        }
    }));
}

/// An invisible layer over the log area. It owns the selection gesture
/// (press, drag, release; double and triple click; Shift to extend) and
/// scrolls `list_state` while the pointer is held near the top or bottom edge.
pub fn log_pointer_layer(
    select: LogSelectHandle,
    list_state: ListState,
    on_disable_auto_scroll: DisableAutoScrollCallback,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let state = window.use_keyed_state("log-pointer", cx, |_, _| EdgeScrollState::default());
    let prepaint_select = select.clone();
    canvas(
        move |bounds, window, _| {
            // Rows paint after this and refill the table.
            prepaint_select.rows.borrow_mut().clear();
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        },
        move |bounds, hitbox, window, cx| {
            let handle = window.window_handle();
            state.update(cx, |s, _| {
                s.top = bounds.top();
                s.bottom = bounds.bottom();
                s.select = Some(select.clone());
                s.list_state = Some(list_state.clone());
                s.on_disable_auto_scroll = Some(on_disable_auto_scroll.clone());
                s.window = Some(handle);
            });

            // These never stop propagation, so the capture phase is safe and
            // cannot be pre-empted by other handlers.
            window.on_mouse_event({
                let (state, select) = (state.clone(), select.clone());
                move |event: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    if select.begin(event.position, event.click_count, event.modifiers.shift) {
                        state.update(cx, |s, _| {
                            s.pointer = event.position;
                            s.stop();
                        });
                        window.refresh();
                    }
                }
            });
            window.on_mouse_event({
                let (state, select) = (state.clone(), select.clone());
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !select.is_dragging() {
                        return;
                    }
                    // The release happened out of our sight (e.g. outside the window).
                    if event.pressed_button != Some(MouseButton::Left) {
                        select.model.borrow_mut().end();
                        state.update(cx, |s, _| s.stop());
                        window.refresh();
                        return;
                    }
                    select.drag_to(event.position);
                    state.update(cx, |s, cx| {
                        s.pointer = event.position;
                        if s.delta().is_some() {
                            spawn_ticker(s, cx);
                        } else {
                            s.stop();
                        }
                    });
                    window.refresh();
                }
            });
            window.on_mouse_event({
                let (state, select) = (state.clone(), select.clone());
                move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                        return;
                    }
                    let was_dragging = select.is_dragging();
                    select.model.borrow_mut().end();
                    state.update(cx, |s, _| s.stop());
                    if was_dragging {
                        window.refresh();
                    }
                }
            });
        },
    )
    // Without an explicit origin an absolute child sits at its static position,
    // i.e. below the list, outside the area it is meant to cover.
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    // Explicit imports: `gpui_kit::*` also exports a `test` macro that would
    // shadow the standard `#[test]`.
    use gpui_kit::{Pixels, px};

    use super::{EDGE_ZONE, MAX_STEP, MIN_STEP, RowHit, edge_scroll_delta, pick_row};

    const TOP: Pixels = px(100.);
    const BOTTOM: Pixels = px(500.);

    #[test]
    fn calm_middle_does_not_scroll() {
        assert_eq!(edge_scroll_delta(px(300.), TOP, BOTTOM), None);
        // Exactly on the trigger lines is still calm.
        assert_eq!(edge_scroll_delta(TOP + EDGE_ZONE, TOP, BOTTOM), None);
        assert_eq!(edge_scroll_delta(BOTTOM - EDGE_ZONE, TOP, BOTTOM), None);
    }

    #[test]
    fn near_edges_scroll_in_the_matching_direction() {
        assert!(edge_scroll_delta(px(110.), TOP, BOTTOM).unwrap() < px(0.));
        assert!(edge_scroll_delta(px(490.), TOP, BOTTOM).unwrap() > px(0.));
    }

    #[test]
    fn speed_grows_with_distance_and_is_capped() {
        let at = |y: f32| edge_scroll_delta(px(y), TOP, BOTTOM).unwrap().abs();
        assert!(at(110.) < at(90.));
        assert!(at(90.) < at(40.));
        // Far past the edge, and far past the window, share the cap.
        assert_eq!(at(-5000.), px(MAX_STEP));
        assert_eq!(
            edge_scroll_delta(px(9000.), TOP, BOTTOM),
            Some(px(MAX_STEP))
        );
        // Just inside the trigger zone starts near the minimum.
        let just_inside = TOP + EDGE_ZONE - px(0.5);
        assert!(edge_scroll_delta(just_inside, TOP, BOTTOM).unwrap().abs() < px(MIN_STEP + 1.));
    }

    #[test]
    fn a_region_too_short_for_two_zones_never_scrolls() {
        assert_eq!(edge_scroll_delta(px(0.), px(100.), px(140.)), None);
        assert_eq!(edge_scroll_delta(px(500.), px(100.), px(140.)), None);
    }

    fn spans() -> Vec<(Pixels, Pixels)> {
        // Three rows of height 20 with a 10px gap after each.
        vec![
            (px(100.), px(120.)),
            (px(130.), px(150.)),
            (px(160.), px(180.)),
        ]
    }

    #[test]
    fn a_pointer_inside_a_row_picks_it() {
        assert_eq!(pick_row(&spans(), px(100.)), Some((0, RowHit::Inside)));
        assert_eq!(pick_row(&spans(), px(145.)), Some((1, RowHit::Inside)));
        assert_eq!(pick_row(&spans(), px(179.)), Some((2, RowHit::Inside)));
    }

    #[test]
    fn above_the_first_row_and_below_the_last_clamp_to_them() {
        assert_eq!(pick_row(&spans(), px(-300.)), Some((0, RowHit::Above)));
        assert_eq!(pick_row(&spans(), px(99.)), Some((0, RowHit::Above)));
        assert_eq!(pick_row(&spans(), px(180.)), Some((2, RowHit::Below)));
        assert_eq!(pick_row(&spans(), px(9000.)), Some((2, RowHit::Below)));
    }

    #[test]
    fn a_pointer_in_a_gap_picks_the_nearer_row() {
        // Gap 120..130: nearer the end of row 0 below 125, the start of row 1 above.
        assert_eq!(pick_row(&spans(), px(122.)), Some((0, RowHit::Below)));
        assert_eq!(pick_row(&spans(), px(128.)), Some((1, RowHit::Above)));
        // Halfway: ties go to the row above.
        assert_eq!(pick_row(&spans(), px(125.)), Some((0, RowHit::Below)));
    }

    #[test]
    fn no_rows_means_no_pick() {
        assert_eq!(pick_row(&[], px(10.)), None);
    }
}
