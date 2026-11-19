use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::{
    ConfigStore, SAMPLE_INTERVAL_MS, SerialConfig, SessionCounters, ThroughputSample,
    ThroughputSpan, ThroughputSummary, format_bytes_compact, format_framing_short, format_rate,
    nice_y_ceiling, theoretical_max_bytes_per_sec,
};
use crate::ui::fonts::MONO_FONT;
use crate::ui::theme::NanoTheme;
use crate::ui::theme_manager::{ThemeHandle, ThemeManager};
use crate::ui::workspace::SashaApp;

const HEADER_HEIGHT: Pixels = px(34.);
const TOOLBAR_HEIGHT: Pixels = px(38.);
const FOOTER_HEIGHT: Pixels = px(28.);
const CHART_LEFT_PAD: Pixels = px(68.);
const CHART_RIGHT_PAD: Pixels = px(16.);
const CHART_TOP_PAD: Pixels = px(16.);
const CHART_BOTTOM_PAD: Pixels = px(24.);

/// Read-only snapshot of the active session's throughput state.
#[derive(Debug, Clone)]
pub struct ThroughputSnapshot {
    pub session_title: String,
    pub config: SerialConfig,
    pub counters: SessionCounters,
    pub samples: Vec<ThroughputSample>,
    pub summary: ThroughputSummary,
    pub theoretical_max: f64,
    pub sampled_at: Instant,
}

impl Default for ThroughputSnapshot {
    fn default() -> Self {
        let config = SerialConfig::default();
        let theoretical_max = theoretical_max_bytes_per_sec(&config);
        Self {
            session_title: config.port.clone(),
            config,
            counters: SessionCounters::default(),
            samples: Vec::new(),
            summary: ThroughputSummary::default(),
            theoretical_max,
            sampled_at: Instant::now(),
        }
    }
}

/// Independent throughput monitor window following the main window's active tab.
pub struct ThroughputWindow {
    sasha_app: WeakEntity<SashaApp>,
    config_store: Entity<ConfigStore>,
    theme_manager: Entity<ThemeManager>,
    span: ThroughputSpan,
    show_rx: bool,
    show_tx: bool,
    paused: bool,
    frozen_phase: f32,
    snapshot: ThroughputSnapshot,
    _theme_subscription: Subscription,
    _config_subscription: Subscription,
}

impl ThroughputWindow {
    pub fn new(
        sasha_app: WeakEntity<SashaApp>,
        config_store: Entity<ConfigStore>,
        initial_snapshot: ThroughputSnapshot,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme_manager = ThemeHandle::get(cx);
        let theme_subscription = cx.observe(&theme_manager, |_, _, cx| cx.notify());
        let config_subscription = cx.observe(&config_store, |_, _, cx| cx.notify());
        Self {
            sasha_app,
            config_store,
            theme_manager,
            span: ThroughputSpan::Minute1,
            show_rx: true,
            show_tx: true,
            paused: false,
            frozen_phase: 0.0,
            snapshot: initial_snapshot,
            _theme_subscription: theme_subscription,
            _config_subscription: config_subscription,
        }
    }

    fn current_phase(&self) -> f32 {
        if self.paused {
            return self.frozen_phase;
        }
        let elapsed = Instant::now()
            .saturating_duration_since(self.snapshot.sampled_at)
            .as_secs_f32();
        (elapsed / (SAMPLE_INTERVAL_MS as f32 / 1000.0)).clamp(0.0, 1.0)
    }

    pub fn span(&self) -> ThroughputSpan {
        self.span
    }

    /// Pushed by `SashaApp` on each sampling tick or active-tab switch.
    /// Avoids reading `SashaApp` re-entrantly while it is already being updated.
    pub fn apply_snapshot(
        &mut self,
        snapshot: ThroughputSnapshot,
        tab_changed: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.paused || tab_changed {
            self.snapshot = snapshot;
            cx.notify();
        }
    }

    fn pull_live_snapshot(&mut self, cx: &mut Context<Self>) {
        if let Some(app) = self.sasha_app.upgrade() {
            self.snapshot = app.read(cx).active_throughput_snapshot(self.span);
        }
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if self.paused {
            self.paused = false;
            self.pull_live_snapshot(cx);
        } else {
            self.frozen_phase = self.current_phase();
            self.paused = true;
        }
        cx.notify();
    }

    fn set_span(&mut self, span: ThroughputSpan, cx: &mut Context<Self>) {
        if self.span == span {
            return;
        }
        self.span = span;
        self.pull_live_snapshot(cx);
        cx.notify();
    }

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        if let Some(app) = self.sasha_app.upgrade() {
            app.update(cx, |app, _| app.clear_active_throughput());
            self.snapshot = app.read(cx).active_throughput_snapshot(self.span);
        }
        cx.notify();
    }

    fn series_pill(
        theme: NanoTheme,
        id: &'static str,
        dot_color: Hsla,
        label: SharedString,
        rate_text: String,
        active: bool,
        tooltip_text: SharedString,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(id)
            .items_center()
            .gap_1p5()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .opacity(if active { 1.0 } else { 0.45 })
            .cursor_pointer()
            .hover(move |s| s.bg(theme.bg_hover))
            .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_click(window, cx);
            })
            .child(div().size(px(7.)).rounded_full().bg(dot_color))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_secondary)
                    .child(label),
            )
            .child(
                div()
                    .font_family(MONO_FONT)
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_primary)
                    .child(rate_text),
            )
    }

    fn icon_action_button(
        theme: NanoTheme,
        id: &'static str,
        icon: LucideIcon,
        active: bool,
        tooltip_text: SharedString,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border_card)
            .bg(if active {
                theme.bg_toggle_active
            } else {
                theme.bg_card
            })
            .text_color(if active {
                theme.text_primary
            } else {
                theme.text_secondary
            })
            .cursor_pointer()
            .hover(move |s| {
                s.bg(theme.hover_over(theme.bg_card))
                    .text_color(theme.text_primary)
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_click(window, cx);
            })
            .child(Icon::new(icon).size(px(13.)))
    }
}

impl Render for ThroughputWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme_manager.read(cx).nano();
        let locale = self.config_store.read(cx).config().locale;
        let snapshot = self.snapshot.clone();
        let phase = self.current_phase();
        if !self.paused {
            window.request_animation_frame();
        }

        let title_prefix = tr_shared(locale, "throughput.title");
        let header_title = format!("{} · {}", title_prefix, snapshot.session_title);

        let max_current = snapshot.summary.current_rx.max(snapshot.summary.current_tx);
        let utilization_pct = if snapshot.theoretical_max > 0.0 {
            ((max_current / snapshot.theoretical_max) * 100.0).clamp(0.0, 999.0)
        } else {
            0.0
        };

        let weak_rx = cx.entity().downgrade();
        let rx_pill = Self::series_pill(
            theme,
            "throughput-pill-rx",
            theme.rx_green,
            tr_shared(locale, "throughput.rx"),
            format_rate(snapshot.summary.current_rx),
            self.show_rx,
            tr_shared(locale, "throughput.toggle_series_hint"),
            move |_, cx| {
                let _ = weak_rx.update(cx, |this, cx| {
                    this.show_rx = !this.show_rx;
                    cx.notify();
                });
            },
        );

        let weak_tx = cx.entity().downgrade();
        let tx_pill = Self::series_pill(
            theme,
            "throughput-pill-tx",
            theme.tx_blue,
            tr_shared(locale, "throughput.tx"),
            format_rate(snapshot.summary.current_tx),
            self.show_tx,
            tr_shared(locale, "throughput.toggle_series_hint"),
            move |_, cx| {
                let _ = weak_tx.update(cx, |this, cx| {
                    this.show_tx = !this.show_tx;
                    cx.notify();
                });
            },
        );

        let span_buttons = ThroughputSpan::ALL.into_iter().map(|span| {
            let active = self.span == span;
            div()
                .id(ElementId::Name(format!("span-{}", span.label()).into()))
                .h_full()
                .px_2p5()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_family(MONO_FONT)
                .when(active, |this| {
                    this.bg(theme.bg_toggle_active)
                        .text_color(theme.accent_blue_hover)
                })
                .when(!active, |this| {
                    this.text_color(theme.text_secondary)
                        .cursor_pointer()
                        .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_span(span, cx);
                    }),
                )
                .child(span.label())
        });

        let pause_tip = if self.paused {
            tr_shared(locale, "throughput.resume")
        } else {
            tr_shared(locale, "throughput.pause")
        };
        let pause_icon = if self.paused {
            LucideIcon::Play
        } else {
            LucideIcon::Pause
        };
        let weak_pause = cx.entity().downgrade();
        let pause_btn = Self::icon_action_button(
            theme,
            "throughput-pause-btn",
            pause_icon,
            self.paused,
            pause_tip,
            move |_, cx| {
                let _ = weak_pause.update(cx, |this, cx| this.toggle_pause(cx));
            },
        );

        let weak_clear = cx.entity().downgrade();
        let clear_btn = Self::icon_action_button(
            theme,
            "throughput-clear-btn",
            LucideIcon::Trash,
            false,
            tr_shared(locale, "throughput.clear"),
            move |_, cx| {
                let _ = weak_clear.update(cx, |this, cx| this.clear_history(cx));
            },
        );

        let footer_left = format!(
            "{}  RX {} ({})  ·  TX {} ({})",
            tr_shared(locale, "throughput.peak"),
            format_rate(snapshot.summary.peak_rx),
            format_bytes_compact(snapshot.counters.rx_bytes),
            format_rate(snapshot.summary.peak_tx),
            format_bytes_compact(snapshot.counters.tx_bytes),
        );
        let footer_right = format!(
            "{} {}  ·  {utilization_pct:.1}% / {} {}",
            snapshot.config.baud_rate,
            format_framing_short(&snapshot.config),
            tr_shared(locale, "throughput.theoretical_max"),
            format_rate(snapshot.theoretical_max),
        );

        let chart = ThroughputChart {
            id: ElementId::Name("throughput-chart".into()),
            theme,
            locale,
            span: self.span,
            samples: Rc::new(snapshot.samples),
            phase,
            show_rx: self.show_rx,
            show_tx: self.show_tx,
        };

        v_flex()
            .id("throughput-window-root")
            .size_full()
            .bg(theme.bg_canvas)
            .text_color(theme.text_primary)
            .on_key_down(|event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    window.remove_window();
                }
            })
            // Native TitleBar overlay with right-aligned window controls.
            .child(
                div()
                    .id("throughput-header")
                    .relative()
                    .flex_shrink_0()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .bg(theme.bg_strip)
                    .border_b_1()
                    .border_color(theme.border_card)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(HEADER_HEIGHT)
                            .child(
                                TitleBar::new()
                                    .bg(gpui_kit::transparent_black())
                                    .border_b_0(),
                            ),
                    )
                    .child(
                        h_flex()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right(px(140.))
                            .h_full()
                            .pl_3()
                            .items_center()
                            .gap_2()
                            .window_control_area(WindowControlArea::Drag)
                            .child(
                                Icon::new(LucideIcon::Activity)
                                    .size(px(14.))
                                    .text_color(theme.accent_blue),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(header_title),
                            )
                            .when(self.paused, |this| {
                                this.child(
                                    div()
                                        .px_1p5()
                                        .py(px(1.))
                                        .rounded(px(4.))
                                        .bg(theme.bg_toggle_active)
                                        .text_xs()
                                        .text_color(theme.text_secondary)
                                        .child(tr_shared(locale, "throughput.paused_badge")),
                                )
                            }),
                    ),
            )
            // Single compact toolbar row: RX/TX legend pills on left, span + actions on right.
            .child(
                h_flex()
                    .id("throughput-toolbar")
                    .flex_shrink_0()
                    .w_full()
                    .h(TOOLBAR_HEIGHT)
                    .px_3()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border_card)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(rx_pill)
                            .child(tx_pill),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .h(px(26.))
                                    .items_center()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(theme.border_card)
                                    .overflow_hidden()
                                    .children(span_buttons),
                            )
                            .child(pause_btn)
                            .child(clear_btn),
                    ),
            )
            // Main chart canvas occupying ~75% of the window.
            .child(div().flex_1().min_h_0().w_full().child(chart))
            // Thin status footer mirroring the main window's BottomBar.
            .child(
                h_flex()
                    .id("throughput-footer")
                    .flex_shrink_0()
                    .w_full()
                    .h(FOOTER_HEIGHT)
                    .px_3()
                    .items_center()
                    .justify_between()
                    .bg(theme.bg_strip)
                    .border_t_1()
                    .border_color(theme.border_card)
                    .font_family(MONO_FONT)
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(footer_left)
                    .child(footer_right),
            )
    }
}

#[derive(Default)]
pub struct ChartHoverState {
    hover_x: Option<Pixels>,
}

struct ThroughputChart {
    id: ElementId,
    theme: NanoTheme,
    locale: Locale,
    span: ThroughputSpan,
    samples: Rc<Vec<ThroughputSample>>,
    phase: f32,
    show_rx: bool,
    show_tx: bool,
}

impl IntoElement for ThroughputChart {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl ThroughputChart {
    fn plot_bounds(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        let left = bounds.left() + CHART_LEFT_PAD;
        let top = bounds.top() + CHART_TOP_PAD;
        let width = (bounds.size.width - CHART_LEFT_PAD - CHART_RIGHT_PAD).max(px(1.));
        let height = (bounds.size.height - CHART_TOP_PAD - CHART_BOTTOM_PAD).max(px(1.));
        Bounds {
            origin: point(left, top),
            size: size(width, height),
        }
    }

    fn paint_text(
        text: impl Into<SharedString>,
        origin: Point<Pixels>,
        font_size: Pixels,
        color: Hsla,
        window: &mut Window,
        cx: &mut App,
    ) {
        let text = text.into();
        let font = Font {
            family: MONO_FONT.into(),
            ..Default::default()
        };
        let run = TextRun {
            len: text.len(),
            font,
            color,
            ..Default::default()
        };
        let shaped = window
            .text_system()
            .shape_line(text, font_size, &[run], None);
        let _ = shaped.paint(origin, font_size * 1.25, TextAlign::Left, None, window, cx);
    }

    fn paint_series(
        plot: Bounds<Pixels>,
        samples: &[ThroughputSample],
        phase: f32,
        y_max: f64,
        color: Hsla,
        extract: impl Fn(&ThroughputSample) -> f64,
        window: &mut Window,
    ) {
        let n = samples.len();
        if n < 2 || y_max <= 0.0 {
            return;
        }
        let intervals = (n - 1) as f32;
        let step_x = plot.size.width / intervals;
        let point_at = |i: usize| -> Point<Pixels> {
            let x = plot.left() + step_x * (i as f32 - phase);
            let ratio = (extract(&samples[i]) / y_max).clamp(0.0, 1.0) as f32;
            let y = plot.bottom() - plot.size.height * ratio;
            point(x, y)
        };

        let first = point_at(0);
        let last = point_at(n - 1);
        let tail_end = point(plot.right(), last.y);

        // Soft translucent area under the curve.
        let mut fill = PathBuilder::fill();
        fill.move_to(point(first.x, plot.bottom()));
        fill.line_to(first);
        for i in 1..n {
            fill.line_to(point_at(i));
        }
        fill.line_to(tail_end);
        fill.line_to(point(plot.right(), plot.bottom()));
        fill.close();
        if let Ok(path) = fill.build() {
            window.paint_path(path, color.alpha(0.14));
        }

        // Crisp stroke on top.
        let mut stroke = PathBuilder::stroke(px(1.75));
        stroke.move_to(first);
        for i in 1..n {
            stroke.line_to(point_at(i));
        }
        stroke.line_to(tail_end);
        if let Ok(path) = stroke.build() {
            window.paint_path(path, color);
        }
    }
}

impl Element for ThroughputChart {
    type RequestLayoutState = Rc<RefCell<ChartHoverState>>;
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
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        let layout_id = window.request_layout(style, [], cx);

        let hover = match id {
            Some(global_id) => window.with_element_state(
                global_id,
                |retained: Option<Rc<RefCell<ChartHoverState>>>, _| {
                    let state = retained.unwrap_or_default();
                    (state.clone(), state)
                },
            ),
            None => Rc::new(RefCell::new(ChartHoverState::default())),
        };
        (layout_id, hover)
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
        hover_state: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let plot = Self::plot_bounds(bounds);
        let samples = self.samples.as_slice();

        let mut visible_peak = 0.0_f64;
        for s in samples {
            if self.show_rx {
                visible_peak = visible_peak.max(s.rx_bytes_per_sec);
            }
            if self.show_tx {
                visible_peak = visible_peak.max(s.tx_bytes_per_sec);
            }
        }
        let y_max = nice_y_ceiling(visible_peak);

        // Horizontal grid lines + Y-axis labels (0%, 25%, 50%, 75%, 100%).
        let grid_color = self.theme.border_card.alpha(0.65);
        let label_size = px(10.5);
        for step in 0..=4 {
            let ratio = step as f32 / 4.0;
            let y = plot.bottom() - plot.size.height * ratio;
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: point(plot.left(), y),
                    size: size(plot.size.width, px(1.)),
                },
                background: grid_color.into(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });

            let val = y_max * ratio as f64;
            Self::paint_text(
                format_rate(val),
                point(bounds.left() + px(10.), y - px(6.)),
                label_size,
                self.theme.text_muted,
                window,
                cx,
            );
        }

        // X-axis time labels (left, middle, right).
        let span_secs = self.span.seconds();
        let x_y = plot.bottom() + px(6.);
        Self::paint_text(
            format!("-{span_secs}s"),
            point(plot.left(), x_y),
            label_size,
            self.theme.text_muted,
            window,
            cx,
        );
        Self::paint_text(
            format!("-{}s", span_secs / 2),
            point(plot.left() + plot.size.width * 0.5 - px(14.), x_y),
            label_size,
            self.theme.text_muted,
            window,
            cx,
        );
        Self::paint_text(
            tr_shared(self.locale, "throughput.now"),
            point(plot.right() - px(24.), x_y),
            label_size,
            self.theme.text_muted,
            window,
            cx,
        );

        // Curves clipped cleanly to the plot area for seamless 60fps scrolling.
        let clip_bounds = Bounds {
            origin: point(plot.left(), plot.top() - px(2.)),
            size: size(plot.size.width, plot.size.height + px(4.)),
        };
        window.with_content_mask(
            Some(ContentMask {
                bounds: clip_bounds,
            }),
            |window| {
                if self.show_rx {
                    Self::paint_series(
                        plot,
                        samples,
                        self.phase,
                        y_max,
                        self.theme.rx_green,
                        |s| s.rx_bytes_per_sec,
                        window,
                    );
                }
                if self.show_tx {
                    Self::paint_series(
                        plot,
                        samples,
                        self.phase,
                        y_max,
                        self.theme.tx_blue,
                        |s| s.tx_bytes_per_sec,
                        window,
                    );
                }
            },
        );

        // Crosshair readout when hovering over the plot area.
        let hover_x = hover_state.borrow().hover_x;
        if let Some(hx) = hover_x
            && samples.len() >= 2
            && hx >= plot.left()
            && hx <= plot.right()
        {
            let intervals = (samples.len() - 1) as f32;
            let step_x = plot.size.width / intervals;
            let raw_idx = ((hx - plot.left()) / step_x + self.phase).round();
            let idx = (raw_idx.clamp(0.0, intervals) as usize).min(samples.len() - 1);
            let snap_x =
                (plot.left() + step_x * (idx as f32 - self.phase)).clamp(plot.left(), plot.right());
            let sample = samples[idx];

            // Vertical crosshair line.
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: point(snap_x, plot.top()),
                    size: size(px(1.), plot.size.height),
                },
                background: self.theme.text_muted.alpha(0.55).into(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });

            // Highlight dots on visible curves.
            let paint_dot = |val: f64, color: Hsla, window: &mut Window| {
                let r = (val / y_max).clamp(0.0, 1.0) as f32;
                let y = plot.bottom() - plot.size.height * r;
                window.paint_quad(PaintQuad {
                    bounds: Bounds {
                        origin: point(snap_x - px(3.5), y - px(3.5)),
                        size: size(px(7.), px(7.)),
                    },
                    background: color.into(),
                    corner_radii: Corners::all(px(3.5)),
                    border_widths: Edges::all(px(1.5)),
                    border_color: self.theme.bg_canvas,
                    border_style: BorderStyle::default(),
                });
            };
            if self.show_rx {
                paint_dot(sample.rx_bytes_per_sec, self.theme.rx_green, window);
            }
            if self.show_tx {
                paint_dot(sample.tx_bytes_per_sec, self.theme.tx_blue, window);
            }

            // Floating readout card anchored inside the plot, avoiding the pointer side.
            let samples_ago = (samples.len() - 1).saturating_sub(idx);
            let secs_ago = samples_ago as f64 * (SAMPLE_INTERVAL_MS as f64 / 1000.0);
            let time_label = if samples_ago == 0 {
                tr_shared(self.locale, "throughput.now").to_string()
            } else {
                format!("-{secs_ago:.1}s")
            };

            let card_w = px(134.);
            let card_h = px(56.);
            let card_x = if snap_x + px(10.) + card_w <= plot.right() {
                snap_x + px(10.)
            } else {
                (snap_x - px(10.) - card_w).max(plot.left() + px(4.))
            };
            let card_y = plot.top() + px(6.);
            window.paint_quad(PaintQuad {
                bounds: Bounds {
                    origin: point(card_x, card_y),
                    size: size(card_w, card_h),
                },
                background: self.theme.bg_popover.alpha(0.94).into(),
                corner_radii: Corners::all(px(6.)),
                border_widths: Edges::all(px(1.)),
                border_color: self.theme.border_popover,
                border_style: BorderStyle::default(),
            });

            let text_x = card_x + px(8.);
            Self::paint_text(
                time_label,
                point(text_x, card_y + px(5.)),
                px(10.5),
                self.theme.text_muted,
                window,
                cx,
            );
            Self::paint_text(
                format!("RX {}", format_rate(sample.rx_bytes_per_sec)),
                point(text_x, card_y + px(21.)),
                px(11.),
                self.theme.rx_green,
                window,
                cx,
            );
            Self::paint_text(
                format!("TX {}", format_rate(sample.tx_bytes_per_sec)),
                point(text_x, card_y + px(37.)),
                px(11.),
                self.theme.tx_blue,
                window,
                cx,
            );
        }

        // Track pointer movement for the crosshair.
        let hover_state = hover_state.clone();
        let hitbox = hitbox.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let next_x = if hitbox.is_hovered(window)
                && event.position.x >= plot.left()
                && event.position.x <= plot.right()
                && event.position.y >= plot.top()
                && event.position.y <= plot.bottom()
            {
                Some(event.position.x)
            } else {
                None
            };
            let mut state = hover_state.borrow_mut();
            if state.hover_x != next_x {
                state.hover_x = next_x;
                window.refresh();
            }
        });
    }
}
