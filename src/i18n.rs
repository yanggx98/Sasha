use std::borrow::Cow;

use gpui_kit::SharedString;
use serde::{Deserialize, Serialize};

/// Supported UI locales backed by `locales/<code >.toml` via `rust-i18n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Locale {
    #[default]
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::ZhCn, Self::EnUs];

    /// Locale identifier matching `locales/<code>.toml`.
    pub fn code(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
        }
    }

    /// Native display name shown in language selectors regardless of current locale.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::EnUs => "English",
        }
    }
}

/// Looks up a translation key for `locale` from the compiled `locales/*.toml` tables.
pub fn tr(locale: Locale, key: &'static str) -> Cow<'static, str> {
    rust_i18n::t!(key, locale = locale.code())
}

/// Looks up a translation key as a GPUI [`SharedString`].
pub fn tr_shared(locale: Locale, key: &'static str) -> SharedString {
    tr(locale, key).into()
}

pub fn start_frame_header(locale: Locale, chevron: &str) -> String {
    rust_i18n::t!("start.frame_header", locale = locale.code(), chevron = chevron).into_owned()
}

pub fn picker_apply_value(locale: Locale, value: &str) -> String {
    rust_i18n::t!("picker.apply_value", locale = locale.code(), value = value).into_owned()
}

pub fn settings_save_error(locale: Locale, err: &str) -> String {
    rust_i18n::t!("settings.save_error", locale = locale.code(), err = err).into_owned()
}

pub fn serial_usb_mfg(locale: Locale, mfg: &str) -> String {
    rust_i18n::t!("serial.usb_mfg", locale = locale.code(), mfg = mfg).into_owned()
}

pub fn serial_usb_vid_pid(locale: Locale, vid: u16, pid: u16) -> String {
    let vid_hex = format!("{vid:04X}");
    let pid_hex = format!("{pid:04X}");
    rust_i18n::t!(
        "serial.usb_vid_pid",
        locale = locale.code(),
        vid = vid_hex,
        pid = pid_hex
    )
    .into_owned()
}

pub fn serial_err_invalid_input(locale: Locale, detail: &str) -> String {
    rust_i18n::t!(
        "serial.err_invalid_input",
        locale = locale.code(),
        detail = detail
    )
    .into_owned()
}

pub fn serial_format_error(locale: Locale, action: &str, port: &str, detail: &str) -> String {
    rust_i18n::t!(
        "serial.format_error",
        locale = locale.code(),
        action = action,
        port = port,
        detail = detail
    )
    .into_owned()
}

pub fn serial_action_open_failed(locale: Locale) -> String {
    tr(locale, "serial.action_open_failed").into_owned()
}

pub fn serial_action_clone_failed(locale: Locale) -> String {
    tr(locale, "serial.action_clone_failed").into_owned()
}

pub fn serial_action_spawn_failed(locale: Locale) -> String {
    tr(locale, "serial.action_spawn_failed").into_owned()
}

pub fn serial_action_write_failed(locale: Locale) -> String {
    tr(locale, "serial.action_write_failed").into_owned()
}

pub fn serial_action_flush_failed(locale: Locale) -> String {
    tr(locale, "serial.action_flush_failed").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_resolve_distinct_non_empty_strings_from_toml() {
        let zh_title = tr(Locale::ZhCn, "start.title");
        let en_title = tr(Locale::EnUs, "start.title");
        assert_eq!(zh_title, "连接串口设备");
        assert_eq!(en_title, "Connect Serial Port");

        assert_ne!(
            tr(Locale::ZhCn, "settings.tab_general"),
            tr(Locale::EnUs, "settings.tab_general")
        );
        assert_eq!(Locale::ZhCn.native_name(), "简体中文");
        assert_eq!(Locale::EnUs.native_name(), "English");
    }

    #[test]
    fn dynamic_helpers_format_for_each_locale() {
        assert_eq!(picker_apply_value(Locale::ZhCn, "9600"), "应用输入值: 9600");
        assert_eq!(picker_apply_value(Locale::EnUs, "9600"), "Apply value: 9600");
        assert_eq!(start_frame_header(Locale::ZhCn, "▸"), "▸ 帧格式（高级）");
        assert_eq!(
            start_frame_header(Locale::EnUs, "▾"),
            "▾ Frame Format (Advanced)"
        );
    }
}
