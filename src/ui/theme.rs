use gpui_kit::{Hsla, rgb, rgba};

/// Color tokens for Sasha's dark, immersive single-session UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NanoTheme {
    /// Canvas shared by the immersive title bar and the log area (`#0D1117`).
    pub bg_canvas: Hsla,
    /// Elevated surface for the floating send card.
    pub bg_card: Hsla,
    /// Recessed surface for the receive toolbar strip tucked under the send card.
    pub bg_strip: Hsla,
    /// Elevated surface for the top-center parameter picker popover.
    pub bg_popover: Hsla,
    /// Subtle hover background for interactive icon/text controls.
    pub bg_hover: Hsla,
    /// Subtle active background for enabled toggle icons.
    pub bg_toggle_active: Hsla,
    /// Subtle card border for light hierarchy.
    pub border_card: Hsla,
    /// Popover border for clear separation without a background scrim.
    pub border_popover: Hsla,
    /// Primary accent blue (send button, active highlights).
    pub accent_blue: Hsla,
    pub accent_blue_hover: Hsla,
    /// Log direction colors.
    pub rx_green: Hsla,
    pub tx_blue: Hsla,
    /// Text colors.
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    /// Validation error colors.
    pub error_text: Hsla,
    pub error_bg: Hsla,
    /// Simulated badge / status colors.
    pub sim_badge_bg: Hsla,
    pub sim_badge_text: Hsla,
    pub status_disconnected: Hsla,
    pub status_connected: Hsla,
}

impl Default for NanoTheme {
    fn default() -> Self {
        Self {
            bg_canvas: rgb(0x0D1117).into(),
            bg_card: rgb(0x161B22).into(),
            bg_strip: rgb(0x0A0E13).into(),
            bg_popover: rgb(0x1C2128).into(),
            bg_hover: rgba(0xFFFFFF12).into(),
            bg_toggle_active: rgba(0x2F81F724).into(),
            border_card: rgba(0xFFFFFF18).into(),
            border_popover: rgba(0xFFFFFF2A).into(),
            accent_blue: rgb(0x2563EB).into(),
            accent_blue_hover: rgb(0x3B82F6).into(),
            rx_green: rgb(0x3FB950).into(),
            tx_blue: rgb(0x58A6FF).into(),
            text_primary: rgb(0xE6EDF3).into(),
            text_secondary: rgb(0x9CA3AF).into(),
            text_muted: rgb(0x6E7681).into(),
            error_text: rgb(0xF85149).into(),
            error_bg: rgba(0xF851491E).into(),
            sim_badge_bg: rgba(0x388BFD1F).into(),
            sim_badge_text: rgb(0x79C0FF).into(),
            status_disconnected: rgb(0x8B949E).into(),
            status_connected: rgb(0x3FB950).into(),
        }
    }
}

impl NanoTheme {
    /// Hover color for an element that paints its own `base` background.
    /// `bg_hover` is a translucent overlay; using it as the hover background
    /// would replace `base` and let the workspace background image show
    /// through, so composite it over `base` instead.
    pub fn hover_over(&self, base: Hsla) -> Hsla {
        base.blend(self.bg_hover)
    }

    /// Returns a copy with surface backgrounds made slightly translucent so a
    /// workspace background image shows through without hurting readability.
    #[allow(dead_code)]
    pub fn with_translucent_surfaces(self) -> Self {
        self.with_surface_opacity(0.84)
    }

    /// Same as [`Self::with_translucent_surfaces`] with an explicit surface alpha.
    pub fn with_surface_opacity(mut self, alpha: f32) -> Self {
        self.bg_canvas = self.bg_canvas.opacity(alpha);
        self.bg_card = self.bg_card.opacity(alpha);
        self.bg_strip = self.bg_strip.opacity(alpha);
        self
    }
}
