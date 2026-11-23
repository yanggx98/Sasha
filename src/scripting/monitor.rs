//! Path rules and file writing behind the Lua `monitor.save` function.

use std::fs;
use std::path::{Path, PathBuf};

/// Lexically validates a script-supplied relative path.
///
/// Accepts `/` and `\` separators and ignores `.` segments. Rejects empty paths,
/// absolute / drive / UNC paths, `..` segments, `:` (drive letters, NTFS streams) and NUL.
/// Does not look at the platform's path parser so the result is the same everywhere.
pub(crate) fn validate_relative_path(raw: &str) -> Result<PathBuf, String> {
    if raw.trim().is_empty() {
        return Err("path must not be empty".to_string());
    }
    if raw.contains('\0') {
        return Err("path must not contain NUL".to_string());
    }
    if raw.contains(':') {
        return Err("path must not contain ':'".to_string());
    }
    if raw.starts_with(['/', '\\']) {
        return Err("path must be relative".to_string());
    }
    let mut out = PathBuf::new();
    for segment in raw.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => return Err("path must not contain '..'".to_string()),
            other => out.push(other),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("path must name a file".to_string());
    }
    Ok(out)
}

/// Writes `text` to `base/rel`, creating directories and overwriting an existing file.
/// Fails if the target's parent resolves (after symlinks) outside `base`.
pub(crate) fn save_text(base: &Path, rel: &Path, text: &str) -> Result<(), String> {
    fs::create_dir_all(base).map_err(|err| err.to_string())?;
    let canonical_base = fs::canonicalize(base).map_err(|err| err.to_string())?;
    let target = canonical_base.join(rel);
    let parent = target
        .parent()
        .ok_or_else(|| "path has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let canonical_parent = fs::canonicalize(parent).map_err(|err| err.to_string())?;
    if !canonical_parent.starts_with(&canonical_base) {
        return Err("path resolves outside the output directory".to_string());
    }
    fs::write(target, text.as_bytes()).map_err(|err| err.to_string())
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
            let path = std::env::temp_dir().join(format!(
                "sasha_monitor_test_{label}_{}_{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn components(path: &Path) -> Vec<String> {
        path.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn validate_accepts_plain_relative_paths() {
        for ok in ["a.txt", "sub/a.txt", "sub\\a.txt", "./a.txt"] {
            assert!(
                validate_relative_path(ok).is_ok(),
                "{ok:?} should be accepted"
            );
        }
        assert_eq!(
            components(&validate_relative_path("sub\\a.txt").unwrap()),
            components(&validate_relative_path("sub/a.txt").unwrap()),
        );
    }

    #[test]
    fn validate_rejects_escaping_and_malformed_paths() {
        for bad in [
            "",
            "   ",
            "/a",
            "\\a",
            "C:\\a",
            "C:a",
            "\\\\srv\\sh\\a",
            "..",
            "a/../b",
            "a/..",
            "a.txt:stream",
            "a\0b",
        ] {
            assert!(
                validate_relative_path(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn save_text_creates_parents_and_overwrites() {
        let tmp = TempDir::new("overwrite");
        let base = tmp.path.join("out");
        save_text(&base, Path::new("x/y/z.txt"), "one").unwrap();
        let target = base.join("x").join("y").join("z.txt");
        assert_eq!(fs::read_to_string(&target).unwrap(), "one");
        save_text(&base, Path::new("x/y/z.txt"), "two").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "two");
    }

    #[test]
    fn save_text_writes_empty_text() {
        let tmp = TempDir::new("empty");
        save_text(&tmp.path, Path::new("e.txt"), "").unwrap();
        assert_eq!(fs::metadata(tmp.path.join("e.txt")).unwrap().len(), 0);
    }

    #[test]
    fn save_text_errors_when_parent_is_a_file() {
        let tmp = TempDir::new("parentfile");
        fs::write(tmp.path.join("file.txt"), "x").unwrap();
        assert!(save_text(&tmp.path, Path::new("file.txt/x.txt"), "y").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn save_text_rejects_target_resolving_outside_base() {
        let tmp = TempDir::new("symlink");
        let base = tmp.path.join("base");
        let outside = tmp.path.join("outside");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("link")).unwrap();
        assert!(save_text(&base, Path::new("link/x.txt"), "y").is_err());
        assert!(!outside.join("x.txt").exists());
    }
}
