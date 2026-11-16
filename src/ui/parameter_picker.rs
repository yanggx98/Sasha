use std::rc::Rc;

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::{ActivePickerKind, SerialConfig};
use crate::ui::motion::animate_popover_enter;
use crate::ui::theme::NanoTheme;

pub type SelectPresetCallback = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// Top-center VS Code-style quick-select & editable parameter popover (REQ-006, REQ-011).
/// Rendered without a background scrim so the workspace remains visible and
/// clicking outside dismisses without blocking other UI interactions.
#[derive(IntoElement)]
pub struct ParameterPickerPopover {
    theme: NanoTheme,
    kind: ActivePickerKind,
    open_epoch: u64,
    config: SerialConfig,
    input: Entity<InputState>,
    error: Option<SharedString>,
    on_select_preset: SelectPresetCallback,
}

impl ParameterPickerPopover {
    pub fn new(
        theme: NanoTheme,
        kind: ActivePickerKind,
        open_epoch: u64,
        config: SerialConfig,
        input: Entity<InputState>,
        error: Option<SharedString>,
        on_select_preset: SelectPresetCallback,
    ) -> Self {
        Self {
            theme,
            kind,
            open_epoch,
            config,
            input,
            error,
            on_select_preset,
        }
    }
}

impl RenderOnce for ParameterPickerPopover {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let current_value = self.config.current_value_str(self.kind);
        let typed_value = self.input.read(cx).value().to_string();
        let trimmed_typed = typed_value.trim().to_string();
        let presets = self.kind.presets();
        let has_exact_preset = presets.iter().any(|p| p == &trimmed_typed);
        let input_border = if self.error.is_some() {
            theme.error_text
        } else {
            theme.accent_blue
        };

        // Frame is 328px rather than the planned ~300px so the longest
        // placeholder (text-sm, ~277px) is not clipped. Padding 8px,
        // section gap 4px, radius 8px. Title and hint stay on one line.
        let popover = v_flex()
            .id("parameter-picker-popover")
            .relative()
            .w(px(328.))
            .p_2()
            .gap_1()
            .rounded(px(8.))
            .bg(theme.bg_popover)
            .border_1()
            .border_color(theme.border_popover)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_primary)
                            .child(self.kind.title()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child("Enter 确认 · Esc 取消"),
                    ),
            )
            .child(
                // Input::Medium is h_8 (32px) with its own padding. The 1px
                // border is outside that box, so the frame is 34px. Extra
                // wrapper padding would stack on the control's own inset.
                div()
                    .id("parameter-picker-input-box")
                    .w_full()
                    .rounded(px(6.))
                    .bg(theme.bg_canvas)
                    .border_1()
                    .border_color(input_border)
                    .child(
                        Input::new(&self.input)
                            .appearance(false)
                            .cleanable(true),
                    ),
            )
            .when_some(self.error, |this, err| {
                this.child(
                    div()
                        .id("parameter-picker-error")
                        .w_full()
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .bg(theme.error_bg)
                        .text_xs()
                        .text_color(theme.error_text)
                        .child(err),
                )
            })
            .child(
                v_flex()
                    .id("parameter-picker-presets")
                    .w_full()
                    .gap_0p5()
                    .when(!trimmed_typed.is_empty() && !has_exact_preset, |this| {
                        let custom_val = trimmed_typed.clone();
                        let on_select = self.on_select_preset.clone();
                        this.child(
                            h_flex()
                                .id("picker-custom-apply")
                                .w_full()
                                .min_h(px(24.))
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .px_2()
                                .line_height(px(16.))
                                .rounded(px(6.))
                                .bg(theme.bg_toggle_active)
                                .hover(|s| s.bg(theme.bg_hover))
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    cx.stop_propagation();
                                    on_select(custom_val.clone(), window, cx);
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(theme.text_primary)
                                        .child(format!("应用输入值: {trimmed_typed}")),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .whitespace_nowrap()
                                        .text_xs()
                                        .text_color(theme.sim_badge_text)
                                        .child("Enter"),
                                ),
                        )
                    })
                    .children(presets.into_iter().enumerate().map(|(idx, preset)| {
                        let is_current = preset == current_value;
                        let preset_for_click = preset.clone();
                        let on_select = self.on_select_preset.clone();

                        h_flex()
                            .id(("picker-preset", idx))
                            .w_full()
                            .min_h(px(24.))
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_2()
                            .line_height(px(16.))
                            .rounded(px(6.))
                            .when(is_current, |this| this.bg(theme.bg_toggle_active))
                            .hover(|s| s.bg(theme.bg_hover))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                on_select(preset_for_click.clone(), window, cx);
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(if is_current {
                                        theme.text_primary
                                    } else {
                                        theme.text_secondary
                                    })
                                    .child(preset),
                            )
                            .when(is_current, |this| {
                                this.child(
                                    h_flex()
                                        .flex_shrink_0()
                                        .items_center()
                                        .gap_1()
                                        .whitespace_nowrap()
                                        .text_xs()
                                        .text_color(theme.accent_blue_hover)
                                        .child(
                                            Icon::new(LucideIcon::Check)
                                                .size(px(12.))
                                                .text_color(theme.accent_blue_hover),
                                        )
                                        .child("当前"),
                                )
                            })
                    })),
            );

        animate_popover_enter(popover, self.open_epoch)
    }
}
