use std::time::Duration;

use gpui_kit::base::animation::{EffectTransition, ease_out_cubic};
use gpui_kit::*;

/// Short-duration motion tokens (REQ-011: 120–180ms range).
pub const TOGGLE_TRANSITION_MS: u64 = 150;
pub const POPOVER_TRANSITION_MS: u64 = 150;
pub const STATUS_TRANSITION_MS: u64 = 160;

/// Linearly interpolates two `Hsla` colors in `Rgba` space to avoid
/// unintended hue-wheel sweeps between neutral and accented colors.
pub fn lerp_color(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    let a = Rgba::from(from);
    let b = Rgba::from(to);
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
    .into()
}

/// Returns a fully transparent color sharing the RGB/HSL chromaticity of `color`
/// so alpha fade-ins do not darken toward black.
pub fn transparent_like(mut color: Hsla) -> Hsla {
    color.a = 0.0;
    color
}

/// Tracks a boolean visual state together with a monotonic transition epoch.
/// When `epoch == 0` (initial mount), callers can render the static target state
/// without triggering a startup animation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransitionFlag {
    pub active: bool,
    pub epoch: u64,
}

impl TransitionFlag {
    pub fn new(active: bool) -> Self {
        Self { active, epoch: 0 }
    }

    /// Updates the flag and bumps `epoch` only if the boolean value changed.
    pub fn set(&mut self, active: bool) -> bool {
        if self.active != active {
            self.active = active;
            self.epoch += 1;
            true
        } else {
            false
        }
    }

    /// Returns `(from, to)` colors for the current transition state.
    pub fn color_pair(&self, inactive: Hsla, active: Hsla) -> (Hsla, Hsla) {
        if self.epoch == 0 {
            let current = if self.active { active } else { inactive };
            (current, current)
        } else if self.active {
            (inactive, active)
        } else {
            (active, inactive)
        }
    }
}

/// Shared UI transition epochs owned by `SashaApp`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UiMotionState {
    pub rx_hex: TransitionFlag,
    pub rx_timestamp: TransitionFlag,
    pub rx_pause: TransitionFlag,
    pub rx_auto_scroll: TransitionFlag,
    pub tx_display_hex: TransitionFlag,
    pub tx_send_hex: TransitionFlag,
    pub connection: TransitionFlag,
    pub picker_open_epoch: u64,
}

impl UiMotionState {
    pub fn new(
        rx_hex: bool,
        rx_timestamp: bool,
        rx_pause: bool,
        rx_auto_scroll: bool,
        tx_display_hex: bool,
        tx_send_hex: bool,
        connected: bool,
    ) -> Self {
        Self {
            rx_hex: TransitionFlag::new(rx_hex),
            rx_timestamp: TransitionFlag::new(rx_timestamp),
            rx_pause: TransitionFlag::new(rx_pause),
            rx_auto_scroll: TransitionFlag::new(rx_auto_scroll),
            tx_display_hex: TransitionFlag::new(tx_display_hex),
            tx_send_hex: TransitionFlag::new(tx_send_hex),
            connection: TransitionFlag::new(connected),
            picker_open_epoch: 0,
        }
    }
}

/// Applies a 150ms ease-out color transition when `flag.epoch > 0`, or sets
/// the static target colors directly on initial mount (`flag.epoch == 0`).
pub fn animate_toggle_colors<E>(
    element: E,
    id_prefix: &'static str,
    flag: TransitionFlag,
    duration_ms: u64,
    apply: impl Fn(E, f32) -> E + 'static,
) -> AnyElement
where
    E: IntoElement + 'static,
{
    if flag.epoch == 0 {
        apply(element, 1.0).into_any_element()
    } else {
        element
            .with_animation(
                ElementId::NamedInteger(id_prefix.into(), flag.epoch),
                Animation::new(Duration::from_millis(duration_ms)).with_easing(ease_out_cubic),
                apply,
            )
            .into_any_element()
    }
}

/// Lightweight enter transition for the top-center parameter picker popover:
/// 150ms ease-out fade-in paired with a 6px downward slide.
pub fn animate_popover_enter<E>(element: E, open_epoch: u64) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    EffectTransition::new(Duration::from_millis(POPOVER_TRANSITION_MS))
        .ease(ease_out_cubic)
        .slide_y(px(-6.), px(0.))
        .fade(0.0, 1.0)
        .apply(
            element,
            ElementId::NamedInteger("parameter-picker-enter".into(), open_epoch),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn transition_flag_only_bumps_epoch_on_actual_change() {
        let mut flag = TransitionFlag::new(false);
        assert_eq!(flag.epoch, 0);
        assert!(!flag.set(false));
        assert_eq!(flag.epoch, 0);

        assert!(flag.set(true));
        assert_eq!(flag.epoch, 1);
        assert!(!flag.set(true));
        assert_eq!(flag.epoch, 1);

        assert!(flag.set(false));
        assert_eq!(flag.epoch, 2);
    }

    #[core::prelude::v1::test]
    fn color_pair_is_static_on_epoch_zero_and_directional_after_toggle() {
        let off: Hsla = rgb(0x112233).into();
        let on: Hsla = rgb(0x445566).into();

        let mut flag = TransitionFlag::new(false);
        assert_eq!(flag.color_pair(off, on), (off, off));

        flag.set(true);
        assert_eq!(flag.color_pair(off, on), (off, on));

        flag.set(false);
        assert_eq!(flag.color_pair(off, on), (on, off));
    }

    #[core::prelude::v1::test]
    fn lerp_color_interpolates_rgba_endpoints_and_clamps() {
        let black: Hsla = rgba(0x000000FF).into();
        let white: Hsla = rgba(0xFFFFFFFF).into();

        let mid = Rgba::from(lerp_color(black, white, 0.5));
        assert!((mid.r - 0.5).abs() < 0.01);
        assert!((mid.g - 0.5).abs() < 0.01);
        assert!((mid.b - 0.5).abs() < 0.01);
        assert!((mid.a - 1.0).abs() < 0.01);

        let clamped_low = Rgba::from(lerp_color(black, white, -1.0));
        assert!(clamped_low.r.abs() < 0.01);
        let clamped_high = Rgba::from(lerp_color(black, white, 2.0));
        assert!((clamped_high.r - 1.0).abs() < 0.01);
    }
}
