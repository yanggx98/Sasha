//! Right-dock section listing every marker of the active session.

use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, marker_clear_confirm, tr_shared};
use crate::model::MarkerColor;
use crate::ui::marker_editor::marker_color_hsla;
use crate::ui::receive_toolbar::ToolbarActionCallback;
use crate::ui::script_panel::rail_button;
use crate::ui::theme::NanoTheme;
use crate::ui::theme_store::contrast_foreground;

pub type MarkerIdCallback = Rc<dyn Fn(u64, &mut Window, &mut App)>;
pub type MarkerEditCallback = Rc<dyn Fn(u64, Point<Pixels>, &mut Window, &mut App)>;

#[derive(Debug, Clone)]
pub struct MarkerPanelItem {
    pub entry_id: u64,
    pub name: String,
    pub color: MarkerColor,
    /// Timestamp and direction of the marked entry, e.g. `12:00:01.350 RX`.
    pub label: String,
    /// The entry is currently hidden by the log filter.
    pub filtered: bool,
}

#[derive(IntoElement)]
pub struct MarkerPanel {
    pub theme: NanoTheme,
    pub locale: Locale,
    pub items: Vec<MarkerPanelItem>,
    pub collapsed: bool,
    /// Icon-only dock: show just the rail button.
    pub compact: bool,
    pub on_expand_dock: ToolbarActionCallback,
    pub pending_clear: bool,
    pub on_toggle_section: ToolbarActionCallback,
    pub on_jump: MarkerIdCallback,
    pub on_edit: MarkerEditCallback,
    pub on_delete: MarkerIdCallback,
    pub on_request_clear: ToolbarActionCallback,
    pub on_confirm_clear: ToolbarActionCallback,
    pub on_cancel_clear: ToolbarActionCallback,
}

impl MarkerPanel {
    fn icon_button(
        theme: NanoTheme,
        id: impl Into<ElementId>,
        icon: LucideIcon,
        tooltip: SharedString,
        on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(22.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .cursor_pointer()
            .text_color(theme.text_muted)
            .hover(move |s| s.text_color(theme.text_primary).bg(theme.bg_hover))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                cx.stop_propagation();
                on_click(event, window, cx);
            })
            .child(Icon::new(icon).size(px(12.)))
    }

    fn text_button(
        theme: NanoTheme,
        id: &'static str,
        label: SharedString,
        primary: bool,
        on_click: ToolbarActionCallback,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(22.))
            .px_2()
            .flex()
            .items_center()
            .rounded(px(4.))
            .cursor_pointer()
            .text_xs()
            .when(primary, |this| {
                this.bg(theme.accent_blue)
                    .text_color(contrast_foreground(theme.accent_blue))
            })
            .when(!primary, |this| {
                this.border_1()
                    .border_color(theme.border_card)
                    .text_color(theme.text_secondary)
                    .hover(move |s| s.bg(theme.bg_hover))
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                on_click(window, cx);
            })
            .child(label)
    }
}

impl RenderOnce for MarkerPanel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let locale = self.locale;
        let count = self.items.len();

        if self.compact {
            return v_flex()
                .id("marker-panel")
                .w_full()
                .flex_none()
                .px_1p5()
                .bg(theme.bg_canvas)
                .occlude()
                .child(rail_button(
                    theme,
                    "marker-rail",
                    LucideIcon::Bookmark,
                    tr_shared(locale, "marker.panel_title"),
                    (count > 0).then_some(count),
                    self.on_expand_dock.clone(),
                ))
                .into_any_element();
        }

        let section_icon = if self.collapsed {
            LucideIcon::ChevronRight
        } else {
            LucideIcon::ChevronDown
        };

        let rows = self
            .items
            .iter()
            .map(|item| {
                let entry_id = item.entry_id;
                let filtered = item.filtered;
                let on_jump = self.on_jump.clone();
                let on_edit = self.on_edit.clone();
                let on_delete = self.on_delete.clone();
                let name: SharedString = item.name.clone().into();
                let label: SharedString = item.label.clone().into();
                let row_tooltip: Option<SharedString> =
                    filtered.then(|| tr_shared(locale, "marker.filtered_hint"));
                h_flex()
                    .id(("marker-row", entry_id as usize))
                    .w_full()
                    .h(px(28.))
                    .px_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(px(6.))
                    .when(!filtered, |this| {
                        this.cursor_pointer().hover(move |s| s.bg(theme.bg_hover))
                    })
                    .when_some(row_tooltip, |this, tip| {
                        this.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    })
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        if !filtered {
                            on_jump(entry_id, window, cx);
                        }
                    })
                    .child(div().size(px(10.)).flex_shrink_0().rounded_full().bg(
                        marker_color_hsla(item.color, if filtered { 0.35 } else { 1.0 }),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(if filtered {
                                theme.text_muted
                            } else {
                                theme.text_primary
                            })
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(label),
                    )
                    .child(Self::icon_button(
                        theme,
                        ("marker-edit", entry_id as usize),
                        LucideIcon::Pencil,
                        tr_shared(locale, "marker.edit"),
                        move |event, window, cx| on_edit(entry_id, event.position, window, cx),
                    ))
                    .child(Self::icon_button(
                        theme,
                        ("marker-remove", entry_id as usize),
                        LucideIcon::X,
                        tr_shared(locale, "marker.delete"),
                        move |_, window, cx| on_delete(entry_id, window, cx),
                    ))
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        let on_toggle = self.on_toggle_section.clone();
        let on_request_clear = self.on_request_clear.clone();

        v_flex()
            .id("marker-panel")
            .w_full()
            .min_w_0()
            .min_h_0()
            // Directly below the quick-send section; shrinks (list scrolls) when space is short.
            .when(!self.collapsed, |this| {
                this.flex_shrink(1.).max_h(relative(0.4))
            })
            .when(self.collapsed, |this| this.flex_none())
            .bg(theme.bg_canvas)
            .border_t_1()
            .border_color(theme.border_card)
            .occlude()
            .child(
                h_flex()
                    .h(px(38.))
                    .flex_shrink_0()
                    .items_center()
                    .px_3()
                    .gap_1()
                    .when(!self.collapsed, |this| {
                        this.border_b_1().border_color(theme.border_card)
                    })
                    .child(Self::icon_button(
                        theme,
                        "marker-section-toggle",
                        section_icon,
                        tr_shared(locale, "marker.collapse_section"),
                        move |_, window, cx| on_toggle(window, cx),
                    ))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(format!(
                        "{} ({count})",
                        tr_shared(locale, "marker.panel_title")
                    )))
                    .when(count > 0 && !self.collapsed, |this| {
                        this.child(Self::icon_button(
                            theme,
                            "marker-clear-all",
                            LucideIcon::X,
                            tr_shared(locale, "marker.clear_all"),
                            move |_, window, cx| on_request_clear(window, cx),
                        ))
                    }),
            )
            .when(!self.collapsed && self.pending_clear && count > 0, |this| {
                this.child(
                    h_flex()
                        .flex_shrink_0()
                        .px_3()
                        .py_1p5()
                        .gap_2()
                        .items_center()
                        .border_b_1()
                        .border_color(theme.border_card)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme.error_text)
                                .child(marker_clear_confirm(locale, count)),
                        )
                        .child(Self::text_button(
                            theme,
                            "marker-clear-confirm",
                            tr_shared(locale, "marker.delete"),
                            true,
                            self.on_confirm_clear.clone(),
                        ))
                        .child(Self::text_button(
                            theme,
                            "marker-clear-cancel",
                            tr_shared(locale, "marker.cancel"),
                            false,
                            self.on_cancel_clear.clone(),
                        )),
                )
            })
            .when(!self.collapsed, |this| {
                this.child(
                    v_flex()
                        .id("marker-list")
                        .flex_1()
                        .min_h_0()
                        .px_1p5()
                        .py_1()
                        .gap_0p5()
                        .overflow_y_scroll()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .when(count == 0, |this| {
                            this.child(
                                div()
                                    .p_3()
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .child(tr_shared(locale, "marker.panel_empty")),
                            )
                        })
                        .children(rows),
                )
            })
            .into_any_element()
    }
}
