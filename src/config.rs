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
    /// Track playback position via mpv IPC and resume on next watch.
    pub watch_progress: bool,
    /// SponsorBlock categories for mpv/yt-dlp ("" disables; e.g. "sponsor,selfpromo").
    pub sponsorblock: String,
    /// Show YouTube Shorts in lists and channel tabs (hidden by default).
    pub show_shorts: bool,
    /// Check for a newer release on launch and offer to install it.
    pub check_updates: bool,
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
            watch_progress: true,
            sponsorblock: String::new(),
            show_shorts: false,
            check_updates: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Twitch config
// ---------------------------------------------------------------------------

/// Default Twitch GQL client-id (the public Twitch web client-id). The old
/// Android app client-id is no longer accepted by gql.twitch.tv.
pub const DEFAULT_TWITCH_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";

#[derive(Debug, Clone)]
pub struct TwitchConfig {
    pub player: String,
    pub quality: String,
    pub editor: String,
    pub enable_preview: bool,
    pub client_id: String,
}

impl Default for TwitchConfig {
    fn default() -> Self {
        Self {
            player: "mpv".into(),
            quality: "best".into(),
            editor: std::env::var("EDITOR").unwrap_or_else(|_| "nano".into()),
            enable_preview: false,
            client_id: DEFAULT_TWITCH_CLIENT_ID.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Kick config
// ---------------------------------------------------------------------------

/// Browser User-Agent; Kick rejects requests without one.
const DEFAULT_KICK_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36";

#[derive(Debug, Clone)]
pub struct KickConfig {
    pub player: String,
    pub quality: String,
    pub editor: String,
    pub enable_preview: bool,
    pub user_agent: String,
}

impl Default for KickConfig {
    fn default() -> Self {
        Self {
            player: "mpv".into(),
            quality: "best".into(),
            editor: std::env::var("EDITOR").unwrap_or_else(|_| "nano".into()),
            enable_preview: false,
            user_agent: DEFAULT_KICK_UA.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// PeerTube config
// ---------------------------------------------------------------------------

pub const DEFAULT_PEERTUBE_INSTANCE: &str = "https://makertube.net";
pub const SEPIASEARCH_URL: &str = "https://sepiasearch.org";

#[derive(Debug, Clone)]
pub struct PeertubeConfig {
    pub instance: String,
    pub search_instance: String,
}

impl Default for PeertubeConfig {
    fn default() -> Self {
        Self {
            instance: String::new(),
            search_instance: SEPIASEARCH_URL.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Unified config
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Keybindings
// ---------------------------------------------------------------------------

/// Optional single-character key overrides. When a field is `Some(c)`, pressing
/// that character is treated as the corresponding navigation action. Defaults
/// (arrow keys, vim h/j/k/l, Enter, Esc, q) always remain active.
#[derive(Debug, Clone, Default)]
pub struct Keybindings {
    pub up: Option<char>,
    pub down: Option<char>,
    pub select: Option<char>,
    pub back: Option<char>,
    pub quit: Option<char>,
    pub page_up: Option<char>,
    pub page_down: Option<char>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub youtube: YoutubeConfig,
    pub twitch: TwitchConfig,
    pub kick: KickConfig,
    pub peertube: PeertubeConfig,
    pub keys: Keybindings,
    /// Platforms shown on the start screen, in menu order.
    pub platforms: Vec<MenuPlatform>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            youtube: YoutubeConfig::default(),
            twitch: TwitchConfig::default(),
            kick: KickConfig::default(),
            peertube: PeertubeConfig::default(),
            keys: Keybindings::default(),
            platforms: MenuPlatform::ALL.to_vec(),
        }
    }
}

// ---------------------------------------------------------------------------
// Platform selection
// ---------------------------------------------------------------------------

/// A platform that can be shown or hidden on the start screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPlatform {
    Youtube,
    Twitch,
    Kick,
    Peertube,
}

impl MenuPlatform {
    /// Every platform, in menu order. This is the default set.
    pub const ALL: [MenuPlatform; 4] = [
        MenuPlatform::Youtube,
        MenuPlatform::Twitch,
        MenuPlatform::Kick,
        MenuPlatform::Peertube,
    ];

    pub fn key(self) -> &'static str {
        match self {
            MenuPlatform::Youtube => "youtube",
            MenuPlatform::Twitch => "twitch",
            MenuPlatform::Kick => "kick",
            MenuPlatform::Peertube => "peertube",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MenuPlatform::Youtube => "📺  YouTube",
            MenuPlatform::Twitch => "🟣  Twitch",
            MenuPlatform::Kick => "🟢  Kick",
            MenuPlatform::Peertube => "🐙  PeerTube",
        }
    }

    /// Comma-separated list of every valid `PLATFORMS` value, for config docs.
    pub fn all_keys() -> String {
        MenuPlatform::ALL
            .iter()
            .map(|p| p.key())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn from_key(s: &str) -> Option<MenuPlatform> {
        match s.trim().to_lowercase().as_str() {
            "youtube" | "yt" => Some(MenuPlatform::Youtube),
            "twitch" => Some(MenuPlatform::Twitch),
            "kick" => Some(MenuPlatform::Kick),
            "peertube" | "pt" => Some(MenuPlatform::Peertube),
            _ => None,
        }
    }
}

/// Parse a `PLATFORMS` value into the enabled set, preserving menu order.
///
/// An empty or entirely unrecognised value means "all platforms", so a typo
/// never leaves the user with an unreachable app.
pub fn parse_platforms(value: &str) -> Vec<MenuPlatform> {
    let wanted: Vec<MenuPlatform> = value
        .split(',')
        .filter_map(MenuPlatform::from_key)
        .collect();
    if wanted.is_empty() {
        return MenuPlatform::ALL.to_vec();
    }
    MenuPlatform::ALL
        .into_iter()
        .filter(|p| wanted.contains(p))
        .collect()
}

impl Config {
    pub fn platform_enabled(&self, p: MenuPlatform) -> bool {
        self.platforms.contains(&p)
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
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache")
        })
        .join("vidi")
}

pub fn youtube_search_history_file() -> PathBuf {
    youtube_cache_dir().join("search_history.txt")
}

pub fn youtube_feed_cache_file() -> PathBuf {
    youtube_cache_dir().join("feed_cache.json")
}

pub fn youtube_channel_names_file() -> PathBuf {
    youtube_cache_dir().join("channel_names.json")
}

pub fn youtube_channel_avatars_file() -> PathBuf {
    youtube_cache_dir().join("channel_avatars.json")
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

pub fn kick_config_file() -> PathBuf {
    youtube_config_dir().join("kick.conf")
}

pub fn kick_subs_file() -> PathBuf {
    youtube_config_dir().join("kick_subs")
}

pub fn peertube_config_file() -> PathBuf {
    youtube_config_dir().join("peertube.conf")
}

pub fn peertube_subs_file() -> PathBuf {
    youtube_config_dir().join("peertube_subs")
}

pub fn peertube_feed_cache_file() -> PathBuf {
    youtube_cache_dir().join("peertube_feed_cache.json")
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
    if let Some(v) = parse_kv(&content, "WATCH_PROGRESS") {
        cfg.watch_progress = v.to_lowercase() == "true";
    }
    if let Some(v) = parse_kv(&content, "SPONSORBLOCK") {
        cfg.sponsorblock = if v.to_lowercase() == "false" {
            String::new()
        } else {
            v
        };
    }
    if let Some(v) = parse_kv(&content, "SHOW_SHORTS") {
        cfg.show_shorts = v.to_lowercase() == "true";
    }
    if let Some(v) = parse_kv(&content, "CHECK_UPDATES") {
        cfg.check_updates = v.to_lowercase() == "true";
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
        if let Some((key, val)) = trimmed
            .split_once(':')
            .map(|(k, v)| (k.trim(), v.trim()))
            .or_else(|| {
                trimmed
                    .split_once('=')
                    .map(|(k, v)| (k.trim(), v.trim().trim_matches('"')))
            })
        {
            match key {
                "PLAYER" => cfg.player = val.to_string(),
                "QUALITY" => cfg.quality = val.to_string(),
                "PREFERRED_EDITOR" | "EDITOR" => cfg.editor = val.to_string(),
                "ENABLE_PREVIEW" => cfg.enable_preview = val.to_lowercase() == "true",
                "CLIENT_ID" if !val.is_empty() => cfg.client_id = val.to_string(),
                _ => {}
            }
        }
    }
    Ok(cfg)
}

pub fn load_kick_config() -> Result<KickConfig> {
    let mut cfg = KickConfig::default();
    let path = kick_config_file();
    if !path.exists() {
        std::fs::create_dir_all(youtube_config_dir())?;
        return Ok(cfg);
    }
    let content = std::fs::read_to_string(&path)?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some((key, val)) = trimmed
            .split_once(':')
            .map(|(k, v)| (k.trim(), v.trim()))
            .or_else(|| {
                trimmed
                    .split_once('=')
                    .map(|(k, v)| (k.trim(), v.trim().trim_matches('"')))
            })
        {
            match key {
                "PLAYER" => cfg.player = val.to_string(),
                "QUALITY" => cfg.quality = val.to_string(),
                "PREFERRED_EDITOR" | "EDITOR" => cfg.editor = val.to_string(),
                "ENABLE_PREVIEW" => cfg.enable_preview = val.to_lowercase() == "true",
                "USER_AGENT" if !val.is_empty() => cfg.user_agent = val.to_string(),
                _ => {}
            }
        }
    }
    Ok(cfg)
}

pub fn load_peertube_config() -> Result<PeertubeConfig> {
    let mut cfg = PeertubeConfig::default();
    let path = peertube_config_file();
    if !path.exists() {
        return Ok(cfg);
    }
    let content = std::fs::read_to_string(&path)?;
    if let Some(v) = parse_kv(&content, "INSTANCE") {
        cfg.instance = v;
    }
    if let Some(v) = parse_kv(&content, "SEARCH_INSTANCE") {
        if !v.is_empty() {
            cfg.search_instance = v;
        }
    }
    Ok(cfg)
}

pub fn save_peertube_instance(instance: &str) -> Result<()> {
    let path = peertube_config_file();
    std::fs::create_dir_all(youtube_config_dir())?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let line = format!("INSTANCE: {}", instance);

    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    for l in existing.lines() {
        if parse_kv(l, "INSTANCE").is_some() {
            out.push(line.clone());
            replaced = true;
        } else {
            out.push(l.to_string());
        }
    }
    if !replaced {
        if out.is_empty() {
            out.push("# vidi PeerTube configuration".to_string());
        }
        out.push(line);
    }
    let mut content = out.join("\n");
    content.push('\n');
    std::fs::write(path, content)?;
    Ok(())
}

/// Read a single-char keybinding override from vidi.conf content.
fn parse_key(content: &str, key: &str) -> Option<char> {
    parse_kv(content, key).and_then(|v| v.trim().chars().next())
}

pub fn load_keybindings() -> Keybindings {
    let path = youtube_config_file();
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Keybindings::default();
    };
    Keybindings {
        up: parse_key(&content, "KEY_UP"),
        down: parse_key(&content, "KEY_DOWN"),
        select: parse_key(&content, "KEY_SELECT"),
        back: parse_key(&content, "KEY_BACK"),
        quit: parse_key(&content, "KEY_QUIT"),
        page_up: parse_key(&content, "KEY_PAGE_UP"),
        page_down: parse_key(&content, "KEY_PAGE_DOWN"),
    }
}

pub fn load_config() -> Result<Config> {
    Ok(Config {
        youtube: load_youtube_config()?,
        twitch: load_twitch_config()?,
        kick: load_kick_config()?,
        peertube: load_peertube_config().unwrap_or_default(),
        keys: load_keybindings(),
        platforms: load_platforms(),
    })
}

/// Read `PLATFORMS` from vidi.conf. Absent or unparseable → all platforms.
fn load_platforms() -> Vec<MenuPlatform> {
    let Ok(content) = std::fs::read_to_string(youtube_config_file()) else {
        return MenuPlatform::ALL.to_vec();
    };
    match parse_kv(&content, "PLATFORMS") {
        Some(v) => parse_platforms(&v),
        None => MenuPlatform::ALL.to_vec(),
    }
}

fn shellexpand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().to_string() + rest;
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
         PRETTY_PRINT: true\n\
         # Track playback position via mpv IPC and resume on next watch:\n\
         WATCH_PROGRESS: true\n\
         # SponsorBlock categories to skip (mpv) / remove (downloads).\n\
         # Comma-separated, e.g. sponsor,selfpromo,interaction — empty disables:\n\
         # SPONSORBLOCK: sponsor\n\
         # Show YouTube Shorts in lists and channel tabs (hidden by default):\n\
         SHOW_SHORTS: false\n\
         # Check for a newer release on launch and offer to install it:\n\
         CHECK_UPDATES: true\n\
         # Platforms shown on the start screen (comma-separated).\n\
         # Valid: {}. Omit this line to show all:\n\
         PLATFORMS: {}\n\
         # Optional single-key overrides (arrows + vim keys always work):\n\
         # KEY_UP: k\n\
         # KEY_DOWN: j\n\
         # KEY_SELECT: l\n\
         # KEY_BACK: h\n\
         # KEY_QUIT: q\n\
         # KEY_PAGE_UP: u\n\
         # KEY_PAGE_DOWN: d\n",
        std::env::var("EDITOR").unwrap_or_else(|_| "nano".into()),
        download_dir.display(),
        MenuPlatform::all_keys(),
        MenuPlatform::all_keys(),
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
         ENABLE_PREVIEW=\"false\"\n\
         # Override the Twitch GQL client-id used for search (optional).\n\
         # CLIENT_ID=\"{}\"\n",
        editor, DEFAULT_TWITCH_CLIENT_ID
    );
    std::fs::write(path, content)?;
    Ok(())
}

pub fn write_default_kick_config() -> Result<()> {
    let path = kick_config_file();
    std::fs::create_dir_all(youtube_config_dir())?;
    if path.exists() {
        return Ok(());
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
    let content = format!(
        "# Kick TUI Configuration\n\
         PREFERRED_EDITOR=\"{}\"\n\
         PLAYER=\"mpv\"\n\
         QUALITY=\"best\"\n\
         ENABLE_PREVIEW=\"false\"\n\
         # Kick has no public API and rejects requests without a browser UA (optional).\n\
         # USER_AGENT=\"{}\"\n",
        editor, DEFAULT_KICK_UA
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

    #[test]
    fn kick_config_defaults() {
        let cfg = KickConfig::default();
        assert_eq!(cfg.player, "mpv");
        assert_eq!(cfg.quality, "best");
        assert!(!cfg.enable_preview);
        assert!(cfg.user_agent.contains("Mozilla"));
    }

    // ── Platform selection ──────────────────────────────────────────────

    #[test]
    fn platforms_default_to_all_four() {
        assert_eq!(parse_platforms(""), MenuPlatform::ALL.to_vec());
        assert_eq!(Config::default().platforms, MenuPlatform::ALL.to_vec());
    }

    #[test]
    fn platforms_parse_subset_in_menu_order() {
        // Requested out of order; result must follow menu order.
        assert_eq!(
            parse_platforms("kick,youtube"),
            vec![MenuPlatform::Youtube, MenuPlatform::Kick]
        );
    }

    #[test]
    fn platforms_accept_aliases_and_whitespace_and_case() {
        assert_eq!(
            parse_platforms(" YT , PT "),
            vec![MenuPlatform::Youtube, MenuPlatform::Peertube]
        );
    }

    #[test]
    fn platforms_unknown_value_falls_back_to_all() {
        // A typo must never leave the user with an unreachable app.
        assert_eq!(parse_platforms("nonsense"), MenuPlatform::ALL.to_vec());
        assert_eq!(
            parse_platforms("youtube,nonsense"),
            vec![MenuPlatform::Youtube]
        );
    }

    #[test]
    fn platforms_dedupe_repeats() {
        assert_eq!(
            parse_platforms("twitch,twitch,twitch"),
            vec![MenuPlatform::Twitch]
        );
    }

    #[test]
    fn platform_enabled_reflects_parsed_set() {
        let cfg = Config {
            platforms: parse_platforms("twitch,kick"),
            ..Config::default()
        };
        assert!(cfg.platform_enabled(MenuPlatform::Twitch));
        assert!(cfg.platform_enabled(MenuPlatform::Kick));
        assert!(!cfg.platform_enabled(MenuPlatform::Youtube));
        assert!(!cfg.platform_enabled(MenuPlatform::Peertube));
    }

    #[test]
    fn platforms_keys_are_unique() {
        let mut keys: Vec<&str> = MenuPlatform::ALL.iter().map(|p| p.key()).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before);
    }
}
