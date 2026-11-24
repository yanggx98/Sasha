use gpui_kit::Hsla;

/// Color tokens for Sasha's dark, immersive single-session UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NanoTheme {
    /// Canvas shared by the immersive title bar and the log area.
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
    /// Tag color of the lines Lua scripts print (`output.print`).
    pub script_tag: Hsla,
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
        super::theme_store::builtin_dark().theme
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
