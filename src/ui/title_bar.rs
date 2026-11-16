use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{Icon, TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui::theme::NanoTheme;

/// Immersive titlebar that blends seamlessly with the main workspace canvas.
/// Uses `gpui_kit::component::TitleBar` for native Windows drag, double-click
/// maximize/restore, minimize, maximize, and close controls.
#[derive(IntoElement)]
pub struct AppTitleBar {
    theme: NanoTheme,
    is_simulated: bool,
}

impl AppTitleBar {
    pub fn new(theme: NanoTheme, is_simulated: bool) -> Self {
        Self {
            theme,
            is_simulated,
        }
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        TitleBar::new().bg(self.theme.bg_canvas).border_b_0().child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(LucideIcon::Activity)
                        .size(px(15.))
                        .text_color(self.theme.accent_blue),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(self.theme.text_primary)
                        .child("Sasha"),
                )
                .when(self.is_simulated, |this| {
                    this.child(
                        div()
                            .px_1p5()
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(self.theme.sim_badge_bg)
                            .text_xs()
                            .text_color(self.theme.sim_badge_text)
                            .child("模拟"),
                    )
                }),
        )
    }
}
