use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, quick_shortcut_hint, tr_shared};
use crate::model::quick_command::{QuickCommand, QuickRow};
use crate::ui::receive_toolbar::ToolbarActionCallback;
use crate::ui::script_panel::rail_button;
use crate::ui::theme::NanoTheme;

pub type QuickIdCallback = Rc<dyn Fn(String, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct QuickSendPanel {
    pub theme: NanoTheme,
    pub locale: Locale,
    pub commands: Vec<QuickCommand>,
    pub rows: Vec<QuickRow>,
    pub search_input: Entity<InputState>,
    pub error: Option<SharedString>,
    pub skipped_notice: Option<SharedString>,
    pub collapsed: bool,
    pub compact: bool,
    /// Configured modifiers of the quick-send keys, e.g. `Alt`; the digit is appended.
    pub shortcut_mods: String,
    pub pending_delete: Option<String>,
    pub on_send: QuickIdCallback,
    pub on_toggle_group: QuickIdCallback,
    pub on_toggle_section: ToolbarActionCallback,
    pub on_expand_dock: ToolbarActionCallback,
    pub on_new: ToolbarActionCallback,
    pub on_edit: QuickIdCallback,
    pub on_delete: QuickIdCallback,
    pub on_confirm_delete: QuickIdCallback,
    pub on_cancel_delete: ToolbarActionCallback,
}

impl QuickSendPanel {
    fn action_icon(
        &self,
        id: impl Into<ElementId>,
        icon: LucideIcon,
        tooltip: SharedString,
        box_size: Pixels,
        icon_size: Pixels,
        action: ToolbarActionCallback,
    ) -> impl IntoElement {
        let theme = self.theme;
        div()
            .id(id)
            .size(box_size)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(theme.text_muted)
            .hover(move |s| s.text_color(theme.text_primary))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                action(window, cx);
            })
            .child(Icon::new(icon).size(icon_size))
    }
}

impl RenderOnce for QuickSendPanel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let locale = self.locale;
        let theme = self.theme;

        if self.compact {
            // 图标栏：与脚本图标同样式，紧挨在脚本图标下方，不再单独画分割线和撑满高度。
            return v_flex()
                .id("quick-send-panel")
                .w_full()
                .flex_none()
                .px_1p5()
                .bg(theme.bg_canvas)
                .occlude()
                .child(rail_button(
                    theme,
                    "quick-rail",
                    LucideIcon::Zap,
                    tr_shared(locale, "quick.title"),
                    None,
                    self.on_expand_dock.clone(),
                ))
                .into_any_element();
        }

        let section_icon = if self.collapsed {
            LucideIcon::ChevronRight
        } else {
            LucideIcon::ChevronDown
        };

        let pending_delete = self.pending_delete.clone();
        let row_elements = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(row_idx, row)| match row {
                QuickRow::Group {
                    name,
                    count,
                    collapsed,
                } => {
                    let group_key = name.clone();
                    let label: SharedString = if name.is_empty() {
                        tr_shared(locale, "quick.ungrouped")
                    } else {
                        name.clone().into()
                    };
                    let on_toggle = self.on_toggle_group.clone();
                    let chevron = if *collapsed {
                        LucideIcon::ChevronRight
                    } else {
                        LucideIcon::ChevronDown
                    };
                    Some(
                        h_flex()
                            .id(("quick-group", row_idx))
                            .w_full()
                            .h(px(26.))
                            .px_2()
                            .gap_1p5()
                            .rounded(px(5.))
                            .cursor_pointer()
                            .text_color(theme.text_muted)
                            .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_secondary))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_toggle(group_key.clone(), window, cx);
                            })
                            .child(Icon::new(chevron).size(px(12.)).flex_shrink_0())
                            .child(div().flex_1().min_w_0().truncate().text_xs().child(label))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .child(count.to_string()),
                            )
                            .into_any_element(),
                    )
                }
                QuickRow::Command { index, shortcut } => {
                    let cmd = self.commands.get(*index)?;
                    if pending_delete.as_deref() == Some(cmd.id.as_str()) {
                        let cmd_id_confirm = cmd.id.clone();
                        let on_confirm = self.on_confirm_delete.clone();
                        let on_cancel = self.on_cancel_delete.clone();
                        let name: SharedString = cmd.name.clone().into();
                        return Some(
                            h_flex()
                                .id(("quick-cmd-del", *index))
                                .w_full()
                                .h(px(30.))
                                .px_2()
                                .gap_1p5()
                                .rounded(px(6.))
                                .bg(theme.bg_hover)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .text_color(theme.text_secondary)
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .id(("quick-confirm-del", *index))
                                        .px_2()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(theme.error_text)
                                        .hover(move |s| s.bg(theme.bg_canvas))
                                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                            cx.stop_propagation();
                                            on_confirm(cmd_id_confirm.clone(), window, cx);
                                        })
                                        .child(tr_shared(locale, "quick.delete_confirm")),
                                )
                                .child(
                                    div()
                                        .id(("quick-cancel-del", *index))
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(theme.text_muted)
                                        .hover(move |s| s.text_color(theme.text_primary))
                                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                            cx.stop_propagation();
                                            on_cancel(window, cx);
                                        })
                                        .child(tr_shared(locale, "quick.cancel")),
                                )
                                .into_any_element(),
                        );
                    }

                    let cmd_id = cmd.id.clone();
                    let cmd_id_edit = cmd.id.clone();
                    let cmd_id_del = cmd.id.clone();
                    let name: SharedString = cmd.name.clone().into();
                    let preview = cmd.preview();
                    let tooltip: SharedString = match shortcut {
                        Some(digit) => format!(
                            "{preview}\n{}",
                            quick_shortcut_hint(locale, &format!("{}+{digit}", self.shortcut_mods))
                        )
                        .into(),
                        None => preview.into(),
                    };
                    let on_send = self.on_send.clone();
                    let on_edit = self.on_edit.clone();
                    let on_delete = self.on_delete.clone();

                    Some(
                        h_flex()
                            .id(("quick-cmd", *index))
                            .w_full()
                            .h(px(30.))
                            .px_2()
                            .gap_1p5()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .text_color(theme.text_secondary)
                            .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_send(cmd_id.clone(), window, cx);
                            })
                            .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
                            .when_some(*shortcut, |this, digit| {
                                this.child(
                                    div()
                                        .flex_shrink_0()
                                        .px_1()
                                        .text_xs()
                                        .text_color(theme.text_muted)
                                        .child(format!("{}+{digit}", self.shortcut_mods)),
                                )
                            })
                            .child(self.action_icon(
                                ("quick-edit", *index),
                                LucideIcon::Pencil,
                                tr_shared(locale, "quick.edit"),
                                px(20.),
                                px(12.),
                                Rc::new(move |window, cx| {
                                    on_edit(cmd_id_edit.clone(), window, cx);
                                }),
                            ))
                            .child(self.action_icon(
                                ("quick-delete", *index),
                                LucideIcon::Trash,
                                tr_shared(locale, "quick.delete"),
                                px(20.),
                                px(12.),
                                Rc::new(move |window, cx| {
                                    on_delete(cmd_id_del.clone(), window, cx);
                                }),
                            ))
                            .into_any_element(),
                    )
                }
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("quick-send-panel")
            .w_full()
            .min_w_0()
            .min_h_0()
            .when(!self.collapsed, |this| this.flex_1())
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
                    .border_b_1()
                    .border_color(theme.border_card)
                    .child(self.action_icon(
                        "quick-section-toggle",
                        section_icon,
                        tr_shared(locale, "quick.collapse_section"),
                        px(20.),
                        px(13.),
                        self.on_toggle_section.clone(),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(tr_shared(locale, "quick.title")),
                    )
                    .child(self.action_icon(
                        "quick-new",
                        LucideIcon::Plus,
                        tr_shared(locale, "quick.new"),
                        px(24.),
                        px(14.),
                        self.on_new.clone(),
                    )),
            )
            .when(!self.collapsed, |this| {
                this.child(
                    h_flex()
                        .h(px(32.))
                        .flex_shrink_0()
                        .px_2p5()
                        .gap_1p5()
                        .items_center()
                        .border_b_1()
                        .border_color(theme.border_card)
                        .child(
                            Icon::new(LucideIcon::Search)
                                .size(px(13.))
                                .text_color(theme.text_muted)
                                .flex_shrink_0(),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&self.search_input)
                                    .appearance(false)
                                    .cleanable(true),
                            ),
                        ),
                )
                .when_some(self.skipped_notice.clone(), |this, notice| {
                    this.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(notice),
                    )
                })
                .when_some(self.error.clone(), |this, err| {
                    this.child(
                        div()
                            .px_3()
                            .py_1()
                            .text_xs()
                            .text_color(theme.error_text)
                            .child(err),
                    )
                })
                .child(
                    v_flex()
                        .id("quick-send-list")
                        .flex_1()
                        .min_h_0()
                        .px_1p5()
                        .py_1()
                        .gap_0p5()
                        .overflow_y_scroll()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .when(self.commands.is_empty(), |this| {
                            this.child(
                                div()
                                    .p_3()
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .child(tr_shared(locale, "quick.empty")),
                            )
                        })
                        .children(row_elements),
                )
            })
            .into_any_element()
    }
}
