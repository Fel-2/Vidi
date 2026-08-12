//! Release update check against the GitHub releases API.

const RELEASES_API: &str = "https://api.github.com/repos/Fel-2/Vidi/releases/latest";
const INSTALL_URL: &str = "https://raw.githubusercontent.com/Fel-2/Vidi/main/install.sh";
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Tag of the latest release when it is newer than the running binary.
/// Returns `None` on any failure: an update check must never interrupt startup.
pub async fn newer_release() -> Option<String> {
    if !check_due() {
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("vidi/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let resp = client.get(RELEASES_API).send().await.ok()?;
    record_check();
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    is_newer(tag, current_version()).then(|| tag.to_string())
}

/// Compare a release tag ("v0.2.1") with a crate version ("0.2.0") field by
/// field. Non-numeric suffixes are ignored, so "v0.3.0-rc1" reads as 0.3.0.
pub fn is_newer(tag: &str, current: &str) -> bool {
    fn fields(s: &str) -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| {
                let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse().unwrap_or(0)
            })
            .collect()
    }
    let (a, b) = (fields(tag), fields(current));
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// Shell command that reruns the published installer into the directory the
/// running binary lives in. `None` when that directory is not writable, which
/// means vidi came from a package manager and should be updated with it.
pub fn install_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let probe = dir.join(".vidi-update-probe");
    std::fs::write(&probe, b"").ok()?;
    std::fs::remove_file(&probe).ok();
    Some(format!(
        "curl -fsSL {} | PREFIX='{}' sh",
        INSTALL_URL,
        dir.display()
    ))
}

fn stamp_file() -> std::path::PathBuf {
    crate::config::youtube_cache_dir().join("update_check")
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn check_due() -> bool {
    let Ok(content) = std::fs::read_to_string(stamp_file()) else {
        return true;
    };
    let last: u64 = content.trim().parse().unwrap_or(0);
    now_unix().saturating_sub(last) >= CHECK_INTERVAL_SECS
}

fn record_check() {
    let path = stamp_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, now_unix().to_string()).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_and_minor() {
        assert!(is_newer("v0.2.1", "0.2.0"));
        assert!(is_newer("v0.3.0", "0.2.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.9", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
    }

    #[test]
    fn suffixes_and_short_tags() {
        assert!(is_newer("v0.3.0-rc1", "0.2.0"));
        assert!(!is_newer("v0.2", "0.2.0"));
        assert!(is_newer("v0.2.0.1", "0.2.0"));
    }
}
