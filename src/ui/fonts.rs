//! Bundled monospace font so log and value text looks identical on every platform.

use std::borrow::Cow;

use gpui_kit::App;

/// Family name of the bundled monospace font (JetBrains Mono, SIL OFL 1.1).
pub const MONO_FONT: &str = "JetBrains Mono";

const REGULAR: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");

/// Registers the bundled fonts with GPUI. Call once at startup, before any window opens.
pub fn register(cx: &App) {
    let fonts = vec![Cow::Borrowed(REGULAR), Cow::Borrowed(BOLD)];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("failed to register bundled fonts: {err}");
    }
}
