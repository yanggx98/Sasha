use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, scripts_status_failed, tr_shared};
use crate::scripting::{ScriptStatus, StopReason};
use crate::ui::receive_toolbar::ToolbarActionCallback;
use crate::ui::theme::NanoTheme;

pub type RunScriptCallback = Rc<dyn Fn(PathBuf, &mut Window, &mut App)>;
pub type StopScriptCallback = Rc<dyn Fn(u64, &mut Window, &mut App)>;
pub type RemoveScriptCallback = Rc<dyn Fn(PathBuf, &mut Window, &mut App)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptPanelItem {
    pub path: PathBuf,
    pub run_id: Option<u64>,
    pub status: Option<ScriptStatus>,
    pub can_run: bool,
    pub disabled_reason: Option<SharedString>,
}

/// Formats a script's lifecycle status (including stop reason or error) for tooltips.
pub fn format_script_status(locale: Locale, status: Option<&ScriptStatus>) -> SharedString {
    match status {
        None => tr_shared(locale, "scripts.not_running"),
        Some(ScriptStatus::Running) => tr_shared(locale, "scripts.status_running"),
        Some(ScriptStatus::Stopping) => tr_shared(locale, "scripts.status_stopping"),
        Some(ScriptStatus::Completed) => tr_shared(locale, "scripts.status_completed"),
        Some(ScriptStatus::Stopped {
            reason: StopReason::User,
        }) => tr_shared(locale, "scripts.status_stopped_user"),
        Some(ScriptStatus::Stopped {
            reason: StopReason::Disconnected,
        }) => tr_shared(locale, "scripts.status_stopped_disconnected"),
        Some(ScriptStatus::Failed { error }) => scripts_status_failed(locale, error).into(),
    }
}

fn status_dot_color(theme: &NanoTheme, status: Option<&ScriptStatus>) -> Hsla {
    match status {
        Some(ScriptStatus::Running) => theme.rx_green,
        Some(ScriptStatus::Stopping) => theme.accent_blue_hover,
        Some(ScriptStatus::Failed { .. }) => theme.error_text,
        None | Some(ScriptStatus::Completed | ScriptStatus::Stopped { .. }) => theme.text_muted,
    }
}
pub const SCRIPT_PANEL_WIDTH: Pixels = px(280.);
pub const SCRIPT_PANEL_MIN_WIDTH: Pixels = px(52.);

/// Automatic window constraints never overwrite the user's preferred width.
pub fn panel_width(preferred: Pixels, main_width: Pixels) -> Pixels {
    preferred.clamp(SCRIPT_PANEL_MIN_WIDTH, panel_max_width(main_width))
}

fn panel_max_width(main_width: Pixels) -> Pixels {
    (main_width - px(380.)).clamp(SCRIPT_PANEL_MIN_WIDTH, px(480.))
}

pub fn dragged_panel_width(pointer_x: Pixels, main_width: Pixels) -> Pixels {
    panel_width(main_width - pointer_x, main_width)
}

pub fn panel_is_compact(width: Pixels) -> bool {
    width < px(180.)
}

#[derive(Clone)]
pub struct ResizeScriptPanel {
    pub tab_id: u64,
}

impl Render for ResizeScriptPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A session-local script list with per-script run, stop, and remove controls.
#[derive(IntoElement)]
pub struct ScriptPanel {
    pub theme: NanoTheme,
    pub locale: Locale,
    pub items: Vec<ScriptPanelItem>,
    pub load_error: Option<SharedString>,
    pub compact: bool,
    pub on_load: ToolbarActionCallback,
    pub on_collapse: ToolbarActionCallback,
    pub on_expand: ToolbarActionCallback,
    pub on_run: RunScriptCallback,
    pub on_stop: StopScriptCallback,
    pub on_remove: RemoveScriptCallback,
}

impl ScriptPanel {
    fn compact_action(
        &self,
        id: impl Into<ElementId>,
        icon: LucideIcon,
        tooltip: SharedString,
        action: Option<ToolbarActionCallback>,
    ) -> impl IntoElement {
        self.sized_action(id, icon, tooltip, action, px(20.), px(13.))
    }

    fn action(
        &self,
        id: impl Into<ElementId>,
        icon: LucideIcon,
        tooltip: SharedString,
        action: Option<ToolbarActionCallback>,
    ) -> impl IntoElement {
        self.sized_action(id, icon, tooltip, action, px(24.), px(14.))
    }

    fn sized_action(
        &self,
        id: impl Into<ElementId>,
        icon: LucideIcon,
        tooltip: SharedString,
        action: Option<ToolbarActionCallback>,
        box_size: Pixels,
        icon_size: Pixels,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(box_size)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(if action.is_some() {
                self.theme.text_secondary
            } else {
                self.theme.text_muted
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .when_some(action, |this, action| {
                this.cursor_pointer()
                    .hover(|s| s.text_color(self.theme.text_primary))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        action(window, cx);
                    })
            })
            .child(Icon::new(icon).size(icon_size))
    }
}

impl RenderOnce for ScriptPanel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let locale = self.locale;
        let rows = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let path = &item.path;
                let filename: SharedString = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
                    .into();
                let status_text = format_script_status(locale, item.status.as_ref());
                let dot_color = status_dot_color(&self.theme, item.status.as_ref());
                let row_tooltip: SharedString =
                    format!("{}\n{}", path.display(), status_text).into();
                let path_to_remove = path.clone();
                let on_remove = self.on_remove.clone();
                if self.compact {
                    return h_flex()
                        .id(("script-row", index))
                        .h(px(34.))
                        .w_full()
                        .flex_shrink_0()
                        .justify_center()
                        .gap(px(3.))
                        .rounded(px(6.))
                        .hover(|s| s.bg(self.theme.bg_hover))
                        .tooltip(move |window, cx| {
                            Tooltip::new(row_tooltip.clone()).build(window, cx)
                        })
                        .child(
                            Icon::new(LucideIcon::FileCode)
                                .size(px(16.))
                                .text_color(self.theme.text_secondary),
                        )
                        .child(div().size(px(5.)).rounded_full().bg(dot_color))
                        .into_any_element();
                }

                let is_active = item.status.as_ref().is_some_and(ScriptStatus::is_active);
                let (action_icon, action_tooltip, action_cb): (
                    LucideIcon,
                    SharedString,
                    Option<ToolbarActionCallback>,
                ) = if is_active {
                    let stop_cb = item.run_id.map(|run_id| {
                        let on_stop = self.on_stop.clone();
                        Rc::new(move |window: &mut Window, cx: &mut App| {
                            on_stop(run_id, window, cx)
                        }) as ToolbarActionCallback
                    });
                    (
                        LucideIcon::Square,
                        tr_shared(locale, "scripts.stop"),
                        stop_cb,
                    )
                } else if item.can_run {
                    let path_to_run = path.clone();
                    let on_run = self.on_run.clone();
                    (
                        LucideIcon::Play,
                        tr_shared(locale, "scripts.run"),
                        Some(Rc::new(move |window, cx| {
                            on_run(path_to_run.clone(), window, cx)
                        })),
                    )
                } else {
                    let reason = item
                        .disabled_reason
                        .clone()
                        .unwrap_or_else(|| tr_shared(locale, "scripts.runtime_unavailable"));
                    (LucideIcon::Play, reason, None)
                };

                h_flex()
                    .id(("script-row", index))
                    .w_full()
                    .h(px(34.))
                    .px_2()
                    .gap_2()
                    .rounded(px(6.))
                    .text_color(self.theme.text_secondary)
                    .hover(|s| {
                        s.bg(self.theme.bg_hover)
                            .text_color(self.theme.text_primary)
                    })
                    .child(
                        div()
                            .size(px(5.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(dot_color),
                    )
                    .child(
                        div()
                            .id(("script-name", index))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .tooltip(move |window, cx| {
                                Tooltip::new(row_tooltip.clone()).build(window, cx)
                            })
                            .child(filename),
                    )
                    .child(self.action(
                        ("script-run", index),
                        action_icon,
                        action_tooltip,
                        action_cb,
                    ))
                    .child(self.action(
                        ("script-remove", index),
                        LucideIcon::X,
                        tr_shared(locale, "scripts.remove"),
                        Some(Rc::new(move |window, cx| {
                            on_remove(path_to_remove.clone(), window, cx)
                        })),
                    ))
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("script-panel")
            .size_full()
            .min_h_0()
            .min_w_0()
            .bg(self.theme.bg_canvas)
            .border_l_1()
            .border_color(self.theme.border_card)
            .occlude()
            .when(self.compact, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .h(px(38.))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .border_b_1()
                        .border_color(self.theme.border_card)
                        .child(
                            self.compact_action(
                                "script-load",
                                LucideIcon::Plus,
                                self.load_error
                                    .clone()
                                    .unwrap_or_else(|| tr_shared(locale, "scripts.load")),
                                Some(self.on_load.clone()),
                            ),
                        )
                        .child(self.compact_action(
                            "script-expand",
                            LucideIcon::ChevronLeft,
                            tr_shared(locale, "scripts.expand"),
                            Some(self.on_expand.clone()),
                        )),
                )
            })
            .when(!self.compact, |this| {
                this.child(
                    h_flex()
                        .h(px(38.))
                        .flex_shrink_0()
                        .px_3()
                        .gap_1()
                        .border_b_1()
                        .border_color(self.theme.border_card)
                        .child(
                            div()
                                .id("script-title-drag")
                                .flex_1()
                                .h_full()
                                .flex()
                                .items_center()
                                .cursor_default()
                                .window_control_area(WindowControlArea::Drag)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    gpui_kit::base::GlobalState::suppress_text_selection(cx);
                                })
                                .text_sm()
                                .child(tr_shared(locale, "scripts.title")),
                        )
                        .child(self.action(
                            "script-load",
                            LucideIcon::Plus,
                            tr_shared(locale, "scripts.load"),
                            Some(self.on_load.clone()),
                        ))
                        .child(self.action(
                            "script-collapse",
                            LucideIcon::ChevronRight,
                            tr_shared(locale, "scripts.collapse"),
                            Some(self.on_collapse.clone()),
                        )),
                )
            })
            .child(
                v_flex()
                    .id("script-list")
                    .flex_1()
                    .min_h_0()
                    .px_1p5()
                    .py_1()
                    .gap_0p5()
                    .overflow_y_scroll()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .when(rows.is_empty() && !self.compact, |this| {
                        this.child(
                            div()
                                .p_3()
                                .text_xs()
                                .text_color(self.theme.text_muted)
                                .child(tr_shared(locale, "scripts.empty")),
                        )
                    })
                    .children(rows)
                    .when_some(
                        self.load_error.clone().filter(|_| !self.compact),
                        |this, error| {
                            this.child(
                                div()
                                    .p_3()
                                    .text_xs()
                                    .text_color(self.theme.error_text)
                                    .child(error),
                            )
                        },
                    ),
            )
    }
}

#[cfg(test)]
mod sizing_tests {
    use super::{SCRIPT_PANEL_MIN_WIDTH, dragged_panel_width, panel_is_compact, panel_width};
    use gpui_kit::px;

    #[test]
    fn narrow_windows_keep_terminal_and_session_rail_available() {
        for window_width in 560..=1200 {
            let main = px(window_width as f32 - 128.);
            for preferred in [52., 180., 280., 480.] {
                let width = panel_width(px(preferred), main);
                assert!(width >= SCRIPT_PANEL_MIN_WIDTH);
                assert!(main - width >= px(380.));
            }
        }
        assert_eq!(panel_width(px(280.), px(432.)), px(52.));
    }

    #[test]
    fn drag_clamps_at_both_edges_and_resizes_in_both_directions() {
        let main = px(872.);
        assert_eq!(dragged_panel_width(px(900.), main), px(52.));
        assert_eq!(dragged_panel_width(px(0.), main), px(480.));
        assert_eq!(dragged_panel_width(px(672.), main), px(200.));
        assert_eq!(dragged_panel_width(px(572.), main), px(300.));
    }

    #[test]
    fn window_constraints_do_not_destroy_preferred_width() {
        let preferred = px(320.);
        assert_eq!(panel_width(preferred, px(432.)), px(52.));
        assert_eq!(panel_width(preferred, px(872.)), preferred);
    }

    #[test]
    fn dragging_across_compact_boundary_restores_full_rows() {
        let main = px(872.);
        assert!(panel_is_compact(dragged_panel_width(px(828.), main)));
        assert!(panel_is_compact(dragged_panel_width(px(693.), main)));
        assert!(!panel_is_compact(dragged_panel_width(px(692.), main)));
        assert!(!panel_is_compact(dragged_panel_width(px(592.), main)));
    }
}

#[cfg(test)]
mod status_tests {
    use super::format_script_status;
    use crate::i18n::Locale;
    use crate::scripting::{ScriptStatus, StopReason};

    #[test]
    fn format_script_status_covers_all_states_in_both_locales() {
        assert_eq!(format_script_status(Locale::EnUs, None), "Not running");
        assert_eq!(format_script_status(Locale::ZhCn, None), "未运行");

        assert_eq!(
            format_script_status(Locale::EnUs, Some(&ScriptStatus::Running)),
            "Running"
        );
        assert_eq!(
            format_script_status(Locale::ZhCn, Some(&ScriptStatus::Running)),
            "运行中"
        );

        assert_eq!(
            format_script_status(Locale::EnUs, Some(&ScriptStatus::Stopping)),
            "Stopping"
        );
        assert_eq!(
            format_script_status(Locale::ZhCn, Some(&ScriptStatus::Stopping)),
            "正在停止"
        );

        assert_eq!(
            format_script_status(Locale::EnUs, Some(&ScriptStatus::Completed)),
            "Completed"
        );
        assert_eq!(
            format_script_status(Locale::ZhCn, Some(&ScriptStatus::Completed)),
            "已完成"
        );

        assert_eq!(
            format_script_status(
                Locale::EnUs,
                Some(&ScriptStatus::Stopped {
                    reason: StopReason::User
                })
            ),
            "Stopped"
        );
        assert_eq!(
            format_script_status(
                Locale::ZhCn,
                Some(&ScriptStatus::Stopped {
                    reason: StopReason::User
                })
            ),
            "已停止"
        );

        assert_eq!(
            format_script_status(
                Locale::EnUs,
                Some(&ScriptStatus::Stopped {
                    reason: StopReason::Disconnected
                })
            ),
            "Stopped (disconnected)"
        );
        assert_eq!(
            format_script_status(
                Locale::ZhCn,
                Some(&ScriptStatus::Stopped {
                    reason: StopReason::Disconnected
                })
            ),
            "已停止（串口已断开）"
        );

        assert_eq!(
            format_script_status(
                Locale::EnUs,
                Some(&ScriptStatus::Failed {
                    error: "syntax error".to_string()
                })
            ),
            "Failed: syntax error"
        );
        assert_eq!(
            format_script_status(
                Locale::ZhCn,
                Some(&ScriptStatus::Failed {
                    error: "syntax error".to_string()
                })
            ),
            "失败: syntax error"
        );
    }
}
