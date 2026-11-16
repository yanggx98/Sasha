use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui::theme::NanoTheme;

pub type TabSelectCallback = Rc<dyn Fn(u64, &mut Window, &mut App)>;
pub type TabCloseCallback = Rc<dyn Fn(u64, &mut Window, &mut App)>;
pub type TabNewCallback = Rc<dyn Fn(&mut Window, &mut App)>;
pub type SettingsOpenCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Sidebar width leaves room for session names and their close buttons.
pub const TAB_RAIL_WIDTH: Pixels = px(128.);

#[derive(Clone, Debug)]
pub struct TabItem {
    pub id: u64,
    pub title: SharedString,
    pub is_active: bool,
    pub is_connected: bool,
    pub is_simulated: bool,
}

/// Right-hand vertical tab rail hosting the window controls at the top
/// and the session tabs underneath on a unified background.
#[derive(IntoElement)]
pub struct TabRail {
    theme: NanoTheme,
    tabs: Vec<TabItem>,
    on_select_tab: TabSelectCallback,
    on_close_tab: TabCloseCallback,
    on_new_tab: TabNewCallback,
    on_open_settings: SettingsOpenCallback,
}

impl TabRail {
    pub fn new(
        theme: NanoTheme,
        tabs: Vec<TabItem>,
        on_select_tab: TabSelectCallback,
        on_close_tab: TabCloseCallback,
        on_new_tab: TabNewCallback,
        on_open_settings: SettingsOpenCallback,
    ) -> Self {
        Self {
            theme,
            tabs,
            on_select_tab,
            on_close_tab,
            on_new_tab,
            on_open_settings,
        }
    }
}

impl RenderOnce for TabRail {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let on_select_tab = self.on_select_tab;
        let on_close_tab = self.on_close_tab;
        let on_new_tab = self.on_new_tab;
        let on_open_settings = self.on_open_settings;

        v_flex()
            .id("workspace-tab-rail")
            .flex_shrink_0()
            .w(TAB_RAIL_WIDTH)
            .h_full()
            .bg(theme.bg_card)
            .child(
                v_flex()
                    .id("tab-rail-list")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .pl_0()
                    .pr_1p5()
                    .pt(px(38.))
                    .pb_1()
                    .gap_1()
                    .children(self.tabs.into_iter().map(move |tab| {
                        let tab_id = tab.id;
                        let select_cb = on_select_tab.clone();
                        let close_cb = on_close_tab.clone();
                        let dot_color = if tab.is_simulated {
                            theme.sim_badge_text
                        } else if tab.is_connected {
                            theme.rx_green
                        } else {
                            theme.text_muted
                        };

                        h_flex()
                            .id(ElementId::named_usize("workspace-tab", tab_id as usize))
                            .h(px(28.))
                            .items_center()
                            .gap_1p5()
                            .pr_1()
                            .when(tab.is_active, |this| {
                                this.pl_2()
                                    .rounded_r(px(6.))
                                    .bg(theme.bg_canvas)
                                    .text_color(theme.text_primary)
                            })
                            .when(!tab.is_active, |this| {
                                this.ml_1p5()
                                    .pl_2()
                                    .rounded(px(6.))
                                    .bg(gpui_kit::transparent_black())
                                    .text_color(theme.text_secondary)
                                    .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            })
                            .text_xs()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                select_cb(tab_id, window, cx);
                            })
                            .child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(dot_color),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when(tab.is_active, |this| {
                                        this.font_weight(FontWeight::MEDIUM)
                                    })
                                    .child(tab.title),
                            )
                            .child(
                                div()
                                    .id(ElementId::named_usize(
                                        "workspace-tab-close",
                                        tab_id as usize,
                                    ))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(px(15.))
                                    .rounded(px(4.))
                                    .text_color(theme.text_muted)
                                    .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        close_cb(tab_id, window, cx);
                                    })
                                    .child(Icon::new(LucideIcon::X).size(px(10.))),
                            )
                    }))
                    .child(
                        h_flex()
                            .id("tab-rail-new-tab")
                            .ml_1p5()
                            .pl_2()
                            .pr_1()
                            .h(px(26.))
                            .items_center()
                            .gap_1p5()
                            .rounded(px(6.))
                            .text_xs()
                            .text_color(theme.text_secondary)
                            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            .cursor_pointer()
                            .tooltip(|window, cx| {
                                Tooltip::new("新建标签页 (Ctrl+T)").build(window, cx)
                            })
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_new_tab(window, cx);
                            })
                            .child(Icon::new(LucideIcon::Plus).size(px(12.)))
                            .child("新建"),
                    ),
            )
            .child(
                h_flex()
                    .id("tab-rail-footer")
                    .flex_shrink_0()
                    .w_full()
                    .h(px(34.))
                    .px_1p5()
                    .items_center()
                    .child(
                        h_flex()
                            .id("tab-rail-settings")
                            .w_full()
                            .h(px(26.))
                            .pl_2()
                            .pr_1()
                            .items_center()
                            .gap_1p5()
                            .rounded(px(6.))
                            .text_xs()
                            .text_color(theme.text_secondary)
                            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            .cursor_pointer()
                            .tooltip(|window, cx| Tooltip::new("设置").build(window, cx))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_open_settings(window, cx);
                            })
                            .child(Icon::new(LucideIcon::Settings).size(px(12.)))
                            .child("设置"),
                    ),
            )
    }
}
