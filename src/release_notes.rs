use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const PENDING_RELEASE_NOTES_PATH: &str = "release-notes.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNotes {
    pub version: String,
    pub body: String,
    pub preview: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredReleaseNotes {
    version: String,
    body: String,
}

pub fn pending_path() -> PathBuf {
    let mut path = crate::config::config_path();
    path.set_file_name(PENDING_RELEASE_NOTES_PATH);
    path
}

pub fn save_pending(version: &str, body: &str) -> std::io::Result<()> {
    save_pending_to_path(&pending_path(), version, body)
}

fn save_pending_to_path(path: &Path, version: &str, body: &str) -> std::io::Result<()> {
    let body = normalize_body(body);
    if body.is_empty() {
        return clear_pending_at(path);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let stored = StoredReleaseNotes {
        version: version.to_string(),
        body,
    };
    let json = serde_json::to_string_pretty(&stored).map_err(std::io::Error::other)?;
    let tmp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&tmp_path, json)?;
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

pub fn load_pending_for_current_version() -> Option<ReleaseNotes> {
    load_pending_from_path(&pending_path(), env!("CARGO_PKG_VERSION"))
}

fn load_pending_from_path(path: &Path, current_version: &str) -> Option<ReleaseNotes> {
    let content = fs::read_to_string(path).ok()?;
    let stored: StoredReleaseNotes = serde_json::from_str(&content).ok()?;
    if stored.version != current_version {
        return None;
    }

    let body = normalize_body(&stored.body);
    if body.is_empty() {
        return None;
    }

    Some(ReleaseNotes {
        version: stored.version,
        body,
        preview: false,
    })
}

pub fn clear_pending() -> std::io::Result<()> {
    clear_pending_at(&pending_path())
}

fn clear_pending_at(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        fs::remove_file(path)
    } else {
        Ok(())
    }
}

pub fn load_preview_from_local_changelog(version: &str) -> Option<ReleaseNotes> {
    let path = Path::new("CHANGELOG.md");
    let content = fs::read_to_string(path).ok()?;
    let body = extract_version_section(&content, version)?;
    Some(ReleaseNotes {
        version: version.to_string(),
        body: normalize_body(&body),
        preview: true,
    })
}

fn extract_version_section(content: &str, version: &str) -> Option<String> {
    let header = format!("## [{version}]");
    let mut collecting = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if !collecting {
            if line.starts_with(&header) {
                collecting = true;
            }
            continue;
        }

        if line.starts_with("## [") {
            break;
        }

        lines.push(line);
    }

    let body = lines.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

pub fn normalize_body(body: &str) -> String {
    body.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_version_section() {
        let changelog = "# Changelog\n\n## [0.2.3] - 2026-03-31\n\n### Changed\n- One\n\n## [0.2.2] - 2026-03-30\n\n### Fixed\n- Two\n";
        assert_eq!(
            extract_version_section(changelog, "0.2.3").as_deref(),
            Some("### Changed\n- One")
        );
    }

    #[test]
    fn preserves_headings() {
        assert_eq!(
            normalize_body("### Changed\n- One\n\n### Fixed\n- Two"),
            "### Changed\n- One\n\n### Fixed\n- Two"
        );
    }
}
