//! Version checking and in-app updating against GitHub Releases.
//!
//! Everything here is blocking and UI-agnostic; callers run it on a background
//! executor. Updating is deliberately conservative:
//!
//! * only assets served from this repository's release downloads are accepted;
//! * a SHA-256 checksum must be published for the asset and must match;
//! * the running executable is replaced by renaming (Windows allows renaming a
//!   running exe but not overwriting it), with a rollback if the swap fails.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const RELEASES_API: &str = "https://api.github.com/repos/yanggx98/Sasha/releases?per_page=20";
const DOWNLOAD_PREFIX: &str = "https://github.com/yanggx98/Sasha/releases/download/";
const MAX_ASSET_BYTES: u64 = 200 * 1024 * 1024;
const MAX_API_BYTES: u64 = 4 * 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const OLD_SUFFIX: &str = "old";
const NEW_SUFFIX: &str = "new";

/// Asset file name a release must contain for this platform, if self-update is
/// supported here.
pub fn platform_asset_name() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("sasha-windows-x86_64.exe")
    } else {
        None
    }
}

/// Asset name of the `sasha-cli` companion program for this platform.
pub fn platform_cli_asset_name() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("sasha-cli-windows-x86_64.exe")
    } else {
        None
    }
}

/// Where `sasha-cli` lives relative to the running app executable.
fn cli_exe_for(exe: &Path) -> PathBuf {
    exe.with_file_name(if cfg!(windows) {
        "sasha-cli.exe"
    } else {
        "sasha-cli"
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256, when GitHub published a digest for the asset.
    pub sha256: Option<String>,
    /// URL of a `<name>.sha256` sidecar, used when `sha256` is `None`.
    pub checksum_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: Version,
    pub tag: String,
    /// Human-facing release page (notes).
    pub page_url: String,
    /// Installable asset for this platform; `None` means "open the release page".
    pub asset: Option<AssetInfo>,
    /// The `sasha-cli` companion for this platform, when the release ships a verifiable one.
    pub cli_asset: Option<AssetInfo>,
}

#[derive(Debug)]
pub enum UpdateError {
    Network(String),
    Parse(String),
    Verify(String),
    Io(String),
    Unsupported,
    /// The repository has no published (non-draft, semver-tagged) release yet.
    NoRelease,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(msg) | Self::Parse(msg) | Self::Verify(msg) | Self::Io(msg) => {
                f.write_str(msg)
            }
            Self::Unsupported => f.write_str("self-update is not supported on this platform"),
            Self::NoRelease => f.write_str("no published release found"),
        }
    }
}

impl From<io::Error> for UpdateError {
    fn from(err: io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

/// Parses a release tag such as `v0.1.0` or `0.0.1-b`.
pub fn parse_version(tag: &str) -> Option<Version> {
    let tag = tag.trim();
    let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    Version::parse(tag).ok()
}

/// The version this binary was built as.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package version is valid semver")
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

fn normalize_digest(digest: &str) -> Option<String> {
    let hex = digest.trim().strip_prefix("sha256:")?;
    is_sha256_hex(hex).then(|| hex.to_ascii_lowercase())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parses a `sha256sum`-style line (`<hex>  <name>`) or a bare hex digest.
pub fn parse_checksum_file(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?;
    is_sha256_hex(first).then(|| first.to_ascii_lowercase())
}

/// Picks the highest-versioned non-draft release from the GitHub API response.
/// Pre-releases are included because this project ships pre-release builds.
pub fn parse_latest_release(
    json: &str,
    asset_name: Option<&str>,
) -> Result<ReleaseInfo, UpdateError> {
    parse_latest_release_with_cli(json, asset_name, None)
}

/// Like [`parse_latest_release`], also looking for the `sasha-cli` companion asset.
pub fn parse_latest_release_with_cli(
    json: &str,
    asset_name: Option<&str>,
    cli_asset_name: Option<&str>,
) -> Result<ReleaseInfo, UpdateError> {
    let raw: Vec<RawRelease> =
        serde_json::from_str(json).map_err(|err| UpdateError::Parse(err.to_string()))?;
    let (version, release) = raw
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| parse_version(&release.tag_name).map(|v| (v, release)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .ok_or(UpdateError::NoRelease)?;

    let asset = asset_name.and_then(|wanted| find_verifiable_asset(&release.assets, wanted));
    let cli_asset =
        cli_asset_name.and_then(|wanted| find_verifiable_asset(&release.assets, wanted));

    Ok(ReleaseInfo {
        version,
        tag: release.tag_name,
        page_url: release.html_url,
        asset,
        cli_asset,
    })
}

fn find_verifiable_asset(assets: &[RawAsset], wanted: &str) -> Option<AssetInfo> {
    let found = assets.iter().find(|a| a.name == wanted)?;
    let sha256 = found.digest.as_deref().and_then(normalize_digest);
    // A sibling `<name>.sha256` asset is the fallback when GitHub published no digest.
    let checksum_url = assets
        .iter()
        .find(|a| a.name == format!("{wanted}.sha256"))
        .map(|a| a.browser_download_url.clone());
    if sha256.is_none() && checksum_url.is_none() {
        // Never offer an install that cannot be verified.
        return None;
    }
    Some(AssetInfo {
        name: found.name.clone(),
        url: found.browser_download_url.clone(),
        size: found.size,
        sha256,
        checksum_url,
    })
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(concat!("Sasha/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Fetches the newest release. Returns it even when it is not newer; use
/// [`is_newer`] to decide.
pub fn fetch_latest() -> Result<ReleaseInfo, UpdateError> {
    let mut response = agent(CHECK_TIMEOUT)
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|err| UpdateError::Network(err.to_string()))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_API_BYTES)
        .read_to_string()
        .map_err(|err| UpdateError::Network(err.to_string()))?;
    parse_latest_release_with_cli(&body, platform_asset_name(), platform_cli_asset_name())
}

pub fn is_newer(latest: &Version, current: &Version) -> bool {
    latest > current
}

fn is_allowed_download_url(url: &str) -> bool {
    url.starts_with(DOWNLOAD_PREFIX) && !url.contains("..")
}

/// Downloads `asset` next to the running executable and verifies its checksum.
/// Returns the path of the verified file, ready for [`install`].
pub fn download(asset: &AssetInfo) -> Result<PathBuf, UpdateError> {
    if platform_asset_name().is_none() {
        return Err(UpdateError::Unsupported);
    }
    let target = sibling_path(&std::env::current_exe()?, NEW_SUFFIX);
    download_verified(asset, &target).map(|()| target)
}

/// Downloads `asset` to `target`, checking its checksum; removes `target` on failure.
fn download_verified(asset: &AssetInfo, target: &Path) -> Result<(), UpdateError> {
    if !is_allowed_download_url(&asset.url) {
        return Err(UpdateError::Verify("unexpected download location".into()));
    }
    let expected = match (&asset.sha256, &asset.checksum_url) {
        (Some(hex), _) => hex.clone(),
        (None, Some(url)) => fetch_sidecar_checksum(url)?,
        (None, None) => return Err(UpdateError::Verify("no checksum published".into())),
    };

    let result = download_to(&asset.url, target, &expected);
    if result.is_err() {
        let _ = fs::remove_file(target);
    }
    result
}

/// Updates `sasha-cli` next to the app when it is installed there. A missing companion is
/// not an error; callers should treat failures as non-fatal for the app update itself.
pub fn update_cli(asset: &AssetInfo) -> Result<(), UpdateError> {
    let cli = cli_exe_for(&std::env::current_exe()?);
    if !cli.exists() {
        return Ok(());
    }
    let new = sibling_path(&cli, NEW_SUFFIX);
    download_verified(asset, &new)?;
    swap_files(&cli, &new, &sibling_path(&cli, OLD_SUFFIX)).inspect_err(|_| {
        let _ = fs::remove_file(&new);
    })
}

fn fetch_sidecar_checksum(url: &str) -> Result<String, UpdateError> {
    if !is_allowed_download_url(url) {
        return Err(UpdateError::Verify("unexpected checksum location".into()));
    }
    let mut response = agent(CHECK_TIMEOUT)
        .get(url)
        .call()
        .map_err(|err| UpdateError::Network(err.to_string()))?;
    let text = response
        .body_mut()
        .with_config()
        .limit(4096)
        .read_to_string()
        .map_err(|err| UpdateError::Network(err.to_string()))?;
    parse_checksum_file(&text).ok_or_else(|| UpdateError::Verify("malformed checksum file".into()))
}

fn download_to(url: &str, target: &Path, expected_sha256: &str) -> Result<(), UpdateError> {
    let response = agent(DOWNLOAD_TIMEOUT)
        .get(url)
        .call()
        .map_err(|err| UpdateError::Network(err.to_string()))?;
    let mut reader = response.into_body().into_reader().take(MAX_ASSET_BYTES + 1);
    let mut file = File::create(target)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|err| UpdateError::Network(err.to_string()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_ASSET_BYTES {
            return Err(UpdateError::Verify("download is unexpectedly large".into()));
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
    }
    file.flush()?;
    drop(file);

    let actual = hex_lower(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(UpdateError::Verify("checksum mismatch".into()));
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// `<exe>.<suffix>` next to the executable (e.g. `sasha.exe.new`).
fn sibling_path(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    exe.with_file_name(name)
}

/// Swaps `downloaded` in as the running executable: current -> `.old`,
/// `downloaded` -> current, restoring the original if the second step fails.
pub fn install(downloaded: &Path) -> Result<(), UpdateError> {
    let exe = std::env::current_exe()?;
    swap_files(&exe, downloaded, &sibling_path(&exe, OLD_SUFFIX))
}

fn swap_files(exe: &Path, new: &Path, old: &Path) -> Result<(), UpdateError> {
    let _ = fs::remove_file(old);
    fs::rename(exe, old)?;
    if let Err(err) = fs::rename(new, exe) {
        // Put the original back so the app is never left without an executable.
        let _ = fs::rename(old, exe);
        return Err(err.into());
    }
    Ok(())
}

/// Starts the freshly installed executable. The caller should quit afterwards.
pub fn restart() -> Result<(), UpdateError> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe).spawn()?;
    Ok(())
}

/// Removes files left behind by a previous update. Call once at startup.
pub fn cleanup_leftovers() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(sibling_path(&exe, OLD_SUFFIX));
        let _ = fs::remove_file(sibling_path(&exe, NEW_SUFFIX));
        let cli = cli_exe_for(&exe);
        let _ = fs::remove_file(sibling_path(&cli, OLD_SUFFIX));
        let _ = fs::remove_file(sibling_path(&cli, NEW_SUFFIX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn release_json(extra_assets: &str, digest: Option<&str>) -> String {
        let digest = digest
            .map(|d| format!(r#","digest":"{d}""#))
            .unwrap_or_default();
        format!(
            r#"[
              {{"tag_name":"v0.0.1-b","html_url":"https://example/old","assets":[]}},
              {{"tag_name":"v0.2.0","html_url":"https://example/new","assets":[
                {{"name":"sasha-windows-x86_64.exe","browser_download_url":"{DOWNLOAD_PREFIX}v0.2.0/sasha-windows-x86_64.exe","size":10{digest}}}
                {extra_assets}
              ]}},
              {{"tag_name":"v9.9.9","html_url":"https://example/draft","draft":true,"assets":[]}},
              {{"tag_name":"nightly","html_url":"https://example/nightly","assets":[]}}
            ]"#
        )
    }

    #[test]
    fn parses_versions_with_and_without_prefix() {
        assert_eq!(parse_version("v1.2.3"), Version::parse("1.2.3").ok());
        assert_eq!(parse_version("0.0.1-b"), Version::parse("0.0.1-b").ok());
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn prerelease_orders_before_release() {
        let beta = parse_version("0.0.1-b").unwrap();
        let stable = parse_version("0.0.1").unwrap();
        assert!(is_newer(&stable, &beta));
        assert!(!is_newer(&beta, &stable));
        assert!(!is_newer(&beta, &beta));
    }

    #[test]
    fn picks_highest_non_draft_release() {
        let json = release_json("", Some(DIGEST));
        let release = parse_latest_release(&json, Some("sasha-windows-x86_64.exe")).unwrap();
        assert_eq!(release.version, Version::parse("0.2.0").unwrap());
        assert_eq!(release.page_url, "https://example/new");
        let asset = release.asset.unwrap();
        assert_eq!(asset.sha256.as_deref(), Some(&DIGEST["sha256:".len()..]));
    }

    #[test]
    fn asset_without_any_checksum_is_not_installable() {
        let json = release_json("", None);
        let release = parse_latest_release(&json, Some("sasha-windows-x86_64.exe")).unwrap();
        assert!(release.asset.is_none());
    }

    #[test]
    fn sidecar_checksum_asset_makes_release_installable() {
        let sidecar = format!(
            r#",{{"name":"sasha-windows-x86_64.exe.sha256","browser_download_url":"{DOWNLOAD_PREFIX}v0.2.0/sasha-windows-x86_64.exe.sha256","size":90}}"#
        );
        let json = release_json(&sidecar, None);
        let release = parse_latest_release(&json, Some("sasha-windows-x86_64.exe")).unwrap();
        let asset = release.asset.unwrap();
        assert_eq!(asset.sha256, None);
        assert!(asset.checksum_url.unwrap().ends_with(".exe.sha256"));
    }

    #[test]
    fn no_platform_asset_still_reports_release() {
        let json = release_json("", Some(DIGEST));
        let release = parse_latest_release(&json, None).unwrap();
        assert!(release.asset.is_none());
        assert_eq!(release.tag, "v0.2.0");
    }

    #[test]
    fn empty_or_invalid_response_is_an_error() {
        assert!(matches!(
            parse_latest_release("[]", None),
            Err(UpdateError::NoRelease)
        ));
        assert!(parse_latest_release("not json", None).is_err());
    }

    #[test]
    fn checksum_file_formats() {
        let hex = "ab".repeat(32);
        assert_eq!(
            parse_checksum_file(&format!("{hex}  sasha.exe\n")),
            Some(hex.clone())
        );
        assert_eq!(parse_checksum_file(&hex.to_uppercase()), Some(hex));
        assert_eq!(parse_checksum_file("zz"), None);
        assert_eq!(normalize_digest("md5:abc"), None);
    }

    #[test]
    fn download_urls_must_belong_to_the_repo() {
        assert!(is_allowed_download_url(&format!(
            "{DOWNLOAD_PREFIX}v1/a.exe"
        )));
        assert!(!is_allowed_download_url("https://evil.example/a.exe"));
        assert!(!is_allowed_download_url(&format!("{DOWNLOAD_PREFIX}../x")));
        assert!(!is_allowed_download_url(
            "http://github.com/yanggx98/Sasha/releases/download/a"
        ));
    }

    #[test]
    fn swap_replaces_exe_and_keeps_old_copy() {
        let dir = std::env::temp_dir().join(format!("sasha-swap-ok-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (exe, new, old) = (dir.join("app"), dir.join("app.new"), dir.join("app.old"));
        fs::write(&exe, "v1").unwrap();
        fs::write(&new, "v2").unwrap();
        swap_files(&exe, &new, &old).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "v2");
        assert_eq!(fs::read_to_string(&old).unwrap(), "v1");
        assert!(!new.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_swap_restores_original() {
        let dir = std::env::temp_dir().join(format!("sasha-swap-fail-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (exe, new, old) = (dir.join("app"), dir.join("missing"), dir.join("app.old"));
        fs::write(&exe, "v1").unwrap();
        assert!(swap_files(&exe, &new, &old).is_err());
        assert_eq!(fs::read_to_string(&exe).unwrap(), "v1");
        let _ = fs::remove_dir_all(&dir);
    }

    const CLI_ASSET: &str = "sasha-cli-windows-x86_64.exe";

    fn cli_assets(with_sidecar: bool) -> String {
        let mut extra = format!(
            r#",{{"name":"{CLI_ASSET}","browser_download_url":"{DOWNLOAD_PREFIX}v0.2.0/{CLI_ASSET}","size":20,"digest":"{DIGEST}"}}"#
        );
        if with_sidecar {
            extra.push_str(&format!(
                r#",{{"name":"{CLI_ASSET}.sha256","browser_download_url":"{DOWNLOAD_PREFIX}v0.2.0/{CLI_ASSET}.sha256","size":90}}"#
            ));
        }
        extra
    }

    #[test]
    fn release_with_cli_asset_exposes_it() {
        let json = release_json(&cli_assets(false), Some(DIGEST));
        let release =
            parse_latest_release_with_cli(&json, Some("sasha-windows-x86_64.exe"), Some(CLI_ASSET))
                .unwrap();
        let cli = release.cli_asset.expect("cli asset");
        assert_eq!(cli.name, CLI_ASSET);
        assert_eq!(cli.sha256.as_deref(), Some(&DIGEST["sha256:".len()..]));
        assert_eq!(release.asset.unwrap().name, "sasha-windows-x86_64.exe");
    }

    #[test]
    fn release_without_cli_asset_still_installable() {
        let json = release_json("", Some(DIGEST));
        let release =
            parse_latest_release_with_cli(&json, Some("sasha-windows-x86_64.exe"), Some(CLI_ASSET))
                .unwrap();
        assert!(release.cli_asset.is_none());
        assert!(release.asset.is_some());
    }

    #[test]
    fn cli_asset_without_checksum_is_ignored() {
        let extra = format!(
            r#",{{"name":"{CLI_ASSET}","browser_download_url":"{DOWNLOAD_PREFIX}v0.2.0/{CLI_ASSET}","size":20}}"#
        );
        let json = release_json(&extra, Some(DIGEST));
        let release =
            parse_latest_release_with_cli(&json, Some("sasha-windows-x86_64.exe"), Some(CLI_ASSET))
                .unwrap();
        assert!(release.cli_asset.is_none());
        assert!(release.asset.is_some());
    }

    #[test]
    fn cli_exe_sits_next_to_the_app() {
        let exe = Path::new("C:/tools/sasha.exe");
        let cli = cli_exe_for(exe);
        assert_eq!(cli.parent(), exe.parent());
        let name = cli.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("sasha-cli"), "name was {name}");
    }
}
