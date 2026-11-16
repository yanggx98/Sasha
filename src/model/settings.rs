use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Default configuration file name stored in the application root directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Current schema version of [`AppConfig`].
pub const CURRENT_CONFIG_VERSION: u32 = 1;

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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_CONFIG_VERSION,
        }
    }
}

/// Manages loading, migrating, and atomically saving [`AppConfig`].
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    config: AppConfig,
    last_load_warning: Option<String>,
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
            };
        }

        let raw = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                return Self {
                    path,
                    config: AppConfig::default(),
                    last_load_warning: Some(format!("Failed to read config file: {err}")),
                };
            }
        };

        match toml::from_str::<AppConfig>(&raw) {
            Ok(mut config) => {
                migrate_config(&mut config);
                Self {
                    path,
                    config,
                    last_load_warning: None,
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

    /// Mutates the configuration in memory and atomically persists it to disk.
    #[allow(dead_code)]
    pub fn update(&mut self, mutate: impl FnOnce(&mut AppConfig)) -> Result<(), String> {
        mutate(&mut self.config);
        self.save()
    }

    /// Atomically writes the current configuration to `self.path`.
    #[allow(dead_code)]
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
}
