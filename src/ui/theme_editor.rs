//! State of the inline theme color editor shown in Settings > View.
//!
//! Rendering and actions live in `settings_window.rs`; this file owns the
//! eight color inputs, the name input and the draft parsing.

use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::ui::settings_window::SettingsWindow;
use crate::ui::theme_store::{BaseColors, ThemeBase, ThemeEntry, format_hex, parse_color};

/// Editor rows: (locale key of the label, getter into [`BaseColors`]).
pub const FIELDS: [(&str, fn(&BaseColors) -> Hsla); 8] = [
    ("settings.theme_edit_label_canvas", |c| c.canvas),
    ("settings.theme_edit_label_card", |c| c.card),
    ("settings.theme_edit_label_text", |c| c.text),
    ("settings.theme_edit_label_accent", |c| c.accent),
    ("settings.theme_edit_label_rx", |c| c.rx),
    ("settings.theme_edit_label_tx", |c| c.tx),
    ("settings.theme_edit_label_error", |c| c.error),
    ("settings.theme_edit_label_border", |c| c.border),
];

pub struct ThemeEditor {
    /// Id of the theme the editor started from.
    pub source_id: String,
    /// The source is a built-in: saving always creates a new theme.
    pub builtin: bool,
    pub base: ThemeBase,
    pub inputs: Vec<Entity<InputState>>,
    pub pickers: Vec<Entity<ColorPickerState>>,
    pub name_input: Entity<InputState>,
    /// The delete button was clicked once and awaits confirmation.
    pub delete_armed: bool,
    _subscriptions: Vec<Subscription>,
}

impl ThemeEditor {
    pub fn new(
        entry: &ThemeEntry,
        name_placeholder: SharedString,
        window: &mut Window,
        cx: &mut Context<SettingsWindow>,
    ) -> Self {
        let mut inputs = Vec::new();
        let mut pickers = Vec::new();
        let mut subscriptions = Vec::new();
        for (_, get) in FIELDS {
            let color = get(&entry.colors);
            let text = format_hex(color);
            let input = cx.new(|cx| InputState::new(window, cx));
            input.update(cx, |state, cx| state.set_value(text, window, cx));
            let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(color));

            // Text field -> picker (skipped when they already agree, which
            // avoids the slider/hex rounding feedback loop while dragging).
            let picker_for_input = picker.clone();
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let text = input.read(cx).value().to_string();
                        if let Ok(color) = parse_color(&text) {
                            let same = picker_for_input
                                .read(cx)
                                .value()
                                .is_some_and(|v| format_hex(v) == format_hex(color));
                            if !same {
                                picker_for_input
                                    .update(cx, |state, cx| state.set_value(color, window, cx));
                            }
                        }
                        this.theme_editor_changed(cx);
                    }
                },
            ));
            // Picker -> text field.
            let input_for_picker = input.clone();
            subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |this, _, event: &ColorPickerEvent, window, cx| {
                    let ColorPickerEvent::Change(Some(color)) = event else {
                        return;
                    };
                    let hex = format_hex(*color);
                    let current = input_for_picker.read(cx).value().to_string();
                    if parse_color(&current).map(format_hex).ok().as_deref() != Some(hex.as_str()) {
                        input_for_picker.update(cx, |state, cx| state.set_value(hex, window, cx));
                    }
                    this.theme_editor_changed(cx);
                },
            ));
            inputs.push(input);
            pickers.push(picker);
        }
        let name = if entry.builtin {
            String::new()
        } else {
            entry.name.clone()
        };
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(name_placeholder));
        name_input.update(cx, |state, cx| state.set_value(name, window, cx));
        Self {
            source_id: entry.id.clone(),
            builtin: entry.builtin,
            base: entry.base,
            inputs,
            pickers,
            name_input,
            delete_armed: false,
            _subscriptions: subscriptions,
        }
    }

    /// Parsed value of every row, in [`FIELDS`] order.
    pub fn parsed(&self, cx: &App) -> Vec<Result<Hsla, String>> {
        self.inputs
            .iter()
            .map(|input| parse_color(input.read(cx).value().as_ref()))
            .collect()
    }

    /// The draft colors, or `None` while any row is invalid.
    pub fn draft(&self, cx: &App) -> Option<BaseColors> {
        let c: Vec<Hsla> = self.parsed(cx).into_iter().collect::<Result<_, _>>().ok()?;
        Some(BaseColors {
            canvas: c[0],
            card: c[1],
            text: c[2],
            accent: c[3],
            rx: c[4],
            tx: c[5],
            error: c[6],
            border: c[7],
        })
    }

    pub fn name(&self, cx: &App) -> String {
        self.name_input.read(cx).value().trim().to_string()
    }
}
