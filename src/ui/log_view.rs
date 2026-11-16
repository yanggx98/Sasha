use std::ops::Range;

use gpui_kit::base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::{LogDirection, LogEntry, ReceiveViewState, selectable_log_line};
use crate::ui::theme::NanoTheme;

/// Scrollable receive/transmit log area.
/// Occupies `flex_1` above the floating send card so the card never occludes logs.
#[derive(IntoElement)]
pub struct LogView {
    theme: NanoTheme,
    logs: Vec<LogEntry>,
    view_state: ReceiveViewState,
    simulated_notice: &'static str,
    focus: FocusHandle,
}

impl LogView {
    pub fn new(
        theme: NanoTheme,
        logs: Vec<LogEntry>,
        view_state: ReceiveViewState,
        simulated_notice: &'static str,
        focus: FocusHandle,
    ) -> Self {
        Self {
            theme,
            logs,
            view_state,
            simulated_notice,
            focus,
        }
    }
}

impl RenderOnce for LogView {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let show_timestamp = self.view_state.show_timestamp;
        let hex_mode = self.view_state.hex_mode;
        let focus = self.focus;

        div()
            .id("log-scroll-container")
            .track_focus(&focus)
            .tab_stop(false)
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .px_3()
            .pb_2()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                focus.focus(window, cx);
            })
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .font_family("Consolas")
                    .text_sm()
                    .text_color(theme.text_primary)
                    .children(self.logs.into_iter().enumerate().map(move |(index, entry)| {
                        let tag_color = match entry.direction {
                            LogDirection::Rx => theme.rx_green,
                            LogDirection::Tx => theme.tx_blue,
                            LogDirection::System => theme.text_muted,
                        };
                        let line = selectable_log_line(&entry, show_timestamp, hex_mode);
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

                        h_flex()
                            .w_full()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div().flex_1().min_w_0().child(
                                    LogLineText::new(
                                        ElementId::named_usize("log-line", entry.id as usize),
                                        line.text,
                                        highlights,
                                        index as u64,
                                        rgba(0x2563EB99).into(),
                                    ),
                                ),
                            )
                            .when(simulated, |this| {
                                this.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(theme.text_muted)
                                        .child("[模拟]"),
                                )
                            })
                    }))
                    .child(
                        div()
                            .pt_3()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(format!("* {}", self.simulated_notice)),
                    ),
            )
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
    document_order: u64,
    selection_color: Hsla,
    styled: StyledText,
}

impl LogLineText {
    fn new(
        id: ElementId,
        text: impl Into<SharedString>,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
        document_order: u64,
        selection_color: Hsla,
    ) -> Self {
        let text = text.into();
        let styled = StyledText::new(text.clone()).with_highlights(highlights.clone());
        Self {
            id,
            text,
            highlights,
            document_order,
            selection_color,
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
        let (layout_id, ()) = self
            .styled
            .request_layout(id, inspector_id, window, cx);
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
        let projection = handle.update_runs(
            &[TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)
                .with_document_order(self.document_order)],
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
