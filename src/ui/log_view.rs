use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::LogWrap;
use crate::model::ansi::{AnsiColor, AnsiStyle, indexed_rgb};
use crate::model::{
    LogDirection, LogEntry, LogMarkers, MarkerColor, ReceiveViewState, SearchMatch,
    minimap_fraction, selectable_log_line, visible_marker_positions,
};
use crate::ui::log_minimap::{DisableAutoScrollCallback, JumpCallback, LogMinimap, MinimapMarker};
use crate::ui::log_pointer::{LogSelectHandle, RowGeom, log_pointer_layer};
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
    select: LogSelectHandle,
    wrap: LogWrap,
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
        select: LogSelectHandle,
        wrap: LogWrap,
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
            select,
            wrap,
        }
    }
}

impl RenderOnce for LogView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
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
        let select = self.select;
        let break_anywhere = self.wrap == LogWrap::Anywhere;
        let pointer_layer = log_pointer_layer(
            select.clone(),
            list_state.clone(),
            self.on_disable_auto_scroll.clone(),
            window,
            cx,
        );
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
                                    LogDirection::System | LogDirection::ScriptInfo => {
                                        theme.text_muted
                                    }
                                    LogDirection::ScriptWarn => {
                                        marker_color_hsla(MarkerColor::Orange, 1.0)
                                    }
                                    LogDirection::ScriptError => theme.error_text,
                                    LogDirection::ScriptOutput => theme.script_tag,
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
                                for (range, style) in &line.payload_styles {
                                    highlights.push((range.clone(), ansi_highlight(style, &theme)));
                                }
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
                                    // Fill every line (break between any two characters) instead
                                    // of moving a long token, such as a path, whole to the next one.
                                    .when(break_anywhere, |this| this.whitespace_break_anywhere())
                                    .text_color(theme.text_primary)
                                    .child(div().flex_1().min_w_0().child(LogLineText::new(
                                        ElementId::named_usize("log-line", entry.id as usize),
                                        line.text,
                                        highlights,
                                        normal_search_ranges,
                                        active_search_ranges,
                                        entry.id,
                                        select.clone(),
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
                    )
                    // Above the rows so it sees every press, but it never intercepts input.
                    .child(pointer_layer),
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

/// xterm-style 16 colors for dark and light canvases.
const ANSI_DARK: [(u8, u8, u8); 16] = [
    (0x3b, 0x3b, 0x3b),
    (0xe0, 0x5a, 0x5a),
    (0x5f, 0xc2, 0x6a),
    (0xe0, 0xc0, 0x5a),
    (0x5a, 0x8f, 0xe8),
    (0xc0, 0x6a, 0xd8),
    (0x4f, 0xc4, 0xcf),
    (0xd0, 0xd0, 0xd0),
    (0x7a, 0x7a, 0x7a),
    (0xff, 0x7b, 0x7b),
    (0x7d, 0xe0, 0x8a),
    (0xff, 0xdc, 0x7a),
    (0x7a, 0xae, 0xff),
    (0xe0, 0x8a, 0xf0),
    (0x7a, 0xe0, 0xe8),
    (0xff, 0xff, 0xff),
];

const ANSI_LIGHT: [(u8, u8, u8); 16] = [
    (0x20, 0x20, 0x20),
    (0xc6, 0x28, 0x28),
    (0x1e, 0x8a, 0x2e),
    (0x9a, 0x6b, 0x00),
    (0x1c, 0x5f, 0xd0),
    (0x93, 0x2a, 0xb0),
    (0x0b, 0x87, 0x96),
    (0x80, 0x80, 0x80),
    (0x55, 0x55, 0x55),
    (0xe0, 0x40, 0x40),
    (0x2a, 0xa8, 0x3e),
    (0xb8, 0x82, 0x00),
    (0x3a, 0x78, 0xe8),
    (0xb0, 0x44, 0xd0),
    (0x1a, 0xa5, 0xb5),
    (0xb0, 0xb0, 0xb0),
];

fn ansi_color(color: AnsiColor, palette: &[(u8, u8, u8); 16]) -> Hsla {
    let (r, g, b) = match color {
        AnsiColor::Indexed(index) => indexed_rgb(index, palette),
        AnsiColor::Rgb(r, g, b) => (r, g, b),
    };
    rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()
}

/// Converts an ANSI style into a text highlight, using a palette that suits the theme.
fn ansi_highlight(style: &AnsiStyle, theme: &NanoTheme) -> HighlightStyle {
    let palette = if theme.bg_canvas.l < 0.5 {
        &ANSI_DARK
    } else {
        &ANSI_LIGHT
    };
    let mut fg = style.fg.map(|c| ansi_color(c, palette));
    let mut bg = style.bg.map(|c| ansi_color(c, palette));
    if style.inverse {
        let (new_fg, new_bg) = (
            bg.unwrap_or(theme.bg_canvas.opacity(1.0)),
            fg.unwrap_or(theme.text_primary),
        );
        fg = Some(new_fg);
        bg = Some(new_bg);
    }
    if style.dim {
        fg = Some(fg.unwrap_or(theme.text_primary).alpha(0.6));
    }
    HighlightStyle {
        color: fg,
        background_color: bg,
        font_weight: style.bold.then_some(FontWeight::BOLD),
        font_style: style.italic.then_some(FontStyle::Italic),
        underline: style.underline.then(|| UnderlineStyle {
            thickness: px(1.),
            color: fg,
            wavy: false,
        }),
        strikethrough: style.strikethrough.then(|| StrikethroughStyle {
            thickness: px(1.),
            color: fg,
        }),
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
///
/// It does not talk to `gpui-base`: it reports where it was painted so the
/// pointer layer can map the mouse to a position in it, and draws its share of
/// the selection from the shared [`LogSelection`](crate::model::log_select::LogSelection).
struct LogLineText {
    id: ElementId,
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    normal_search_ranges: Vec<Range<usize>>,
    active_search_ranges: Vec<Range<usize>>,
    entry_id: u64,
    select: LogSelectHandle,
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
        entry_id: u64,
        select: LogSelectHandle,
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
            entry_id,
            select,
            selection_color,
            match_colors,
            styled,
        }
    }

    fn paint_selection(
        layout: &TextLayout,
        text: &str,
        range: Range<usize>,
        color: Hsla,
        window: &mut Window,
    ) {
        let (Some(start), Some(end)) = (
            layout.position_for_index(range.start),
            layout.position_for_index(range.end),
        ) else {
            return;
        };
        // The end of the first selected character shows which visual line it is on.
        let first_char_end = text
            .get(range.start..)
            .and_then(|rest| rest.chars().next())
            .and_then(|c| layout.position_for_index(range.start + c.len_utf8()));
        let start = soft_wrap_start(start, first_char_end, layout.bounds().left());
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

/// Where a highlight beginning at `start` should be drawn from.
///
/// A byte offset exactly at a soft wrap is both the end of the upper visual
/// line and the start of the next, and text layout reports the former. When the
/// first selected character is really on the next line (`first_char_end` is
/// below `start`), starting there would paint a block of empty space after the
/// upper line, so start at the left edge of the lower line instead.
fn soft_wrap_start(
    start: Point<Pixels>,
    first_char_end: Option<Point<Pixels>>,
    line_left: Pixels,
) -> Point<Pixels> {
    match first_char_end {
        Some(after) if after.y > start.y => Point::new(line_left, after.y),
        _ => start,
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
    type RequestLayoutState = ();
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
        self.styled.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled.layout().clone();
        let (normal_match_bg, active_match_bg) = self.match_colors;
        for range in &self.normal_search_ranges {
            Self::paint_selection(&layout, &self.text, range.clone(), normal_match_bg, window);
        }
        for range in &self.active_search_ranges {
            Self::paint_selection(&layout, &self.text, range.clone(), active_match_bg, window);
        }
        // Report where this row is so the pointer layer can find it. Rows paint
        // top to bottom, so the table stays in display order.
        self.select.rows.borrow_mut().push(RowGeom {
            entry_id: self.entry_id,
            bounds,
            layout: layout.clone(),
            text: self.text.clone(),
        });
        let selected = self
            .select
            .model
            .borrow()
            .row_range(self.entry_id, self.text.len());
        if let Some(range) = selected {
            Self::paint_selection(&layout, &self.text, range, self.selection_color, window);
        }
        self.styled
            .paint(id, inspector_id, bounds, &mut (), &mut (), window, cx);
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use gpui::prelude::*;
    use gpui::{
        App, FocusHandle, ListAlignment, ListState, Modifiers, MouseButton, Pixels, Point, Render,
        SharedString, StyledText, TestAppContext, TextLayout, VisualTestContext, Window, div, list,
        point, px, size,
    };

    use super::{LogView, log_row_height_hint, soft_wrap_start};
    use crate::i18n::Locale;
    use crate::model::{
        LogDirection, LogEntry, LogMarkers, LogWrap, ReceiveViewState, selectable_log_line,
    };
    use crate::ui::log_pointer::LogSelectHandle;
    use crate::ui::theme::NanoTheme;

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
    /// A view hosting the `LogView`: the pointer layer keeps state keyed to the
    /// view that renders it, as in the app.
    struct LogHost {
        logs: Vec<LogEntry>,
        select: LogSelectHandle,
        list_state: ListState,
        focus: FocusHandle,
    }

    impl Render for LogHost {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            // As in the app, the log fills the leftover height of a column.
            div().flex().flex_col().size_full().child(test_log_view(
                self.logs.clone(),
                &self.select,
                &self.list_state,
                self.focus.clone(),
            ))
        }
    }

    /// A `LogView` over `logs` with no-op callbacks, sharing `select` and `list_state`.
    fn test_log_view(
        logs: Vec<LogEntry>,
        select: &LogSelectHandle,
        list_state: &ListState,
        focus: FocusHandle,
    ) -> LogView {
        LogView::new(
            NanoTheme::default(),
            Locale::EnUs,
            logs,
            ReceiveViewState::default(),
            SharedString::default(),
            14,
            focus,
            list_state.clone(),
            Rc::new(Vec::new()),
            None,
            Rc::new(LogMarkers::default()),
            Rc::new(|_: u64, _: Point<Pixels>, _: &mut Window, _: &mut App| {}),
            Rc::new(|_: u64, _: &mut Window, _: &mut App| {}),
            Rc::new(|_: &mut Window, _: &mut App| {}),
            select.clone(),
            LogWrap::Anywhere,
        )
    }

    fn numbered_logs(count: u64) -> Vec<LogEntry> {
        (1..=count)
            .map(|id| {
                LogEntry::new(
                    id,
                    "00:00:00.000",
                    LogDirection::Rx,
                    format!("row {id}").into_bytes(),
                )
            })
            .collect()
    }

    /// Dragging past the bottom edge keeps scrolling, and the selection keeps
    /// growing across rows that are no longer rendered. (`gpui-base`'s own
    /// selection is dropped as soon as its start row stops rendering.)
    #[gpui::test]
    fn dragging_past_the_edge_scrolls_and_the_selection_survives_virtualization(
        cx: &mut TestAppContext,
    ) {
        let logs = numbered_logs(400);
        let list_state = ListState::new(logs.len(), ListAlignment::Top, px(200.))
            .with_uniform_item_height(px(24.));
        let select = LogSelectHandle::default();
        let (_host, cx) = cx.add_window_view(|_, cx| LogHost {
            logs: logs.clone(),
            select: select.clone(),
            list_state: list_state.clone(),
            focus: cx.focus_handle(),
        });
        cx.simulate_resize(size(px(600.), px(400.)));
        cx.run_until_parked();
        // The first layout drops height hints; reapply them as the app does.
        let _ = list_state.clone().with_uniform_item_height(px(24.));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let painted = select.rows.borrow().len();
        assert!(painted > 5, "rows should have been painted, got {painted}");
        let first = select.rows.borrow()[0].bounds;
        let last_visible_id = select.rows.borrow().last().unwrap().entry_id;
        assert!(last_visible_id < 60, "only a screenful is rendered");

        // Press on the first row (hover first, as a real pointer does), then hold
        // the pointer in the bottom edge zone.
        let on_first_row = point(first.left() + px(4.), first.top() + px(2.));
        cx.simulate_mouse_move(on_first_row, None, Modifiers::none());
        cx.simulate_mouse_down(on_first_row, MouseButton::Left, Modifiers::none());
        assert!(select.model.borrow().is_dragging());
        cx.simulate_mouse_move(
            point(px(100.), px(395.)),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        for _ in 0..150 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
        }

        let scrolled_to = list_state.logical_scroll_top().item_ix;
        assert!(
            scrolled_to > last_visible_id as usize,
            "the list kept scrolling past the first screen (top row {scrolled_to})"
        );
        let end = select.model.borrow().range().expect("selection kept").1;
        assert!(
            end.entry_id > last_visible_id,
            "the selection grew past the rows that were first rendered ({} <= {last_visible_id})",
            end.entry_id
        );

        // Releasing keeps it, and the text covers every row in between,
        // including the ones scrolled out of the rendered range.
        cx.simulate_mouse_up(
            point(px(100.), px(395.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert!(!select.model.borrow().is_dragging());
        let (start, end) = select.model.borrow().range().expect("selection kept");
        let rows = numbered_logs(400)
            .into_iter()
            .map(|entry| (entry.id, selectable_log_line(&entry, true, false).text));
        let text = select.model.borrow().selected_text(rows);
        let lines = text.lines().count() as u64;
        assert_eq!(lines, end.entry_id - start.entry_id + 1);
        assert!(text.contains(&format!("row {}", last_visible_id + 1)));
    }
    #[gpui::test]
    fn dragging_past_the_top_edge_scrolls_up_and_extends_the_selection(cx: &mut TestAppContext) {
        let logs = numbered_logs(400);
        let list_state = ListState::new(logs.len(), ListAlignment::Top, px(200.))
            .with_uniform_item_height(px(24.));
        let select = LogSelectHandle::default();
        let (_host, cx) = cx.add_window_view(|_, cx| LogHost {
            logs: logs.clone(),
            select: select.clone(),
            list_state: list_state.clone(),
            focus: cx.focus_handle(),
        });
        cx.simulate_resize(size(px(600.), px(400.)));
        cx.run_until_parked();
        let _ = list_state.clone().with_uniform_item_height(px(24.));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        // Start near the bottom of the log, as when following the newest rows.
        let max = list_state.max_offset_for_scrollbar().y;
        list_state.set_offset_from_scrollbar(point(px(0.), -max));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let (first_visible_id, last_row) = {
            let rows = select.rows.borrow();
            (rows[0].entry_id, rows.last().unwrap().bounds)
        };
        assert!(first_visible_id > 300, "scrolled to the end");

        let on_last_row = point(last_row.left() + px(4.), last_row.top() + px(2.));
        cx.simulate_mouse_move(on_last_row, None, Modifiers::none());
        cx.simulate_mouse_down(on_last_row, MouseButton::Left, Modifiers::none());
        // Above the top of the window: far beyond the edge zone.
        cx.simulate_mouse_move(
            point(px(100.), px(-40.)),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        for _ in 0..150 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
        }
        cx.simulate_mouse_up(
            point(px(100.), px(-40.)),
            MouseButton::Left,
            Modifiers::none(),
        );

        let (start, end) = select.model.borrow().range().expect("selection kept");
        assert!(
            start.entry_id < first_visible_id - 20,
            "extended upward past the first rendered row ({} vs {first_visible_id})",
            start.entry_id
        );
        assert!(end.entry_id > 300, "the anchor stayed at the bottom");
    }

    #[gpui::test]
    fn double_click_selects_a_word_and_keeps_it_after_release(cx: &mut TestAppContext) {
        let logs = numbered_logs(50);
        let list_state = ListState::new(logs.len(), ListAlignment::Top, px(200.))
            .with_uniform_item_height(px(24.));
        let select = LogSelectHandle::default();
        let (_host, cx) = cx.add_window_view(|_, cx| LogHost {
            logs: logs.clone(),
            select: select.clone(),
            list_state: list_state.clone(),
            focus: cx.focus_handle(),
        });
        cx.simulate_resize(size(px(600.), px(400.)));
        cx.run_until_parked();
        let first = select.rows.borrow()[0].bounds;
        let on_row = point(first.left() + px(2.), first.top() + px(2.));
        cx.simulate_mouse_move(on_row, None, Modifiers::none());
        cx.simulate_event(gpui::MouseDownEvent {
            position: on_row,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_mouse_up(on_row, MouseButton::Left, Modifiers::none());
        assert!(select.model.borrow().has_selection(), "a word was selected");
        // The click, wherever it landed in the row, selected within that one row.
        let (start, end) = select.model.borrow().range().unwrap();
        assert_eq!(start.entry_id, end.entry_id);
    }
    #[test]
    fn a_highlight_starting_at_a_soft_wrap_begins_on_the_lower_line() {
        let left = px(10.);
        // Offset at the wrap is reported at the end of the upper line (y = 20)...
        let at_wrap = point(px(520.), px(20.));
        // ...but its first character ends on the next line (y = 40).
        let lower = point(px(120.), px(40.));
        assert_eq!(
            soft_wrap_start(at_wrap, Some(lower), left),
            point(left, px(40.))
        );
    }

    #[test]
    fn a_highlight_within_one_line_keeps_its_start() {
        let left = px(10.);
        let start = point(px(60.), px(20.));
        let same_line = point(px(68.), px(20.));
        assert_eq!(soft_wrap_start(start, Some(same_line), left), start);
        // No next character (an empty or end-of-text range): unchanged.
        assert_eq!(soft_wrap_start(start, None, left), start);
    }
    /// Lays out `text` the way a log row does (a block of text wrapping at `width`)
    /// and returns its layout.
    fn lay_out(
        cx: &mut VisualTestContext,
        text: &str,
        width: Pixels,
        break_anywhere: bool,
    ) -> TextLayout {
        let styled = StyledText::new(text.to_string());
        let layout = styled.layout().clone();
        cx.draw(
            point(px(0.), px(0.)),
            size(px(4000.), px(400.)),
            move |_, _| {
                div()
                    .w(width)
                    .text_size(px(16.))
                    .when(break_anywhere, |this| this.whitespace_break_anywhere())
                    .child(styled)
                    .into_any_element()
            },
        );
        layout
    }

    /// A long token that fits a line on its own but not after a short word: the
    /// default moves it whole to the next line; `BreakAnywhere` fills the first line.
    #[gpui::test]
    fn break_anywhere_fills_the_line_instead_of_moving_the_token(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let probe = lay_out(cx, "mm", px(1000.), false);
        let em = probe.position_for_index(1).unwrap().x - probe.position_for_index(0).unwrap().x;
        assert!(em > px(0.));

        let token = 20;
        let text = format!("x {}", "y".repeat(token));
        let width = em * (token as f32 + 1.5);
        // The token's second character: the first one sits exactly at a wrap, where
        // `position_for_index` reports the end of the upper line.
        let second_y = text.find('y').unwrap() + 1;
        // The end of the text: the last character alone may start the next line.
        let last = text.len();

        let words = lay_out(cx, &text, width, false);
        let top = words.position_for_index(0).unwrap().y;
        assert!(
            words.position_for_index(second_y).unwrap().y > top,
            "word wrapping moves the whole token to the next line"
        );

        let anywhere = lay_out(cx, &text, width, true);
        let top = anywhere.position_for_index(0).unwrap().y;
        assert_eq!(
            anywhere.position_for_index(second_y).unwrap().y,
            top,
            "break-anywhere keeps the first characters of the token on the first line"
        );
        assert!(
            anywhere.position_for_index(last).unwrap().y > top,
            "and it still wraps once the line is full"
        );
    }
}
