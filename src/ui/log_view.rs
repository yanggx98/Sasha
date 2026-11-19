use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::{
    LogDirection, LogEntry, LogMarkers, MarkerColor, ReceiveViewState, SearchMatch,
    minimap_fraction, selectable_log_line, visible_marker_positions,
};
use crate::ui::log_minimap::{DisableAutoScrollCallback, JumpCallback, LogMinimap, MinimapMarker};
use crate::ui::marker_editor::marker_color_hsla;
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::search_highlight_colors;

/// Called with a log entry id and the window position when a row is right-clicked.
pub type MarkCallback = Rc<dyn Fn(u64, Point<Pixels>, &mut Window, &mut App)>;

/// Scrollable receive/transmit log area.
/// Occupies `flex_1` above the floating send card so the card never occludes logs.
#[derive(IntoElement)]
pub struct LogView {
    theme: NanoTheme,
    locale: Locale,
    logs: Vec<LogEntry>,
    view_state: ReceiveViewState,
    simulated_notice: SharedString,
    log_font_size: u16,
    focus: FocusHandle,
    list_state: ListState,
    search_matches: Rc<Vec<SearchMatch>>,
    active_match: Option<usize>,
    markers: Rc<LogMarkers>,
    on_mark: MarkCallback,
    on_jump: JumpCallback,
    on_disable_auto_scroll: DisableAutoScrollCallback,
}

impl LogView {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        locale: Locale,
        logs: Vec<LogEntry>,
        view_state: ReceiveViewState,
        simulated_notice: SharedString,
        log_font_size: u16,
        focus: FocusHandle,
        list_state: ListState,
        search_matches: Rc<Vec<SearchMatch>>,
        active_match: Option<usize>,
        markers: Rc<LogMarkers>,
        on_mark: MarkCallback,
        on_jump: JumpCallback,
        on_disable_auto_scroll: DisableAutoScrollCallback,
    ) -> Self {
        Self {
            theme,
            locale,
            logs,
            view_state,
            simulated_notice,
            log_font_size,
            focus,
            list_state,
            search_matches,
            active_match,
            markers,
            on_mark,
            on_jump,
            on_disable_auto_scroll,
        }
    }
}

impl RenderOnce for LogView {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let sim_badge_text = tr_shared(self.locale, "status.log_sim_badge");
        let show_timestamp = self.view_state.show_timestamp;
        let hex_mode = self.view_state.hex_mode;
        let log_font_size = self.log_font_size;
        let focus = self.focus;
        let list_state = self.list_state;
        let row_ids: Vec<u64> = self.logs.iter().map(|entry| entry.id).collect();
        let logs = Rc::new(self.logs);
        let simulated_notice = self.simulated_notice;
        let search_matches = self.search_matches;
        let active_match = self.active_match;
        let markers = self.markers;
        let on_mark = self.on_mark;
        let total = row_ids.len();
        let minimap_markers: Vec<MinimapMarker> = visible_marker_positions(&row_ids, &markers)
            .into_iter()
            .map(|(index, entry_id)| {
                let color = markers
                    .get(entry_id)
                    .map(|marker| marker.color)
                    .unwrap_or(MarkerColor::Red);
                MinimapMarker {
                    entry_id,
                    fraction: minimap_fraction(index, total),
                    color,
                }
            })
            .collect();
        let minimap = LogMinimap::new(
            theme,
            list_state.clone(),
            minimap_markers,
            log_row_height_hint(log_font_size),
            self.on_jump,
            self.on_disable_auto_scroll,
        );

        h_flex()
            .id("log-area")
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .id("log-scroll-container")
                    .relative()
                    .track_focus(&focus)
                    .tab_stop(false)
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        focus.focus(window, cx);
                    })
                    .child(
                        list(list_state.clone(), move |index, _, _| {
                            if let Some(entry) = logs.get(index) {
                                let tag_color = match entry.direction {
                                    LogDirection::Rx => theme.rx_green,
                                    LogDirection::Tx => theme.tx_blue,
                                    LogDirection::System => theme.text_muted,
                                };
                                let line = selectable_log_line(entry, show_timestamp, hex_mode);
                                let mut highlights = Vec::new();
                                if let Some(timestamp) = line.timestamp_bytes.clone() {
                                    highlights.push((timestamp, color_highlight(theme.text_muted)));
                                }
                                highlights.push((
                                    line.tag_bytes.clone(),
                                    HighlightStyle {
                                        color: Some(tag_color),
                                        font_weight: Some(FontWeight::MEDIUM),
                                        ..Default::default()
                                    },
                                ));
                                let simulated = entry.is_simulated;
                                let start = search_matches.partition_point(|m| m.log_index < index);
                                let mut normal_search_ranges = Vec::new();
                                let mut active_search_ranges = Vec::new();
                                for (offset, m) in search_matches[start..].iter().enumerate() {
                                    if m.log_index != index {
                                        break;
                                    }
                                    if active_match == Some(start + offset) {
                                        active_search_ranges.push(m.byte_range.clone());
                                    } else {
                                        normal_search_ranges.push(m.byte_range.clone());
                                    }
                                }

                                let marker_color = markers.get(entry.id).map(|marker| marker.color);
                                let entry_id = entry.id;
                                let on_mark = on_mark.clone();
                                let hover_bg = marker_color
                                    .map(|color| marker_color_hsla(color, 0.30))
                                    .unwrap_or(theme.bg_hover);

                                h_flex()
                                    .id(ElementId::named_usize("log-row", entry_id as usize))
                                    .w_full()
                                    .items_baseline()
                                    .gap_2()
                                    .pl(px(10.))
                                    .pr_2()
                                    .when(index == 0, |this| this.pt_2())
                                    .pb_1()
                                    // The marker bar sits in the left padding so text never shifts.
                                    .border_l_2()
                                    .border_color(
                                        marker_color
                                            .map(|color| marker_color_hsla(color, 1.0))
                                            .unwrap_or_else(gpui_kit::transparent_black),
                                    )
                                    .when_some(marker_color, |this, color| {
                                        this.bg(marker_color_hsla(color, 0.18))
                                    })
                                    // Like a button: the row under the pointer lights up, so it
                                    // is clear which entry a right-click will mark.
                                    .hover(move |style| style.bg(hover_bg))
                                    .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                        on_mark(entry_id, event.position, window, cx);
                                    })
                                    .font_family(crate::ui::fonts::MONO_FONT)
                                    .text_size(px(log_font_size as f32))
                                    .text_color(theme.text_primary)
                                    .child(div().flex_1().min_w_0().child(LogLineText::new(
                                        ElementId::named_usize("log-line", entry.id as usize),
                                        line.text,
                                        highlights,
                                        normal_search_ranges,
                                        active_search_ranges,
                                        index as u64,
                                        theme.accent_blue.alpha(0.6),
                                        search_highlight_colors(theme.bg_canvas),
                                    )))
                                    .when(simulated, |this| {
                                        let badge = sim_badge_text.clone();
                                        this.child(
                                            div()
                                                .flex_shrink_0()
                                                .text_xs()
                                                .text_color(theme.text_muted)
                                                .child(badge),
                                        )
                                    })
                                    .into_any_element()
                            } else {
                                div()
                                    .pl_3()
                                    .pr_2()
                                    .pt_3()
                                    .pb_2()
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .child(format!("* {}", simulated_notice))
                                    .into_any_element()
                            }
                        })
                        .size_full(),
                    ),
            )
            .child(minimap)
    }
}

fn color_highlight(color: Hsla) -> HighlightStyle {
    HighlightStyle {
        color: Some(color),
        ..Default::default()
    }
}

/// Height of one single-line log row before it has been measured.
///
/// GPUI's default line height is φ times the font size. Each row also has
/// `pb_1` (4px). The list only measures the viewport plus a little overdraw,
/// so rows above that would otherwise contribute 0 and the scrollbar would
/// hit the top of its track while earlier logs were still above.
pub(crate) fn log_row_height_hint(font_size: u16) -> Pixels {
    // Mirrors GPUI's default line-height factor; keep the literal (not a std constant).
    #[allow(clippy::approx_constant)]
    let line_height = (font_size as f32 * 1.618_034).round();
    px(line_height + 4.)
}

/// One selectable log row. The string is the copyable line; `[模拟]` is not included.
struct LogLineText {
    id: ElementId,
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    normal_search_ranges: Vec<Range<usize>>,
    active_search_ranges: Vec<Range<usize>>,
    document_order: u64,
    selection_color: Hsla,
    /// Search match backgrounds `(normal, active)`.
    match_colors: (Hsla, Hsla),
    styled: StyledText,
}

impl LogLineText {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: ElementId,
        text: impl Into<SharedString>,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
        normal_search_ranges: Vec<Range<usize>>,
        active_search_ranges: Vec<Range<usize>>,
        document_order: u64,
        selection_color: Hsla,
        match_colors: (Hsla, Hsla),
    ) -> Self {
        let text = text.into();
        let styled = StyledText::new(text.clone()).with_highlights(highlights.clone());
        Self {
            id,
            text,
            highlights,
            normal_search_ranges,
            active_search_ranges,
            document_order,
            selection_color,
            match_colors,
            styled,
        }
    }

    fn paint_selection(layout: &TextLayout, range: Range<usize>, color: Hsla, window: &mut Window) {
        let (Some(start), Some(end)) = (
            layout.position_for_index(range.start),
            layout.position_for_index(range.end),
        ) else {
            return;
        };
        for bounds in selection_quad_bounds(start, end, layout.bounds(), layout.line_height()) {
            window.paint_quad(PaintQuad {
                bounds,
                background: color.into(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });
        }
    }
}

fn selection_quad_bounds(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )];
    }

    let mut quads = vec![Bounds::from_corners(
        start,
        Point::new(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), start.y + line_height),
            Point::new(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        Point::new(bounds.left(), end.y),
        Point::new(end.x, end.y + line_height),
    ));
    quads
}

impl IntoElement for LogLineText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LogLineText {
    type RequestLayoutState = TextSelectionHandle;
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
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.styled = StyledText::new(self.text.clone()).with_highlights(self.highlights.clone());
        let (layout_id, ()) = self.styled.request_layout(id, inspector_id, window, cx);
        let text = self.text.clone();
        let handle = match id {
            Some(global_id) => window.with_element_state(
                global_id,
                |retained: Option<TextSelectionHandle>, window| {
                    let handle = match retained {
                        Some(handle) => handle,
                        None => {
                            let handle = TextSelectionHandle::new(text.clone(), cx);
                            handle.refresh_window_on_change(window, cx).detach();
                            handle
                        }
                    };
                    (handle.clone(), handle)
                },
            ),
            None => TextSelectionHandle::new(text, cx),
        };
        (layout_id, handle)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_document_order(self.document_order)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(handle, window, cx);
        handle.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled.layout().clone();
        let (normal_match_bg, active_match_bg) = self.match_colors;
        for range in &self.normal_search_ranges {
            Self::paint_selection(&layout, range.clone(), normal_match_bg, window);
        }
        for range in &self.active_search_ranges {
            Self::paint_selection(&layout, range.clone(), active_match_bg, window);
        }
        let projection = handle.update_runs(
            &[
                TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)
                    .with_document_order(self.document_order),
            ],
            cx,
        );
        for range in projection.ranges().iter().flatten().cloned() {
            Self::paint_selection(&layout, range, self.selection_color, window);
        }
        self.styled
            .paint(id, inspector_id, bounds, &mut (), &mut (), window, cx);
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
    }
}

#[cfg(test)]
mod tests {
    use gpui::prelude::*;
    use gpui::{ListAlignment, ListState, Render, TestAppContext, div, list, point, px, size};

    use super::log_row_height_hint;

    #[test]
    fn row_height_hint_tracks_the_log_font_size() {
        assert_eq!(log_row_height_hint(14), px(27.));
        assert!(log_row_height_hint(24) > log_row_height_hint(10));
    }

    /// Rows the list has not measured must still occupy space. Otherwise
    /// dragging the scrollbar to the top lands on the first measured row.
    #[gpui::test]
    fn scrollbar_top_is_the_first_row_when_unseen_rows_have_a_height(cx: &mut TestAppContext) {
        let mut cx = cx.add_empty_window();
        let hinted =
            ListState::new(40, ListAlignment::Top, px(10.)).with_uniform_item_height(px(30.));
        let unhinted = ListState::new(40, ListAlignment::Top, px(10.));

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
            cx.draw(point(px(0.), px(0.)), size(px(100.), px(40.)), |_, cx| {
                cx.new(|_| TestView(state.clone())).into_any_element()
            });
        };

        // The first layout clears height hints because it has no previous
        // item width. Reapplying after that layout is what keeps them.
        hinted.scroll_to_end();
        draw(&hinted, &mut cx);
        let _ = hinted.clone().with_uniform_item_height(px(30.));
        draw(&hinted, &mut cx);
        hinted.set_offset_from_scrollbar(point(px(0.), px(0.)));

        unhinted.scroll_to_end();
        draw(&unhinted, &mut cx);
        draw(&unhinted, &mut cx);
        unhinted.set_offset_from_scrollbar(point(px(0.), px(0.)));

        assert_eq!(
            hinted.logical_scroll_top().item_ix,
            0,
            "a height hint makes the top of the scrollbar the first row"
        );
        assert_ne!(
            unhinted.logical_scroll_top().item_ix,
            0,
            "without a hint the scrollbar top stops at the first measured row"
        );
    }
}
