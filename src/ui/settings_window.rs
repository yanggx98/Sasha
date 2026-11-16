use std::sync::{Arc, OnceLock};

use gpui_kit::assets::IconName as LucideIcon;
use gpui_kit::component::{Icon, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{self, Locale, tr, tr_shared};
use crate::model::ConfigStore;
use crate::model::settings::LOG_FONT_SIZE_PRESETS;
use crate::ui::theme::NanoTheme;

const HEADER_HEIGHT: Pixels = px(38.);
const APP_AUTHOR: &str = "yanggx98";
const GITHUB_URL: &str = "https://github.com/yanggx98/Sasha";

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../assets/icons/sasha-arrows-transparent.png").to_vec(),
        ))
    })
    .clone()
}

/// Top-level tabs inside [`SettingsWindow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    View,
    About,
}

impl SettingsTab {
    const ALL: [Self; 3] = [Self::General, Self::View, Self::About];

    fn id(self) -> &'static str {
        match self {
            Self::General => "settings-tab-general",
            Self::View => "settings-tab-view",
            Self::About => "settings-tab-about",
        }
    }

    fn label(self, locale: Locale) -> SharedString {
        match self {
            Self::General => tr_shared(locale, "settings.tab_general"),
            Self::View => tr_shared(locale, "settings.tab_view"),
            Self::About => tr_shared(locale, "settings.tab_about"),
        }
    }

    fn subtitle(self, locale: Locale) -> SharedString {
        match self {
            Self::General => tr_shared(locale, "settings.tab_general_sub"),
            Self::View => tr_shared(locale, "settings.tab_view_sub"),
            Self::About => tr_shared(locale, "settings.tab_about_sub"),
        }
    }

    fn icon(self) -> LucideIcon {
        match self {
            Self::General => LucideIcon::Settings,
            Self::View => LucideIcon::Eye,
            Self::About => LucideIcon::Info,
        }
    }
}

/// Independent settings window with Windows Terminal-style top tabs extending
/// into the transparent titlebar and flat Zed-style setting rows.
pub struct SettingsWindow {
    theme: NanoTheme,
    config_store: Entity<ConfigStore>,
    active_tab: SettingsTab,
    save_error: Option<String>,
    _config_subscription: Subscription,
}

impl SettingsWindow {
    pub fn new(
        config_store: Entity<ConfigStore>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&config_store, |_, _, cx| cx.notify());
        Self {
            theme: NanoTheme::default(),
            config_store,
            active_tab: SettingsTab::General,
            save_error: None,
            _config_subscription: subscription,
        }
    }

    fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        if self.active_tab != tab {
            self.active_tab = tab;
            cx.notify();
        }
    }

    fn current_locale(&self, cx: &App) -> Locale {
        self.config_store.read(cx).config().locale
    }

    fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.update(|cfg| cfg.locale = locale);
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn set_log_font_size(&mut self, size: u16, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.update(|cfg| cfg.log_font_size = size);
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn reset_defaults(&mut self, cx: &mut Context<Self>) {
        let result = self.config_store.update(cx, |store, cx| {
            let res = store.reset_to_default();
            cx.notify();
            res
        });
        self.save_error = result.err();
        cx.notify();
    }

    fn copy_text(&self, text: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    }

    fn render_tab_button(&self, tab: SettingsTab, cx: &mut Context<Self>) -> AnyElement {
        let is_active = self.active_tab == tab;
        let theme = self.theme;
        let locale = self.current_locale(cx);

        h_flex()
            .id(tab.id())
            .min_w(px(108.))
            .px_3()
            .items_center()
            .gap_1p5()
            .text_xs()
            .cursor_pointer()
            // Active tab connects seamlessly to the content canvas below (Windows Terminal style).
            .when(is_active, |this| {
                this.h(px(32.))
                    .rounded_t(px(8.))
                    .bg(theme.bg_canvas)
                    .text_color(theme.text_primary)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!is_active, |this| {
                this.h(px(28.))
                    .mb(px(4.))
                    .rounded(px(6.))
                    .text_color(theme.text_secondary)
                    .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.select_tab(tab, cx);
                }),
            )
            .child(Icon::new(tab.icon()).size(px(12.)))
            .child(tab.label(locale))
            .into_any_element()
    }

    fn render_section_header(&self, tab: SettingsTab, locale: Locale) -> impl IntoElement {
        v_flex()
            .w_full()
            .pb_3()
            .gap_1()
            .border_b_1()
            .border_color(self.theme.border_card)
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.text_primary)
                    .child(tab.label(locale)),
            )
            .child(
                div()
                    .font_family("Consolas")
                    .text_xs()
                    .text_color(self.theme.text_muted)
                    .child(tab.subtitle(locale)),
            )
    }

    fn render_setting_row(
        &self,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        control: AnyElement,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .py_4()
            .items_center()
            .justify_between()
            .gap_6()
            .border_b_1()
            .border_color(self.theme.border_card)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.text_primary)
                            .child(title.into()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(self.theme.text_secondary)
                            .child(description.into()),
                    ),
            )
            .child(div().flex_shrink_0().child(control))
            .into_any_element()
    }

    fn flat_action_button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        div()
            .id(id)
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .text_xs()
            .text_color(theme.text_primary)
            .hover(|s| s.bg(theme.bg_hover))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    on_click(this, window, cx);
                }),
            )
            .child(label.into())
            .into_any_element()
    }

    fn render_general_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.current_locale(cx);
        let theme = self.theme;
        let path_text = self.config_store.read(cx).path().display().to_string();

        let language_picker = h_flex()
            .p_0p5()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .gap_0p5()
            .children(Locale::ALL.into_iter().enumerate().map(|(idx, candidate)| {
                let is_selected = locale == candidate;
                div()
                    .id(ElementId::named_usize("settings-locale", idx))
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.))
                    .text_xs()
                    .cursor_pointer()
                    .when(is_selected, |this| {
                        this.bg(theme.bg_toggle_active)
                            .text_color(theme.accent_blue_hover)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!is_selected, |this| {
                        this.text_color(theme.text_secondary)
                            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_locale(candidate, cx);
                        }),
                    )
                    .child(candidate.native_name())
            }))
            .into_any_element();

        let copy_btn = self.flat_action_button(
            "settings-copy-config-path",
            tr_shared(locale, "settings.copy_path"),
            |this, _, cx| {
                let path = this.config_store.read(cx).path().display().to_string();
                this.copy_text(&path, cx);
            },
            cx,
        );

        let reset_btn = self.flat_action_button(
            "settings-reset-defaults",
            tr_shared(locale, "settings.reset_btn"),
            |this, _, cx| this.reset_defaults(cx),
            cx,
        );

        v_flex()
            .w_full()
            .child(self.render_section_header(SettingsTab::General, locale))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.language_title"),
                tr_shared(locale, "settings.language_desc"),
                language_picker,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.config_path_title"),
                path_text,
                copy_btn,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.reset_title"),
                tr_shared(locale, "settings.reset_desc"),
                reset_btn,
            ))
    }

    fn render_view_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.current_locale(cx);
        let current_size = self.config_store.read(cx).config().log_font_size;
        let theme = self.theme;

        let font_size_picker = h_flex()
            .p_0p5()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .gap_0p5()
            .children(LOG_FONT_SIZE_PRESETS.into_iter().map(|preset| {
                let is_selected = current_size == preset;
                div()
                    .id(ElementId::named_usize(
                        "settings-font-size",
                        preset as usize,
                    ))
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.))
                    .text_xs()
                    .cursor_pointer()
                    .when(is_selected, |this| {
                        this.bg(theme.bg_toggle_active)
                            .text_color(theme.accent_blue_hover)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!is_selected, |this| {
                        this.text_color(theme.text_secondary)
                            .hover(|s| s.bg(theme.bg_hover).text_color(theme.text_primary))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_log_font_size(preset, cx);
                        }),
                    )
                    .child(format!("{preset}px"))
            }))
            .into_any_element();

        let theme_badge = div()
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(theme.bg_card)
            .text_xs()
            .text_color(theme.text_secondary)
            .child(tr(locale, "settings.theme_dark"))
            .into_any_element();

        v_flex()
            .w_full()
            .child(self.render_section_header(SettingsTab::View, locale))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.font_size_title"),
                tr_shared(locale, "settings.font_size_desc"),
                font_size_picker,
            ))
            .child(self.render_setting_row(
                tr_shared(locale, "settings.theme_title"),
                tr_shared(locale, "settings.theme_desc"),
                theme_badge,
            ))
    }

    fn render_about_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.current_locale(cx);
        let version = format!("v{}", env!("CARGO_PKG_VERSION"));
        let copy_github_btn = self.flat_action_button(
            "settings-copy-github",
            tr_shared(locale, "settings.copy_url"),
            |this, _, cx| this.copy_text(GITHUB_URL, cx),
            cx,
        );
        let open_github_btn = self.flat_action_button(
            "settings-open-github",
            tr_shared(locale, "settings.open_repo"),
            |_, _, cx| cx.open_url(GITHUB_URL),
            cx,
        );
        let github_actions = h_flex()
            .justify_center()
            .gap_1p5()
            .child(open_github_btn)
            .child(copy_github_btn)
            .into_any_element();

        v_flex()
            .w_full()
            .child(
                v_flex()
                    .id("settings-about-brand")
                    .w_full()
                    .items_center()
                    .pt_2()
                    .pb_5()
                    .gap_1()
                    .child(img(app_icon()).size(px(64.)).flex_shrink_0())
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.text_primary)
                            .child("Sasha"),
                    )
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_xs()
                            .text_color(self.theme.text_muted)
                            .child(version),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .pt_2()
                    .gap_3()
                    .text_xs()
                    .text_color(self.theme.text_muted)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(tr(locale, "settings.built_with"))
                            .child(self.about_project_link(
                                "about-gpui-kit",
                                "GPUI-KIT",
                                "https://github.com/longbridge/gpui-kit",
                            ))
                            .child("·")
                            .child(self.about_project_link(
                                "about-gpui",
                                "GPUI",
                                "https://github.com/zed-industries/zed/tree/main/crates/gpui",
                            )),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(tr(locale, "settings.ai_assisted"))
                            .child("DeepSeek · Grok · ChatGPT · Gemini"),
                    )
                    .child(
                        div().child(format!(
                            "{} {}",
                            tr(locale, "settings.created_by"),
                            APP_AUTHOR
                        )),
                    )
                    .child(div().mt_3().child(github_actions)),
            )
    }

    fn about_project_link(
        &self,
        id: &'static str,
        label: &'static str,
        url: &'static str,
    ) -> AnyElement {
        div()
            .id(id)
            .cursor_pointer()
            .text_color(self.theme.text_secondary)
            .hover(|style| style.text_color(self.theme.accent_blue_hover))
            .on_click(move |_, _, cx| cx.open_url(url))
            .child(label)
            .into_any_element()
    }

    fn build_top_bar(&self) -> impl IntoElement {
        div()
            .id("settings-top-bar-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(34.))
            .occlude()
            .cursor_default()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                gpui_kit::base::GlobalState::suppress_text_selection(cx);
            })
            .child(
                TitleBar::new()
                    .bg(gpui_kit::transparent_black())
                    .border_b_0(),
            )
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let locale = self.current_locale(cx);
        let save_error = self.save_error.clone();

        v_flex()
            .id("settings-window-root")
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
            // Recessed Windows Terminal-style tab strip extending to the top edge.
            .child(
                div()
                    .id("settings-header-surface")
                    .flex_shrink_0()
                    .w_full()
                    .h(HEADER_HEIGHT)
                    .bg(theme.bg_strip),
            )
            // Scrollable borderless settings content area (Zed-style flat rows).
            .child(
                v_flex()
                    .id("settings-content")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .px_6()
                    .py_5()
                    .when_some(save_error, |this, err| {
                        this.child(
                            div()
                                .w_full()
                                .mb_3()
                                .px_3()
                                .py_2()
                                .rounded(px(6.))
                                .bg(theme.error_bg)
                                .text_xs()
                                .text_color(theme.error_text)
                                .child(i18n::settings_save_error(locale, &err)),
                        )
                    })
                    .child(match self.active_tab {
                        SettingsTab::General => self.render_general_tab(cx).into_any_element(),
                        SettingsTab::View => self.render_view_tab(cx).into_any_element(),
                        SettingsTab::About => self.render_about_tab(cx).into_any_element(),
                    }),
            )
            // Transparent TitleBar overlay for window dragging and native window controls.
            .child(self.build_top_bar())
            // Interactive tabs aligned to the bottom of the header strip so the active tab
            // merges into the content canvas below.
            .child(
                h_flex()
                    .id("settings-top-tabs")
                    .absolute()
                    .top_0()
                    .left_0()
                    .right(px(140.))
                    .h(HEADER_HEIGHT)
                    .pl_2()
                    .pt(px(6.))
                    .items_end()
                    .gap_1()
                    .children(
                        SettingsTab::ALL
                            .into_iter()
                            .map(|tab| self.render_tab_button(tab, cx)),
                    ),
            )
    }
}
