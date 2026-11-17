rust_i18n::i18n!("locales", fallback = "zh-CN");

mod i18n;
mod model;
mod scripting;
mod serial;
mod session_service;
mod ui;

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;

use crate::model::ConfigStore;
use crate::ui::SashaApp;

fn main() {
    application().with_assets(assets::AllAssets).run(|cx| {
        init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        {
            // Window-close hover uses the global danger colors; use the native
            // Windows-style deep red with white glyph instead of the default salmon.
            let theme = Theme::global_mut(cx);
            theme.danger = rgb(0xC42B1C).into();
            theme.danger_active = rgb(0xA52619).into();
            theme.danger_foreground = white();
        }

        let bounds = Bounds::centered(None, size(px(1000.), px(660.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(560.), px(400.))),
            is_resizable: true,
            ..TitleBar::window_options()
        };

        let config_store = cx.new(|_| ConfigStore::load_default());
        open_window(options, cx, move |_, cx| {
            let store = config_store.clone();
            cx.new(move |cx| SashaApp::new(store, cx))
        })
        .expect("Failed to open Sasha window");
    });
}
