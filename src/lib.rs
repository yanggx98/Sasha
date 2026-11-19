rust_i18n::i18n!("locales", fallback = "zh-CN");

pub mod checksum;
mod i18n;
pub mod model;
pub mod scripting;
pub mod serial;
pub mod session_service;
mod ui;
mod update;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

use crate::model::ConfigStore;
use crate::ui::SashaApp;
use crate::ui::theme_manager::ThemeManager;
use crate::ui::update_manager::UpdateManager;

pub fn run_gui() {
    update::cleanup_leftovers();
    application().with_assets(assets::AllAssets).run(|cx| {
        init(cx);
        ui::fonts::register(cx);
        let config_store = cx.new(|_| ConfigStore::load_default());
        // Applies the configured theme (default: built-in dark) to the global gpui
        // `Theme`, including the native-red window-close danger colors.
        ThemeManager::init(config_store.clone(), cx);
        UpdateManager::init(&config_store, cx);

        let bounds = Bounds::centered(None, size(px(1000.), px(660.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(560.), px(400.))),
            is_resizable: true,
            ..TitleBar::window_options()
        };

        open_window(options, cx, move |_, cx| {
            let store = config_store.clone();
            cx.new(move |cx| SashaApp::new(store, cx))
        })
        .expect("Failed to open Sasha window");
    });
}
