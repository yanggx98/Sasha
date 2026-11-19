use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::i18n::Locale;

use super::quick_command::{MAX_QUICK_COMMANDS, QuickCommand};
use super::timed_send::{DEFAULT_TIMER_INTERVAL_MS, clamp_interval_ms};

/// Default configuration file name stored in the application root directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Current schema version of [`AppConfig`].
pub const CURRENT_CONFIG_VERSION: u32 = 1;

/// Default monospace font size (in logical pixels) for the receive log view.
pub const DEFAULT_LOG_FONT_SIZE: u16 = 14;

/// Default receive-log payload budget, in bytes (1 MiB).
pub const DEFAULT_LOG_BUFFER_LIMIT: u32 = 1024 * 1024;

/// Smallest receive-log payload budget the bottom bar accepts, in bytes (4 KiB).
pub const MIN_LOG_BUFFER_LIMIT: u32 = 4 * 1024;

/// Largest receive-log payload budget the bottom bar accepts, in bytes (64 MiB).
pub const MAX_LOG_BUFFER_LIMIT: u32 = 64 * 1024 * 1024;

/// Preset receive-log payload budgets shown in the bottom-bar picker, in bytes.
pub const LOG_BUFFER_LIMIT_PRESETS: [u32; 5] = [
    64 * 1024,
    256 * 1024,
    1024 * 1024,
    4 * 1024 * 1024,
    16 * 1024 * 1024,
];

/// Preset font sizes exposed in the Settings View tab.
pub const LOG_FONT_SIZE_PRESETS: [u16; 4] = [12, 13, 14, 15];

/// Default background image opacity percentage (`0..=100`).
pub const DEFAULT_BACKGROUND_OPACITY: u8 = 25;

/// Default (built-in dark) theme id.
pub const DEFAULT_THEME_ID: &str = "dark";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamOutputScope {
    CurrentBufferAndFuture,
    #[default]
    FutureOnly,
}

/// Global application configuration persisted as `config.toml`.
///
/// Session/tab states are intentionally excluded in this phase.
/// Future global preferences (e.g. `locale`, `theme`) can be added here with
/// `#[serde(default)]` to preserve backward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Configuration schema version for forward-compatible migrations.
    pub version: u32,
    /// UI display language.
    pub locale: Locale,
    /// Monospace font size (px) used in the log view.
    pub log_font_size: u16,
    /// Maximum receive-log payload retained per session, in bytes.
    #[serde(default = "default_log_buffer_limit")]
    pub log_buffer_limit: u32,
    /// Applies when the next stream capture is attached to its session.
    pub stream_output_scope: StreamOutputScope,
    /// Persisted Lua script paths shown in the script panel.
    pub scripts: Vec<PathBuf>,
    /// Optional custom background image path for the main workspace.
    pub background_image: Option<PathBuf>,
    /// Background image opacity percentage (`0..=100`).
    #[serde(default = "default_background_opacity")]
    pub background_opacity: u8,
    /// Selected theme id (`dark`, `light`, or a file stem in the `themes` directory).
    #[serde(default = "default_theme_id")]
    pub theme: String,
    /// Saved quick-send commands shared across sessions.
    #[serde(default, deserialize_with = "lenient_quick_commands")]
    pub quick_commands: Vec<QuickCommand>,
    /// Last timed-send interval (ms) used in the send card.
    #[serde(default = "default_timer_interval_ms")]
    pub timer_interval_ms: u32,
    /// Check GitHub Releases for a newer version once at startup.
    #[serde(default = "default_auto_check_updates")]
    pub auto_check_updates: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_CONFIG_VERSION,
            locale: Locale::default(),
            log_font_size: DEFAULT_LOG_FONT_SIZE,
            log_buffer_limit: DEFAULT_LOG_BUFFER_LIMIT,
            stream_output_scope: StreamOutputScope::default(),
            scripts: Vec::new(),
            background_image: None,
            background_opacity: DEFAULT_BACKGROUND_OPACITY,
            theme: DEFAULT_THEME_ID.to_string(),
            quick_commands: Vec::new(),
            timer_interval_ms: DEFAULT_TIMER_INTERVAL_MS,
            auto_check_updates: true,
        }
    }
}

fn lenient_quick_commands<'de, D>(deserializer: D) -> Result<Vec<QuickCommand>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<toml::Value>::deserialize(deserializer)?;
    let mut result = Vec::new();
    let mut seen_ids = HashSet::new();
    for value in values {
        if result.len() >= MAX_QUICK_COMMANDS {
            break;
        }
        let Ok(cmd) = value.try_into::<QuickCommand>() else {
            continue;
        };
        if cmd.id.trim().is_empty() || cmd.validate().is_err() || !seen_ids.insert(cmd.id.clone()) {
            continue;
        }
        result.push(cmd);
    }
    Ok(result)
}

fn default_log_buffer_limit() -> u32 {
    DEFAULT_LOG_BUFFER_LIMIT
}

fn default_auto_check_updates() -> bool {
    true
}

fn default_timer_interval_ms() -> u32 {
    DEFAULT_TIMER_INTERVAL_MS
}

fn default_background_opacity() -> u8 {
    DEFAULT_BACKGROUND_OPACITY
}

fn default_theme_id() -> String {
    DEFAULT_THEME_ID.to_string()
}

/// Formats a live usage figure as `512 B`, `1.5 KB`, or `1 MB`.
pub fn format_buffer_usage(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;
    if bytes >= MB {
        format_scaled_bytes(bytes, MB, "MB")
    } else if bytes >= KB {
        format_scaled_bytes(bytes, KB, "KB")
    } else {
        format!("{bytes} B")
    }
}

fn format_scaled_bytes(bytes: u64, unit: u64, suffix: &str) -> String {
    if bytes.is_multiple_of(unit) {
        return format!("{} {suffix}", bytes / unit);
    }
    let tenths = (bytes * 10) / unit;
    let whole = tenths / 10;
    let frac = tenths % 10;
    if frac == 0 {
        format!("{whole} {suffix}")
    } else {
        format!("{whole}.{frac} {suffix}")
    }
}

/// Formats a byte budget as `64 KB`, `1 MB`, or a raw byte count.
pub fn format_log_buffer_limit(bytes: u32) -> String {
    const MB: u32 = 1024 * 1024;
    const KB: u32 = 1024;
    if bytes >= MB && bytes.is_multiple_of(MB) {
        format!("{} MB", bytes / MB)
    } else if bytes >= KB && bytes.is_multiple_of(KB) {
        format!("{} KB", bytes / KB)
    } else {
        bytes.to_string()
    }
}

/// Parses a bottom-bar buffer limit in bytes.
///
/// Accepts a plain byte count (`1048576`) or a 1024-based suffix (`64 KB`, `1 MB`).
/// Rejects blank input and values outside
/// [`MIN_LOG_BUFFER_LIMIT`]..=[`MAX_LOG_BUFFER_LIMIT`].
pub fn parse_log_buffer_limit_for(
    raw: &str,
    locale: crate::i18n::Locale,
) -> Result<u32, std::borrow::Cow<'static, str>> {
    let compact: String = raw
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let (number, multiplier) = if let Some(number) = compact.strip_suffix("mb") {
        (number, 1024 * 1024)
    } else if let Some(number) = compact.strip_suffix("kb") {
        (number, 1024)
    } else if let Some(number) = compact.strip_suffix('m') {
        (number, 1024 * 1024)
    } else if let Some(number) = compact.strip_suffix('k') {
        (number, 1024)
    } else {
        let number = compact.strip_suffix('b').unwrap_or(compact.as_str());
        (number, 1)
    };
    let bytes = number
        .parse::<u32>()
        .ok()
        .and_then(|count| count.checked_mul(multiplier));
    match bytes {
        Some(value) if (MIN_LOG_BUFFER_LIMIT..=MAX_LOG_BUFFER_LIMIT).contains(&value) => Ok(value),
        _ => Err(crate::i18n::err_buffer_limit(
            locale,
            &format_log_buffer_limit(MIN_LOG_BUFFER_LIMIT),
            &format_log_buffer_limit(MAX_LOG_BUFFER_LIMIT),
        )
        .into()),
    }
}

/// Manages loading, migrating, and atomically saving [`AppConfig`].
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    config: AppConfig,
    last_load_warning: Option<String>,
    quick_commands_skipped: usize,
}

impl ConfigStore {
    /// Resolves `config.toml` in the application root directory (executable
    /// directory, falling back to current working directory) and loads it.
    pub fn load_default() -> Self {
        Self::load_from_path(default_config_path())
    }

    /// Loads configuration from an explicit file path.
    ///
    /// - Missing file: falls back to [`AppConfig::default()`] without error.
    /// - Corrupted TOML: backs up the file to `<name>.bak` and falls back to default.
    pub fn load_from_path(path: PathBuf) -> Self {
        if !path.exists() {
            return Self {
                path,
                config: AppConfig::default(),
                last_load_warning: None,
                quick_commands_skipped: 0,
            };
        }

        let raw = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                return Self {
                    path,
                    config: AppConfig::default(),
                    last_load_warning: Some(format!("Failed to read config file: {err}")),
                    quick_commands_skipped: 0,
                };
            }
        };

        match toml::from_str::<AppConfig>(&raw) {
            Ok(mut config) => {
                migrate_config(&mut config);
                let raw_count = toml::from_str::<toml::Table>(&raw)
                    .ok()
                    .and_then(|t| {
                        t.get("quick_commands")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len())
                    })
                    .unwrap_or(0);
                let skipped = raw_count.saturating_sub(config.quick_commands.len());
                let warning = if skipped > 0 {
                    Some(format!("Skipped {skipped} invalid quick command(s)"))
                } else {
                    None
                };
                Self {
                    path,
                    config,
                    last_load_warning: warning,
                    quick_commands_skipped: skipped,
                }
            }
            Err(err) => {
                let backup_note = match backup_corrupted_file(&path) {
                    Ok(backup_path) => format!("backed up to {}", backup_path.display()),
                    Err(backup_err) => format!("backup failed: {backup_err}"),
                };
                Self {
                    path,
                    config: AppConfig::default(),
                    last_load_warning: Some(format!(
                        "Corrupted config file ({err}); {backup_note}"
                    )),
                    quick_commands_skipped: 0,
                }
            }
        }
    }

    /// Returns the immutable current configuration.
    #[allow(dead_code)]
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Returns the resolved configuration file path.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a non-fatal warning captured during the initial load, if any.
    #[allow(dead_code)]
    pub fn last_load_warning(&self) -> Option<&str> {
        self.last_load_warning.as_deref()
    }

    /// Returns the number of invalid/duplicate quick commands skipped on load.
    pub fn skipped_quick_commands(&self) -> usize {
        self.quick_commands_skipped
    }

    /// Mutates the configuration in memory without writing to disk.
    pub fn update_in_memory(&mut self, mutate: impl FnOnce(&mut AppConfig)) {
        let prev_quick_commands = self.config.quick_commands.clone();
        mutate(&mut self.config);
        migrate_config(&mut self.config);
        if self.config.quick_commands != prev_quick_commands {
            self.quick_commands_skipped = 0;
        }
    }

    /// Mutates the configuration in memory and atomically persists it to disk.
    pub fn update(&mut self, mutate: impl FnOnce(&mut AppConfig)) -> Result<(), String> {
        self.update_in_memory(mutate);
        self.save()
    }

    /// Resets the configuration to [`AppConfig::default()`] and persists it to disk.
    pub fn reset_to_default(&mut self) -> Result<(), String> {
        self.config = AppConfig::default();
        self.quick_commands_skipped = 0;
        self.save()
    }

    /// Atomically writes the current configuration to `self.path`.
    pub fn save(&self) -> Result<(), String> {
        write_config_atomically(&self.path, &self.config)
    }
}

/// Resolves the `config.toml` path in the application root directory.
pub fn default_config_path() -> PathBuf {
    let base_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    base_dir.join(CONFIG_FILE_NAME)
}

fn migrate_config(config: &mut AppConfig) {
    if config.version == 0 || config.version < CURRENT_CONFIG_VERSION {
        config.version = CURRENT_CONFIG_VERSION;
    }
    config.log_font_size = config.log_font_size.clamp(10, 24);
    if !(MIN_LOG_BUFFER_LIMIT..=MAX_LOG_BUFFER_LIMIT).contains(&config.log_buffer_limit) {
        config.log_buffer_limit = DEFAULT_LOG_BUFFER_LIMIT;
    }
    let mut unique_scripts = Vec::with_capacity(config.scripts.len());
    for path in config.scripts.drain(..) {
        if !path.as_os_str().is_empty() && !unique_scripts.contains(&path) {
            unique_scripts.push(path);
        }
    }
    config.scripts = unique_scripts;
    if config
        .background_image
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        config.background_image = None;
    }
    config.background_opacity = config.background_opacity.min(100);
    config.timer_interval_ms = clamp_interval_ms(config.timer_interval_ms);
    let trimmed = config.theme.trim();
    config.theme = if trimmed.is_empty() {
        DEFAULT_THEME_ID.to_string()
    } else {
        trimmed.to_string()
    };
}

fn backup_corrupted_file(path: &Path) -> std::io::Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(CONFIG_FILE_NAME);
    let backup_path = path.with_file_name(format!("{file_name}.bak"));
    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }
    fs::rename(path, &backup_path)?;
    Ok(backup_path)
}

fn write_config_atomically(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create config directory: {err}"))?;
    }

    let serialized = toml::to_string_pretty(config)
        .map_err(|err| format!("Failed to serialize config: {err}"))?;

    let tmp_path = temp_path_for(path);
    {
        let mut file = fs::File::create(&tmp_path)
            .map_err(|err| format!("Failed to create temporary config file: {err}"))?;
        file.write_all(serialized.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|err| {
                let _ = fs::remove_file(&tmp_path);
                format!("Failed to write temporary config file: {err}")
            })?;
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to replace config file: {err}"));
    }

    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(CONFIG_FILE_NAME);
    let pid = std::process::id();
    path.with_file_name(format!("{file_name}.tmp.{pid}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir().join(format!(
                "sasha_settings_test_{label}_{}_{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&dir).expect("create temp test dir");
            Self { path: dir }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn missing_config_uses_defaults_without_creating_file() {
        let dir = TempDir::new("missing");
        let cfg_path = dir.file(CONFIG_FILE_NAME);

        let store = ConfigStore::load_from_path(cfg_path.clone());
        assert_eq!(store.config(), &AppConfig::default());
        assert_eq!(store.config().version, CURRENT_CONFIG_VERSION);
        assert!(store.last_load_warning().is_none());
        assert!(!cfg_path.exists());
    }

    #[test]
    fn save_and_reload_roundtrip_leaves_no_temp_files() {
        let dir = TempDir::new("roundtrip");
        let cfg_path = dir.file(CONFIG_FILE_NAME);

        let mut store = ConfigStore::load_from_path(cfg_path.clone());
        store
            .update(|cfg| {
                cfg.version = CURRENT_CONFIG_VERSION;
            })
            .expect("save config");

        assert!(cfg_path.exists());
        let raw = fs::read_to_string(&cfg_path).expect("read saved toml");
        assert!(raw.contains("version = 1"));

        // Ensure no leftover temporary files remain in the directory.
        let entries: Vec<_> = fs::read_dir(&dir.path)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec![CONFIG_FILE_NAME.to_string()]);

        let reloaded = ConfigStore::load_from_path(cfg_path);
        assert_eq!(reloaded.config(), store.config());
        assert!(reloaded.last_load_warning().is_none());
    }

    #[test]
    fn stream_scope_defaults_roundtrips_and_resets() {
        let dir = TempDir::new("stream-scope");
        let path = dir.file(CONFIG_FILE_NAME);
        fs::write(&path, "version = 1\nlocale = \"zh-CN\"\n").unwrap();
        let mut store = ConfigStore::load_from_path(path.clone());
        assert_eq!(
            store.config().stream_output_scope,
            StreamOutputScope::FutureOnly
        );
        for scope in [
            StreamOutputScope::CurrentBufferAndFuture,
            StreamOutputScope::FutureOnly,
        ] {
            store.update(|cfg| cfg.stream_output_scope = scope).unwrap();
            let reloaded = ConfigStore::load_from_path(path.clone());
            assert_eq!(reloaded.config().stream_output_scope, scope);
        }
        store
            .update(|cfg| cfg.stream_output_scope = StreamOutputScope::CurrentBufferAndFuture)
            .unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("stream_output_scope = \"current_buffer_and_future\"")
        );
        store.reset_to_default().unwrap();
        assert_eq!(
            ConfigStore::load_from_path(path)
                .config()
                .stream_output_scope,
            StreamOutputScope::FutureOnly
        );
    }

    #[test]
    fn missing_buffer_limit_stays_at_default_and_out_of_range_is_clamped() {
        let dir = TempDir::new("buffer-default");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        fs::write(&cfg_path, "version = 1\nlocale = \"zh-CN\"\n").expect("write partial");

        let store = ConfigStore::load_from_path(cfg_path);
        assert_eq!(store.config().log_buffer_limit, DEFAULT_LOG_BUFFER_LIMIT);

        let dir = TempDir::new("buffer-clamp");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        fs::write(&cfg_path, "log_buffer_limit = 1\n").expect("write tiny limit");
        let store = ConfigStore::load_from_path(cfg_path);
        assert_eq!(store.config().log_buffer_limit, DEFAULT_LOG_BUFFER_LIMIT);

        assert_eq!(
            parse_log_buffer_limit_for("1 MB", Locale::ZhCn),
            Ok(1024 * 1024)
        );
        assert_eq!(
            parse_log_buffer_limit_for("64KB", Locale::EnUs),
            Ok(64 * 1024)
        );
        assert!(parse_log_buffer_limit_for("0", Locale::EnUs).is_err());
        assert!(parse_log_buffer_limit_for("999999999", Locale::ZhCn).is_err());
        assert_eq!(format_buffer_usage(0), "0 B");
        assert_eq!(format_buffer_usage(1536), "1.5 KB");
        assert_eq!(format_buffer_usage(1024 * 1024), "1 MB");
    }

    #[test]
    fn empty_or_partial_config_fills_defaults_and_ignores_unknown_fields() {
        let dir = TempDir::new("partial");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        fs::write(&cfg_path, "# empty user config\nfuture_flag = true\n").expect("write partial");

        let store = ConfigStore::load_from_path(cfg_path);
        assert_eq!(store.config().version, CURRENT_CONFIG_VERSION);
        assert!(store.last_load_warning().is_none());
    }

    #[test]
    fn corrupted_config_backs_up_and_falls_back_to_default() {
        let dir = TempDir::new("corrupted");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        let invalid_toml = "version = [unclosed";
        fs::write(&cfg_path, invalid_toml).expect("write invalid toml");

        let store = ConfigStore::load_from_path(cfg_path.clone());
        assert_eq!(store.config(), &AppConfig::default());
        assert!(store.last_load_warning().is_some());
        assert!(!cfg_path.exists());

        let backup_path = dir.file("config.toml.bak");
        assert!(backup_path.exists());
        assert_eq!(
            fs::read_to_string(backup_path).expect("read backup"),
            invalid_toml
        );
    }

    #[test]
    fn default_config_path_ends_with_config_toml() {
        let path = default_config_path();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some(CONFIG_FILE_NAME)
        );
    }

    #[test]
    fn scripts_roundtrip_and_deduplication() {
        let dir = TempDir::new("scripts");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        let script_a = dir.file("a.lua");
        let script_b = dir.file("b.lua");

        let mut store = ConfigStore::load_from_path(cfg_path.clone());
        store
            .update(|cfg| {
                cfg.scripts = vec![
                    script_a.clone(),
                    script_b.clone(),
                    script_a.clone(),
                    PathBuf::new(),
                ];
            })
            .expect("save scripts");

        assert_eq!(
            store.config().scripts,
            vec![script_a.clone(), script_b.clone()]
        );

        let reloaded = ConfigStore::load_from_path(cfg_path);
        assert_eq!(reloaded.config().scripts, vec![script_a, script_b]);
    }

    #[test]
    fn background_settings_roundtrip_and_clamp() {
        let dir = TempDir::new("background");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        let bg_path = dir.file("wallpaper.png");

        let mut store = ConfigStore::load_from_path(cfg_path.clone());
        assert_eq!(store.config().background_image, None);
        assert_eq!(
            store.config().background_opacity,
            DEFAULT_BACKGROUND_OPACITY
        );

        store
            .update(|cfg| {
                cfg.background_image = Some(bg_path.clone());
                cfg.background_opacity = 180;
            })
            .expect("save background");

        assert_eq!(store.config().background_image, Some(bg_path.clone()));
        assert_eq!(store.config().background_opacity, 100);

        let reloaded = ConfigStore::load_from_path(cfg_path);
        assert_eq!(reloaded.config().background_image, Some(bg_path));
        assert_eq!(reloaded.config().background_opacity, 100);
    }

    #[test]
    fn theme_defaults_roundtrips_and_normalizes() {
        let dir = TempDir::new("theme");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        fs::write(
            &cfg_path,
            "log_font_size = 14
",
        )
        .unwrap();
        let mut store = ConfigStore::load_from_path(cfg_path.clone());
        assert_eq!(store.config().theme, "dark");

        store
            .update(|cfg| cfg.theme = "  ocean ".to_string())
            .unwrap();
        assert_eq!(store.config().theme, "ocean");
        let reloaded = ConfigStore::load_from_path(cfg_path);
        assert_eq!(reloaded.config().theme, "ocean");

        store.update_in_memory(|cfg| cfg.theme = "   ".to_string());
        assert_eq!(store.config().theme, "dark");
    }

    #[test]
    fn config_roundtrip_preserves_content_exactly() {
        use crate::model::SendLineEnding;

        let dir = TempDir::new("quick-roundtrip");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        let mut store = ConfigStore::load_from_path(cfg_path.clone());
        let cmd = QuickCommand {
            id: "qc-1".to_string(),
            name: "Exact".to_string(),
            group: "G1".to_string(),
            content: " a\r\nb 你好 ".to_string(),
            hex: false,
            line_ending: SendLineEnding::CrLf,
            checksum: Default::default(),
        };
        store
            .update(|cfg| cfg.quick_commands = vec![cmd.clone()])
            .expect("save quick commands");

        let reloaded = ConfigStore::load_from_path(cfg_path);
        assert_eq!(reloaded.config().quick_commands, vec![cmd]);
        assert_eq!(reloaded.skipped_quick_commands(), 0);
    }

    #[test]
    fn config_skips_invalid_and_duplicate_commands() {
        let dir = TempDir::new("quick-skip");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        let toml_text = r#"
version = 1
locale = "en-US"
log_font_size = 15

[[quick_commands]]
id = "c1"
name = "First"
content = "AT"

[[quick_commands]]
id = "bad"
name = "MissingContent"

[[quick_commands]]
id = "c1"
name = "DuplicateId"
content = "AT+RST"

[[quick_commands]]
id = "c2"
name = "Second"
content = "OK"
"#;
        fs::write(&cfg_path, toml_text).expect("write toml");

        let store = ConfigStore::load_from_path(cfg_path);
        assert_eq!(store.config().locale, Locale::EnUs);
        assert_eq!(store.config().log_font_size, 15);
        assert_eq!(store.config().quick_commands.len(), 2);
        assert_eq!(store.config().quick_commands[0].id, "c1");
        assert_eq!(store.config().quick_commands[1].id, "c2");
        assert_eq!(store.skipped_quick_commands(), 2);
        assert!(store.last_load_warning().is_some());
    }

    #[test]
    fn old_config_without_quick_commands_loads() {
        let dir = TempDir::new("quick-old");
        let cfg_path = dir.file(CONFIG_FILE_NAME);
        fs::write(&cfg_path, "version = 1\nlocale = \"zh-CN\"\n").expect("write old toml");

        let store = ConfigStore::load_from_path(cfg_path);
        assert!(store.config().quick_commands.is_empty());
        assert_eq!(store.skipped_quick_commands(), 0);
        assert!(store.last_load_warning().is_none());
    }

    #[test]
    fn timer_interval_defaults_roundtrips_and_clamps() {
        let dir = TempDir::new("timer-interval");
        let path = dir.file(CONFIG_FILE_NAME);
        fs::write(
            &path,
            "version = 1
",
        )
        .unwrap();
        let mut store = ConfigStore::load_from_path(path.clone());
        assert_eq!(store.config().timer_interval_ms, DEFAULT_TIMER_INTERVAL_MS);

        store.update(|cfg| cfg.timer_interval_ms = 250).unwrap();
        let reloaded = ConfigStore::load_from_path(path.clone());
        assert_eq!(reloaded.config().timer_interval_ms, 250);

        fs::write(
            &path,
            "version = 1
timer_interval_ms = 1
",
        )
        .unwrap();
        let clamped = ConfigStore::load_from_path(path);
        assert_eq!(clamped.config().timer_interval_ms, 10);
    }

    #[test]
    fn auto_check_updates_defaults_on_and_roundtrips() {
        let dir = TempDir::new("auto-check-updates");
        let path = dir.file(CONFIG_FILE_NAME);
        fs::write(&path, "version = 1\n").unwrap();
        let mut store = ConfigStore::load_from_path(path.clone());
        assert!(store.config().auto_check_updates);

        store.update(|cfg| cfg.auto_check_updates = false).unwrap();
        assert!(
            !ConfigStore::load_from_path(path)
                .config()
                .auto_check_updates
        );
    }
}
