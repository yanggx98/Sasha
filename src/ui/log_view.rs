use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_kit::component::h_flex;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::{LogDirection, LogEntry, ReceiveViewState, SearchMatch, selectable_log_line};
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::search_highlight_colors;

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
        let logs = Rc::new(self.logs);
        let simulated_notice = self.simulated_notice;
        let search_matches = self.search_matches;
        let active_match = self.active_match;

        div()
            .id("log-scroll-container")
            .relative()
            .track_focus(&focus)
            .tab_stop(false)
            .flex_1()
            .min_h_0()
            .w_full()
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

                        h_flex()
                            .w_full()
                            .items_baseline()
                            .gap_2()
                            .pl_3()
                            .pr_4()
                            .when(index == 0, |this| this.pt_2())
                            .pb_1()
                            .font_family("Consolas")
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
                            .pr_4()
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
            .child(Scrollbar::vertical(&list_state))
    }
}

fn color_highlight(color: Hsla) -> HighlightStyle {
    HighlightStyle {
        color: Some(color),
        ..Default::default()
    }
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
