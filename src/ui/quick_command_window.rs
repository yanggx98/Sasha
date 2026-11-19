use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::checksum::Checksum;
use crate::i18n::{Locale, quick_save_failed, tr_shared};
use crate::model::quick_command::{
    QuickCommand, QuickCommandError, QuickEditorDraft, move_command, upsert_command,
};
use crate::model::{ConfigStore, SendLineEnding};
use crate::ui::theme::NanoTheme;
use crate::ui::theme_manager::{ThemeHandle, ThemeManager};
use crate::ui::theme_store::contrast_foreground;

const HEADER_HEIGHT: Pixels = px(38.);

/// 打开窗口时取得的发送卡快照，供“用输入框内容填充”使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickFillSource {
    pub text: String,
    pub hex: bool,
    pub line_ending: SendLineEnding,
}

pub fn quick_command_error_text(locale: Locale, err: QuickCommandError) -> SharedString {
    match err {
        QuickCommandError::EmptyName => tr_shared(locale, "quick.err_empty_name"),
        QuickCommandError::NameTooLong => tr_shared(locale, "quick.err_name_long"),
        QuickCommandError::GroupTooLong => tr_shared(locale, "quick.err_group_long"),
        QuickCommandError::EmptyContent => tr_shared(locale, "quick.err_empty_content"),
        QuickCommandError::ContentTooLong => tr_shared(locale, "quick.err_content_long"),
        QuickCommandError::InvalidHex => tr_shared(locale, "err.invalid_hex"),
        QuickCommandError::TooMany => tr_shared(locale, "quick.err_too_many"),
    }
}

/// 写入快捷命令列表并落盘。写盘失败时回滚内存，避免内存与磁盘不一致，
/// 重试保存也不会产生重复命令。
pub fn persist_quick_commands(
    store: &Entity<ConfigStore>,
    commands: Vec<QuickCommand>,
    cx: &mut App,
) -> Result<(), String> {
    let mut save_res = Ok(());
    store.update(cx, |store, cx| {
        let previous = store.config().quick_commands.clone();
        save_res = store.update(|cfg| cfg.quick_commands = commands);
        if save_res.is_err() {
            store.update_in_memory(|cfg| cfg.quick_commands = previous);
        }
        cx.notify();
    });
    save_res
}

/// 新建 / 编辑快捷命令的独立窗口。保存成功后自动关闭。
pub struct QuickCommandWindow {
    theme: NanoTheme,
    theme_manager: Entity<ThemeManager>,
    config_store: Entity<ConfigStore>,
    draft: QuickEditorDraft,
    name_input: Entity<InputState>,
    group_input: Entity<InputState>,
    content_input: Entity<TextareaState>,
    fill: QuickFillSource,
    error: Option<SharedString>,
    _theme_subscription: Subscription,
}

impl QuickCommandWindow {
    pub fn new(
        config_store: Entity<ConfigStore>,
        draft: QuickEditorDraft,
        fill: QuickFillSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let locale = config_store.read(cx).config().locale;
        let theme_manager = ThemeHandle::get(cx);
        let theme_subscription = cx.observe(&theme_manager, |_, _, cx| cx.notify());
        let theme = theme_manager.read(cx).nano();

        let name_ph = tr_shared(locale, "quick.ph_name");
        let group_ph = tr_shared(locale, "quick.ph_group");
        let content_ph = tr_shared(locale, "quick.ph_content");
        let name_val = draft.name.clone();
        let group_val = draft.group.clone();
        let content_val = draft.content.clone();

        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(name_ph));
        name_input.update(cx, |state, cx| state.set_value(name_val, window, cx));
        let group_input = cx.new(|cx| InputState::new(window, cx).placeholder(group_ph));
        group_input.update(cx, |state, cx| state.set_value(group_val, window, cx));
        let content_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(content_ph)
                .default_value(content_val)
        });

        Self {
            theme,
            theme_manager,
            config_store,
            draft,
            name_input,
            group_input,
            content_input,
            fill,
            error: None,
            _theme_subscription: theme_subscription,
        }
    }

    fn locale(&self, cx: &App) -> Locale {
        self.config_store.read(cx).config().locale
    }

    fn fill_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.hex = self.fill.hex;
        self.draft.line_ending = self.fill.line_ending;
        let text = self.fill.text.clone();
        self.content_input
            .update(cx, |state, cx| state.set_value(text, window, cx));
        self.error = None;
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let locale = self.locale(cx);
        self.draft.name = self.name_input.read(cx).value().to_string();
        self.draft.group = self.group_input.read(cx).value().to_string();
        self.draft.content = self.content_input.read(cx).value().to_string();

        let mut commands = self.config_store.read(cx).config().quick_commands.clone();
        if let Some(id) = &self.draft.editing_id
            && !commands.iter().any(|c| &c.id == id)
        {
            self.error = Some(tr_shared(locale, "quick.err_gone"));
            cx.notify();
            return;
        }
        let cmd = match self.draft.to_command(&commands) {
            Ok(cmd) => cmd,
            Err(err) => {
                self.error = Some(quick_command_error_text(locale, err));
                cx.notify();
                return;
            }
        };
        if let Err(err) = upsert_command(&mut commands, cmd) {
            self.error = Some(quick_command_error_text(locale, err));
            cx.notify();
            return;
        }
        if let Err(err) = persist_quick_commands(&self.config_store, commands, cx) {
            self.error = Some(quick_save_failed(locale, &err).into());
            cx.notify();
            return;
        }
        window.remove_window();
    }

    fn move_current(&mut self, delta: i32, cx: &mut Context<Self>) {
        let locale = self.locale(cx);
        let Some(id) = self.draft.editing_id.clone() else {
            return;
        };
        let mut commands = self.config_store.read(cx).config().quick_commands.clone();
        if !move_command(&mut commands, &id, delta) {
            return;
        }
        if let Err(err) = persist_quick_commands(&self.config_store, commands, cx) {
            self.error = Some(quick_save_failed(locale, &err).into());
        }
        cx.notify();
    }

    fn field_label(theme: NanoTheme, text: SharedString) -> impl IntoElement {
        div().text_xs().text_color(theme.text_muted).child(text)
    }

    fn bordered_field(theme: NanoTheme, child: impl IntoElement) -> Div {
        div()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border_card)
            .child(child)
    }

    /// 分段控件中的一段。`enabled = false` 时置灰且不可点。
    fn segment(
        theme: NanoTheme,
        id: &'static str,
        label: SharedString,
        active: bool,
        enabled: bool,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(26.))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .when(active && enabled, |this| {
                this.bg(theme.bg_toggle_active)
                    .text_color(theme.accent_blue_hover)
            })
            .when(!(active && enabled), |this| {
                this.text_color(if enabled {
                    theme.text_secondary
                } else {
                    theme.text_muted
                })
            })
            .when(enabled && !active, |this| {
                this.cursor_pointer()
                    .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_primary))
            })
            .child(label)
    }

    fn segmented(theme: NanoTheme) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border_card)
            .overflow_hidden()
    }
}

impl Render for QuickCommandWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.theme = self.theme_manager.read(cx).nano();
        let theme = self.theme;
        let locale = self.locale(cx);
        let editing = self.draft.editing_id.is_some();
        let hex = self.draft.hex;
        let line_ending = self.draft.line_ending;
        let checksum = self.draft.checksum;
        let title = if editing {
            tr_shared(locale, "quick.editor_title_edit")
        } else {
            tr_shared(locale, "quick.editor_title_new")
        };

        let move_button =
            |id: &'static str, icon: LucideIcon, tip_key: &'static str, delta: i32| {
                div()
                    .id(id)
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border_card)
                    .cursor_pointer()
                    .text_color(theme.text_secondary)
                    .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                    .tooltip(move |window, cx| {
                        Tooltip::new(tr_shared(locale, tip_key)).build(window, cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.move_current(delta, cx);
                        }),
                    )
                    .child(Icon::new(icon).size(px(14.)))
            };
        let le_segment = |id: &'static str, label: SharedString, value: SendLineEnding| {
            Self::segment(theme, id, label, line_ending == value, !hex).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if !this.draft.hex {
                        this.draft.line_ending = value;
                        cx.notify();
                    }
                }),
            )
        };

        v_flex()
            .id("quick-command-window-root")
            .relative()
            .size_full()
            .bg(theme.bg_canvas)
            .text_color(theme.text_primary)
            .on_key_down(|event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") {
                    cx.stop_propagation();
                    window.remove_window();
                }
            })
            .child(
                h_flex()
                    .id("quick-command-header")
                    .flex_shrink_0()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .items_center()
                    .px_5()
                    .bg(theme.bg_strip)
                    .window_control_area(WindowControlArea::Drag)
                    .text_sm()
                    .child(title),
            )
            .child(
                v_flex()
                    .id("quick-command-form")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .px_5()
                    .pt_4()
                    .pb_3()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(Self::field_label(
                                theme,
                                tr_shared(locale, "quick.editor_name"),
                            ))
                            .child(Self::bordered_field(
                                theme,
                                Input::new(&self.name_input).appearance(false),
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(Self::field_label(
                                theme,
                                tr_shared(locale, "quick.editor_group"),
                            ))
                            .child(Self::bordered_field(
                                theme,
                                Input::new(&self.group_input).appearance(false),
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .child(Self::field_label(
                                        theme,
                                        tr_shared(locale, "quick.editor_content"),
                                    ))
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id("quick-editor-fill")
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(px(4.))
                                            .cursor_pointer()
                                            .text_xs()
                                            .text_color(theme.accent_blue_hover)
                                            .hover(move |s| s.bg(theme.bg_hover))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.fill_from_input(window, cx);
                                                }),
                                            )
                                            .child(tr_shared(locale, "quick.fill_from_input")),
                                    ),
                            )
                            .child(Self::bordered_field(
                                theme,
                                Textarea::new(&self.content_input)
                                    .appearance(false)
                                    .bordered(false)
                                    .h(px(120.)),
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_4()
                            .items_end()
                            .child(
                                v_flex()
                                    .gap_1p5()
                                    .child(Self::field_label(
                                        theme,
                                        tr_shared(locale, "quick.editor_mode"),
                                    ))
                                    .child(
                                        Self::segmented(theme)
                                            .child(
                                                Self::segment(
                                                    theme,
                                                    "quick-mode-text",
                                                    tr_shared(locale, "quick.editor_mode_text"),
                                                    !hex,
                                                    true,
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.draft.hex = false;
                                                        cx.notify();
                                                    }),
                                                ),
                                            )
                                            .child(
                                                Self::segment(
                                                    theme,
                                                    "quick-mode-hex",
                                                    tr_shared(locale, "quick.editor_hex"),
                                                    hex,
                                                    true,
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.draft.hex = true;
                                                        cx.notify();
                                                    }),
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_1p5()
                                    .child(Self::field_label(
                                        theme,
                                        tr_shared(locale, "quick.editor_line_ending"),
                                    ))
                                    .child(
                                        Self::segmented(theme)
                                            .child(le_segment(
                                                "quick-le-none",
                                                tr_shared(locale, "flow.none"),
                                                SendLineEnding::None,
                                            ))
                                            .child(le_segment(
                                                "quick-le-crlf",
                                                "\\r\\n".into(),
                                                SendLineEnding::CrLf,
                                            ))
                                            .child(le_segment(
                                                "quick-le-lf",
                                                "\\n".into(),
                                                SendLineEnding::Lf,
                                            )),
                                    ),
                            )
                            .child(div().flex_1())
                            .when(editing, |this| {
                                this.child(move_button(
                                    "quick-editor-up",
                                    LucideIcon::ChevronUp,
                                    "quick.move_up",
                                    -1,
                                ))
                                .child(move_button(
                                    "quick-editor-down",
                                    LucideIcon::ChevronDown,
                                    "quick.move_down",
                                    1,
                                ))
                            }),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(Self::field_label(
                                theme,
                                tr_shared(locale, "quick.editor_checksum"),
                            ))
                            .child(Self::segmented(theme).children(
                                Checksum::ALL.into_iter().enumerate().map(|(idx, algo)| {
                                    let label: SharedString = if algo == Checksum::None {
                                        tr_shared(locale, "flow.none")
                                    } else {
                                        algo.label().into()
                                    };
                                    let ids = [
                                        "quick-cs-none",
                                        "quick-cs-sum8",
                                        "quick-cs-xor8",
                                        "quick-cs-crc8",
                                        "quick-cs-crc16-modbus",
                                        "quick-cs-crc16-ccitt",
                                        "quick-cs-crc32",
                                    ];
                                    Self::segment(theme, ids[idx], label, checksum == algo, hex)
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                if this.draft.hex {
                                                    this.draft.checksum = algo;
                                                    cx.notify();
                                                }
                                            }),
                                        )
                                }),
                            )),
                    )
                    .when_some(self.error.clone(), |this, err| {
                        this.child(div().text_xs().text_color(theme.error_text).child(err))
                    }),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .w_full()
                    .px_5()
                    .py_3()
                    .gap_2()
                    .items_center()
                    .justify_end()
                    .border_t_1()
                    .border_color(theme.border_card)
                    .child(
                        div()
                            .id("quick-editor-cancel")
                            .h(px(30.))
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme.border_card)
                            .cursor_pointer()
                            .text_sm()
                            .text_color(theme.text_secondary)
                            .hover(move |s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                cx.stop_propagation();
                                window.remove_window();
                            })
                            .child(tr_shared(locale, "quick.cancel")),
                    )
                    .child(
                        div()
                            .id("quick-editor-save")
                            .h(px(30.))
                            .px_4()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .bg(theme.accent_blue)
                            .cursor_pointer()
                            .text_sm()
                            .text_color(contrast_foreground(theme.accent_blue))
                            .hover(move |s| s.bg(theme.accent_blue_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.save(window, cx);
                                }),
                            )
                            .child(tr_shared(locale, "quick.save")),
                    ),
            )
            // Transparent TitleBar overlay: draws the minimize/maximize/close controls.
            .child(
                div()
                    .id("quick-command-title-overlay")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(HEADER_HEIGHT)
                    .occlude()
                    .cursor_default()
                    .child(
                        TitleBar::new()
                            .bg(gpui_kit::transparent_black())
                            .border_b_0(),
                    ),
            )
    }
}
