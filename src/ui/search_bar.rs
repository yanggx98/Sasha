use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::ui::theme::NanoTheme;

pub type SearchActionCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Floating log search bar, sitting at the upper right of the send-config strip.
#[derive(IntoElement)]
pub struct LogSearchBar {
    theme: NanoTheme,
    locale: Locale,
    input: Entity<InputState>,
    count_label: Option<SharedString>,
    has_matches: bool,
    on_prev: SearchActionCallback,
    on_next: SearchActionCallback,
    on_close: SearchActionCallback,
}

impl LogSearchBar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: NanoTheme,
        locale: Locale,
        input: Entity<InputState>,
        count_label: Option<SharedString>,
        has_matches: bool,
        on_prev: SearchActionCallback,
        on_next: SearchActionCallback,
        on_close: SearchActionCallback,
    ) -> Self {
        Self {
            theme,
            locale,
            input,
            count_label,
            has_matches,
            on_prev,
            on_next,
            on_close,
        }
    }

    fn nav_button(
        &self,
        id: &'static str,
        icon: LucideIcon,
        tooltip: SharedString,
        enabled: bool,
        on_click: SearchActionCallback,
    ) -> impl IntoElement {
        let icon_color = if enabled {
            self.theme.text_secondary
        } else {
            self.theme.text_muted.opacity(0.45)
        };
        let theme = self.theme;
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(22.))
            .rounded(px(5.))
            .text_color(icon_color)
            .when(enabled, |this| {
                this.cursor_pointer()
                    .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        on_click(window, cx);
                    })
            })
            .when(!enabled, |this| {
                this.on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .child(Icon::new(icon).size(px(13.)))
    }
}

impl RenderOnce for LogSearchBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let count_color = if self.has_matches {
            self.theme.text_secondary
        } else {
            self.theme.error_text
        };

        h_flex()
            .id("log-search-bar")
            .items_center()
            .gap_1()
            .h(px(34.))
            .pl_2p5()
            .pr_1p5()
            .rounded(px(8.))
            .bg(self.theme.bg_popover)
            .border_1()
            .border_color(self.theme.border_popover)
            .shadow_md()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(self.theme.text_muted)
                    .child(Icon::new(LucideIcon::Search).size(px(13.))),
            )
            .child(
                div()
                    .w(px(176.))
                    .text_sm()
                    .text_color(self.theme.text_primary)
                    .child(Input::new(&self.input).appearance(false).cleanable(true)),
            )
            .when_some(self.count_label.clone(), |this, label| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .px_1()
                        .text_xs()
                        .font_family("Consolas")
                        .text_color(count_color)
                        .child(label),
                )
            })
            .child(
                div()
                    .w(px(1.))
                    .h(px(14.))
                    .mx_0p5()
                    .bg(self.theme.border_card),
            )
            .child(self.nav_button(
                "log-search-prev",
                LucideIcon::ChevronUp,
                tr_shared(self.locale, "search.prev"),
                self.has_matches,
                self.on_prev.clone(),
            ))
            .child(self.nav_button(
                "log-search-next",
                LucideIcon::ChevronDown,
                tr_shared(self.locale, "search.next"),
                self.has_matches,
                self.on_next.clone(),
            ))
            .child(self.nav_button(
                "log-search-close",
                LucideIcon::X,
                tr_shared(self.locale, "search.close"),
                true,
                self.on_close.clone(),
            ))
    }
}
