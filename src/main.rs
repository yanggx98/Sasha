mod model;
mod serial;
mod ui;

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;

use crate::ui::SashaApp;

fn main() {
    application()
        .with_assets(assets::AllAssets)
        .run(|cx| {
            init(cx);
            Theme::change(ThemeMode::Dark, None, cx);

            let bounds = Bounds::centered(None, size(px(1000.), px(660.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(560.), px(400.))),
                is_resizable: true,
                ..TitleBar::window_options()
            };

            open_window(options, cx, |_, cx| cx.new(SashaApp::new))
                .expect("Failed to open Sasha window");
        });
}
