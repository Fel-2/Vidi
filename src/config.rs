use anyhow::Result;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// YouTube config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct YoutubeConfig {
    pub player: String,
    pub video_quality: String,
    pub editor: String,
    pub enable_preview: bool,
    pub disown_streaming_process: bool,
    pub update_recent: bool,
    pub no_of_recent: usize,
    pub no_of_search_results: usize,
    pub search_history: bool,
    pub download_directory: PathBuf,
    pub pretty_print: bool,
}

impl Default for YoutubeConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            player: "mpv".into(),
            video_quality: "1080".into(),
            editor: std::env::var("EDITOR").unwrap_or_else(|_| "nano".into()),
            enable_preview: false,
            disown_streaming_process: true,
            update_recent: true,
            no_of_recent: 30,
            no_of_search_results: 30,
            search_history: true,
            download_directory: home.join("Videos").join("vidi"),
            pretty_print: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Twitch config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TwitchConfig {
    pub player: String,
    pub quality: String,
    pub editor: String,
    pub enable_preview: bool,
}

impl Default for TwitchConfig {
    fn default() -> Self {
        Self {
            player: "mpv".into(),
            quality: "best".into(),
            editor: std::env::var("EDITOR").unwrap_or_else(|_| "nano".into()),
            enable_preview: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Unified config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Config {
    pub youtube: YoutubeConfig,
    pub twitch: TwitchConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            youtube: YoutubeConfig::default(),
            twitch: TwitchConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// File paths helpers
// ---------------------------------------------------------------------------

pub fn youtube_config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
    });
    base.join("vidi")
}

pub fn youtube_config_file() -> PathBuf {
    youtube_config_dir().join("vidi.conf")
}

pub fn youtube_subs_file() -> PathBuf {
    youtube_config_dir().join("subscriptions")
}

pub fn youtube_recent_file() -> PathBuf {
    youtube_config_dir().join("recent.json")
}

pub fn youtube_saved_file() -> PathBuf {
    youtube_config_dir().join("saved_videos.json")
}

pub fn youtube_custom_playlists_file() -> PathBuf {
    youtube_config_dir().join("custom_playlists.json")
}

pub fn youtube_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".cache"))
        .join("vidi")
}

pub fn youtube_search_history_file() -> PathBuf {
    youtube_cache_dir().join("search_history.txt")
}

pub fn youtube_feed_cache_file() -> PathBuf {
    youtube_cache_dir().join("feed_cache.json")
}

pub fn youtube_preview_cache_dir() -> PathBuf {
    youtube_cache_dir().join("preview_images")
}

pub fn twitch_config_file() -> PathBuf {
    youtube_config_dir().join("twitch.conf")
}

pub fn twitch_subs_file() -> PathBuf {
    youtube_config_dir().join("twitch_subs")
}

// ---------------------------------------------------------------------------
// Loaders
// ---------------------------------------------------------------------------

fn parse_kv(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix(':') {
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

pub fn load_youtube_config() -> Result<YoutubeConfig> {
    let mut cfg = YoutubeConfig::default();
    let path = youtube_config_file();
    if !path.exists() {
        std::fs::create_dir_all(youtube_config_dir())?;
        return Ok(cfg);
    }
    let content = std::fs::read_to_string(&path)?;
    if let Some(v) = parse_kv(&content, "PLAYER") {
        cfg.player = v;
    }
    if let Some(v) = parse_kv(&content, "VIDEO_QUALITY") {
        cfg.video_quality = v;
    }
    if let Some(v) = parse_kv(&content, "EDITOR") {
        cfg.editor = v;
    }
    if let Some(v) = parse_kv(&content, "ENABLE_PREVIEW") {
        cfg.enable_preview = v.to_lowercase() == "true";
    }
    if let Some(v) = parse_kv(&content, "DISOWN_STREAMING_PROCESS") {
        cfg.disown_streaming_process = v.to_lowercase() == "true";
    }
    if let Some(v) = parse_kv(&content, "UPDATE_RECENT") {
        cfg.update_recent = v.to_lowercase() == "true";
    }
    if let Some(v) = parse_kv(&content, "NO_OF_RECENT") {
        if let Ok(n) = v.parse() {
            cfg.no_of_recent = n;
        }
    }
    if let Some(v) = parse_kv(&content, "NO_OF_SEARCH_RESULTS") {
        if let Ok(n) = v.parse() {
            cfg.no_of_search_results = n;
        }
    }
    if let Some(v) = parse_kv(&content, "SEARCH_HISTORY") {
        cfg.search_history = v.to_lowercase() == "true";
    }
    if let Some(v) = parse_kv(&content, "DOWNLOAD_DIRECTORY") {
        let expanded = shellexpand_tilde(&v);
        cfg.download_directory = PathBuf::from(expanded);
    }
    if let Some(v) = parse_kv(&content, "PRETTY_PRINT") {
        cfg.pretty_print = v.to_lowercase() == "true";
    }
    Ok(cfg)
}

pub fn load_twitch_config() -> Result<TwitchConfig> {
    let mut cfg = TwitchConfig::default();
    let path = twitch_config_file();
    if !path.exists() {
        std::fs::create_dir_all(youtube_config_dir())?;
        return Ok(cfg);
    }
    let content = std::fs::read_to_string(&path)?;

    // Twitch config supports both KEY="VALUE" (shell) and KEY: VALUE formats.
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        // Try KEY: VALUE first, then KEY=VALUE
        if let Some((key, val)) = trimmed.split_once(':').map(|(k, v)| (k.trim(), v.trim()))
            .or_else(|| trimmed.split_once('=').map(|(k, v)| (k.trim(), v.trim().trim_matches('"'))))
        {
            match key {
                "PLAYER" => cfg.player = val.to_string(),
                "QUALITY" => cfg.quality = val.to_string(),
                "PREFERRED_EDITOR" | "EDITOR" => cfg.editor = val.to_string(),
                "ENABLE_PREVIEW" => cfg.enable_preview = val.to_lowercase() == "true",
                _ => {}
            }
        }
    }
    Ok(cfg)
}

pub fn load_config() -> Result<Config> {
    Ok(Config {
        youtube: load_youtube_config()?,
        twitch: load_twitch_config()?,
    })
}

fn shellexpand_tilde(s: &str) -> String {
    if s.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().to_string() + &s[1..];
        }
    }
    s.to_string()
}

pub fn write_default_youtube_config() -> Result<()> {
    let path = youtube_config_file();
    std::fs::create_dir_all(youtube_config_dir())?;
    if path.exists() {
        return Ok(());
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let download_dir = home.join("Videos").join("vidi");
    let content = format!(
        "# vidi configuration\n\
         PLAYER: mpv\n\
         VIDEO_QUALITY: 1080\n\
         EDITOR: {}\n\
         ENABLE_PREVIEW: false\n\
         DISOWN_STREAMING_PROCESS: true\n\
         UPDATE_RECENT: true\n\
         NO_OF_RECENT: 30\n\
         NO_OF_SEARCH_RESULTS: 30\n\
         SEARCH_HISTORY: true\n\
         DOWNLOAD_DIRECTORY: {}\n\
         PRETTY_PRINT: true\n",
        std::env::var("EDITOR").unwrap_or_else(|_| "nano".into()),
        download_dir.display()
    );
    std::fs::write(path, content)?;
    Ok(())
}

pub fn write_default_twitch_config() -> Result<()> {
    let path = twitch_config_file();
    std::fs::create_dir_all(youtube_config_dir())?;
    if path.exists() {
        return Ok(());
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
    let content = format!(
        "# Twitch TUI Configuration\n\
         PREFERRED_EDITOR=\"{}\"\n\
         PLAYER=\"mpv\"\n\
         QUALITY=\"best\"\n\
         ENABLE_PREVIEW=\"false\"\n",
        editor
    );
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_kv ────────────────────────────────────────────────────────

    #[test]
    fn parse_kv_basic() {
        assert_eq!(parse_kv("PLAYER: mpv", "PLAYER"), Some("mpv".to_string()));
    }

    #[test]
    fn parse_kv_with_spaces() {
        assert_eq!(
            parse_kv("PLAYER :   mpv  ", "PLAYER"),
            Some("mpv".to_string())
        );
    }

    #[test]
    fn parse_kv_ignores_comments() {
        let content = "# PLAYER: vlc\nPLAYER: mpv";
        assert_eq!(parse_kv(content, "PLAYER"), Some("mpv".to_string()));
    }

    #[test]
    fn parse_kv_missing_key() {
        assert_eq!(parse_kv("QUALITY: 1080", "PLAYER"), None);
    }

    #[test]
    fn parse_kv_no_colon_separator() {
        // parse_kv requires colon, not equals
        assert_eq!(parse_kv("PLAYER=mpv", "PLAYER"), None);
    }

    #[test]
    fn parse_kv_multiline() {
        let content = "PLAYER: mpv\nVIDEO_QUALITY: 720\nEDITOR: vim";
        assert_eq!(parse_kv(content, "VIDEO_QUALITY"), Some("720".to_string()));
        assert_eq!(parse_kv(content, "EDITOR"), Some("vim".to_string()));
    }

    // ── shellexpand_tilde ───────────────────────────────────────────────

    #[test]
    fn shellexpand_no_tilde() {
        assert_eq!(shellexpand_tilde("/usr/bin"), "/usr/bin");
    }

    #[test]
    fn shellexpand_tilde_expands() {
        let result = shellexpand_tilde("~/Downloads");
        assert!(!result.starts_with('~'));
        assert!(result.ends_with("/Downloads"));
    }

    // ── Config defaults ─────────────────────────────────────────────────

    #[test]
    fn youtube_config_defaults() {
        let cfg = YoutubeConfig::default();
        assert_eq!(cfg.player, "mpv");
        assert_eq!(cfg.video_quality, "1080");
        assert!(cfg.update_recent);
        assert_eq!(cfg.no_of_recent, 30);
    }

    #[test]
    fn twitch_config_defaults() {
        let cfg = TwitchConfig::default();
        assert_eq!(cfg.player, "mpv");
        assert_eq!(cfg.quality, "best");
        assert!(!cfg.enable_preview);
    }
}
