use std::rc::Rc;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::i18n::{Locale, tr_shared};
use crate::model::log_filter::{FilterDirection, LogFilter};
use crate::ui::theme::NanoTheme;

#[derive(Clone, Copy)]
pub enum FilterControl {
    CycleDirection,
    CaseSensitive,
    Regex,
    Exclude,
    Reset,
}

pub type FilterCallback = Rc<dyn Fn(FilterControl, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct LogFilterBar {
    pub theme: NanoTheme,
    pub locale: Locale,
    pub input: Entity<InputState>,
    pub filter: LogFilter,
    pub invalid: bool,
    pub visible: usize,
    pub total: usize,
    pub on_control: FilterCallback,
}

impl LogFilterBar {
    fn button(
        &self,
        id: &'static str,
        label_key: &'static str,
        tooltip_key: &'static str,
        active: bool,
        control: FilterControl,
    ) -> impl IntoElement {
        let callback = self.on_control.clone();
        let tooltip = tr_shared(self.locale, tooltip_key);
        let theme = self.theme;
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .h(px(22.))
            .min_w(px(if matches!(control, FilterControl::CycleDirection) {
                42.
            } else {
                22.
            }))
            .px_1()
            .rounded(px(6.))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if active {
                theme.accent_blue_hover
            } else {
                theme.text_secondary
            })
            .hover(move |s| s.bg(theme.bg_hover))
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                callback(control, window, cx);
            })
            .child(tr_shared(self.locale, label_key))
    }
}

impl RenderOnce for LogFilterBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let count_tooltip = crate::i18n::filter_count_label(self.locale, self.visible, self.total);
        let direction = self.filter.direction;
        let separator = || {
            div()
                .flex_shrink_0()
                .w(px(1.))
                .h(px(14.))
                .mx_0p5()
                .bg(self.theme.border_card)
        };
        let row = h_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .items_center()
            .child(self.button(
                "filter-direction",
                match direction {
                    FilterDirection::All => "filter.all",
                    FilterDirection::Rx => "filter.rx",
                    FilterDirection::Tx => "filter.tx",
                },
                "filter.direction_tooltip",
                direction != FilterDirection::All,
                FilterControl::CycleDirection,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(48.))
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .px_2()
                    .rounded(px(6.))
                    .bg(self.theme.bg_card)
                    .border_1()
                    .border_color(if self.invalid {
                        self.theme.error_text
                    } else {
                        self.theme.border_card
                    })
                    .text_sm()
                    .text_color(self.theme.text_primary)
                    .child(Input::new(&self.input).appearance(false).cleanable(true)),
            )
            .child(self.button(
                "filter-case",
                "filter.case_label",
                "filter.case_tooltip",
                self.filter.case_sensitive,
                FilterControl::CaseSensitive,
            ))
            .child(self.button(
                "filter-regex",
                "filter.regex_label",
                "filter.regex_tooltip",
                self.filter.regex,
                FilterControl::Regex,
            ))
            .child(self.button(
                "filter-exclude",
                "filter.exclude_label",
                "filter.exclude_tooltip",
                self.filter.exclude,
                FilterControl::Exclude,
            ))
            .child(separator())
            .child(
                div()
                    .id("filter-count")
                    .flex_shrink_0()
                    .min_w(px(30.))
                    .flex()
                    .justify_center()
                    .text_xs()
                    .text_color(self.theme.text_muted)
                    .tooltip(move |window, cx| {
                        Tooltip::new(count_tooltip.clone()).build(window, cx)
                    })
                    .child(format!("{}/{}", self.visible, self.total)),
            )
            .child(self.button(
                "filter-reset",
                "filter.reset_label",
                "filter.reset_tooltip",
                false,
                FilterControl::Reset,
            ));

        // Content of the toolbar's expanding drawer; the toolbar owns its
        // padding, surface and border. This stays outside the title drag area.
        v_flex()
            .id("log-filter-bar")
            .w_full()
            .flex_shrink_0()
            .py_1()
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(row)
            .children(self.invalid.then(|| {
                div()
                    .text_xs()
                    .text_color(self.theme.error_text)
                    .child(tr_shared(self.locale, "filter.invalid_regex"))
            }))
    }
}
