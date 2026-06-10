use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::{
    youtube_channel_avatars_file, youtube_channel_names_file, youtube_custom_playlists_file,
    youtube_feed_cache_file, youtube_recent_file, youtube_saved_file, youtube_search_history_file,
};
use crate::models::{Channel, CustomPlaylist, RecentVideos, SavedVideos, Video};

// ---------------------------------------------------------------------------
// yt-dlp helpers
// ---------------------------------------------------------------------------

pub async fn run_yt_dlp(url: &str, playlist_end: u32) -> Result<Value> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("yt-dlp")
            .args([
                url,
                "-J",
                "--flat-playlist",
                "--extractor-args",
                "youtubetab:approximate_date",
                "--playlist-start",
                "1",
                "--playlist-end",
                &playlist_end.to_string(),
                "--socket-timeout",
                "15",
                "--retries",
                "1",
            ])
            .output(),
    )
    .await
    .context("yt-dlp timed out")?
    .context("Failed to run yt-dlp")?;

    if !output.status.success() || output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.lines().last().unwrap_or("unknown error").trim();
        anyhow::bail!(
            "yt-dlp failed: {}",
            if msg.is_empty() { "no output" } else { msg }
        );
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse yt-dlp JSON output")
}

/// Parse a playlist JSON (--flat-playlist -J) into a Vec<Video>.
pub fn parse_playlist_json(json: &Value) -> Vec<Video> {
    // Extract channel info from the root playlist object as fallback for entries
    // (flat-playlist entries often have channel: null).
    let root_channel = json
        .get("channel")
        .and_then(|v| v.as_str())
        .or_else(|| json.get("uploader").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let root_channel_url = json
        .get("channel_url")
        .and_then(|v| v.as_str())
        .or_else(|| json.get("uploader_url").and_then(|v| v.as_str()))
        .or_else(|| json.get("webpage_url").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();

    let entries = match json.get("entries") {
        Some(Value::Array(arr)) => arr,
        _ => {
            // Single video
            if json.get("id").and_then(|v| v.as_str()).is_some() {
                let v = json_to_video(json, &root_channel, &root_channel_url);
                return if v.id.is_empty() { vec![] } else { vec![v] };
            }
            return vec![];
        }
    };

    entries
        .iter()
        .map(|e| json_to_video(e, &root_channel, &root_channel_url))
        .collect()
}

fn json_to_video(j: &Value, fallback_channel: &str, fallback_channel_url: &str) -> Video {
    let id = j
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let title = j
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(no title)")
        .to_string();
    let url = j
        .get("url")
        .and_then(|v| v.as_str())
        .or_else(|| j.get("webpage_url").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if id.is_empty() {
                String::new()
            } else {
                format!("https://www.youtube.com/watch?v={}", id)
            }
        });
    // Entry-level channel is often null in flat-playlist; fall back to root playlist info.
    let channel = j
        .get("channel")
        .and_then(|v| v.as_str())
        .or_else(|| j.get("uploader").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_channel)
        .to_string();
    let channel_url = j
        .get("channel_url")
        .and_then(|v| v.as_str())
        .or_else(|| j.get("uploader_url").and_then(|v| v.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_channel_url)
        .to_string();
    // yt-dlp flat-playlist with approximate_date may omit upload_date entirely.
    // Derive it from timestamp (Unix epoch) when present.
    let upload_date = j
        .get("upload_date")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            j.get("timestamp")
                .and_then(|v| v.as_i64())
                .or_else(|| j.get("release_timestamp").and_then(|v| v.as_i64()))
                .or_else(|| j.get("modified_timestamp").and_then(|v| v.as_i64()))
                .or_else(|| j.get("epoch").and_then(|v| v.as_i64()))
                .map(timestamp_to_yyyymmdd)
                .unwrap_or_default()
        });
    let duration_string = j
        .get("duration_string")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let view_count = j.get("view_count").and_then(|v| v.as_u64());
    // yt-dlp flat-playlist returns a `thumbnails` array, not a `thumbnail` string.
    // Pick the last (highest-res) entry, or fall back to a known-good URL.
    let thumbnail = j
        .get("thumbnail")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            j.get("thumbnails")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.last())
                .and_then(|last| last.get("url"))
                .and_then(|u| u.as_str())
                // Strip query-string noise from cached thumbnail URLs
                .map(|u| u.split('?').next().unwrap_or(u).to_string())
        })
        .unwrap_or_else(|| {
            if id.is_empty() {
                String::new()
            } else {
                format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", id)
            }
        });
    let playlist_url = j
        .get("playlist_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let playlist_title = j
        .get("playlist_title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let description = j
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let timestamp = j
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .or_else(|| j.get("release_timestamp").and_then(|v| v.as_i64()))
        .or_else(|| j.get("modified_timestamp").and_then(|v| v.as_i64()))
        .or_else(|| j.get("epoch").and_then(|v| v.as_i64()));

    let is_short = url.contains("/shorts/");

    Video {
        id,
        title,
        url,
        channel,
        channel_url,
        upload_date,
        duration_string,
        view_count,
        thumbnail,
        playlist_url,
        playlist_title,
        description,
        timestamp,
        is_short,
    }
}

/// Drop Shorts from a video list unless the user enabled SHOW_SHORTS.
pub fn apply_shorts_filter(videos: Vec<Video>, show_shorts: bool) -> Vec<Video> {
    if show_shorts {
        videos
    } else {
        videos.into_iter().filter(|v| !v.is_short).collect()
    }
}

/// Convert a Unix timestamp (seconds) to `YYYYMMDD` string (UTC).
pub fn timestamp_to_yyyymmdd(secs: i64) -> String {
    let days = secs / 86400;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}{:02}{:02}", y, m, d)
}

// ---------------------------------------------------------------------------
// Specific fetch operations
// ---------------------------------------------------------------------------

pub async fn fetch_trending(limit: u32) -> Result<Vec<Video>> {
    if let Ok(videos) = crate::innertube::trending(limit as usize).await {
        return Ok(videos);
    }
    let json = run_yt_dlp("https://www.youtube.com/gaming", limit).await?;
    Ok(parse_playlist_json(&json))
}

pub async fn fetch_search(query: &str, sp: &str, limit: u32) -> Result<Vec<Video>> {
    if let Ok(videos) = crate::innertube::search_videos(query, sp, limit as usize).await {
        return Ok(videos);
    }
    let url = if sp.is_empty() {
        format!("ytsearch{}:{}", limit, query)
    } else {
        format!(
            "https://www.youtube.com/results?search_query={}&sp={}",
            urlencoding_simple(query),
            sp
        )
    };
    let json = run_yt_dlp(&url, limit).await?;
    Ok(parse_playlist_json(&json))
}

pub fn urlencoding_simple(s: &str) -> String {
    let mut out = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    out
}

pub async fn fetch_playlist(playlist_url: &str, limit: u32) -> Result<Vec<Video>> {
    // Fast path: serve channel tabs, playlists and search-result URLs straight
    // from Innertube. Anything unrecognised (channel /playlists tab, channel
    // /search) still goes through yt-dlp below.
    use crate::innertube::{self, UrlKind};
    let fast = match innertube::classify_url(playlist_url) {
        UrlKind::ChannelTab(base, tab) => innertube::channel_tab_videos(&base, tab, limit as usize)
            .await
            .ok(),
        UrlKind::Playlist(id) => innertube::playlist_videos(&id, limit as usize).await.ok(),
        UrlKind::SearchResults { query, sp } => {
            innertube::search_videos(&query, &sp, limit as usize)
                .await
                .ok()
        }
        UrlKind::Unsupported => None,
    };
    if let Some(videos) = fast {
        return Ok(videos);
    }
    let json = run_yt_dlp(playlist_url, limit).await?;
    Ok(parse_playlist_json(&json))
}

/// Search YouTube for channels matching `query`.
/// Uses the YouTube search channel-type filter (sp=EgIQAg==).
pub async fn search_channels(query: &str, limit: u32) -> Result<Vec<crate::models::Channel>> {
    if let Ok(channels) = crate::innertube::search_channels(query, limit as usize).await {
        return Ok(channels);
    }
    let encoded = urlencoding_simple(query);
    // sp=EgIQAg%3D%3D is the URL-encoded form of the protobuf filter "Type: Channel"
    let url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgIQAg%3D%3D",
        encoded
    );
    let json = run_yt_dlp(&url, limit).await?;

    let entries = match json.get("entries").and_then(|e| e.as_array()) {
        Some(arr) => arr,
        None => return Ok(vec![]),
    };

    let channels = entries
        .iter()
        .filter_map(|e| {
            let url = e
                .get("url")
                .and_then(|v| v.as_str())
                .or_else(|| e.get("webpage_url").and_then(|v| v.as_str()))
                .map(|s| s.to_string())?;
            let name = e
                .get("title")
                .and_then(|v| v.as_str())
                .or_else(|| e.get("channel").and_then(|v| v.as_str()))
                .or_else(|| e.get("uploader").and_then(|v| v.as_str()))
                .map(|s| s.to_string())?;
            if url.is_empty() || name.is_empty() {
                return None;
            }
            Some(crate::models::Channel { name, url })
        })
        .collect();

    Ok(channels)
}

/// Fetch latest videos from all subscribed channels in parallel.
/// `cutoff_date` – if `Some("YYYYMMDD")`, only include videos on/after that date.
/// `playlist_end` – how many recent videos to pull per channel.
pub async fn fetch_subscription_feed(
    subs: Vec<String>,
    playlist_end: u32,
    max_concurrent: usize,
    cutoff_date: Option<String>,
) -> Result<Vec<Video>> {
    use tokio::task::JoinSet;

    let mut set: JoinSet<Vec<Video>> = JoinSet::new();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrent));

    // Channel names for filling in Innertube tab results, which omit the
    // byline: inline names from the subscriptions file + the resolved cache.
    let mut names: std::collections::HashMap<String, String> = load_channel_name_cache();
    for (url, name) in load_subscriptions_with_names() {
        if let Some(name) = name {
            names.insert(url.trim_end_matches('/').to_string(), name);
        }
    }
    let names = std::sync::Arc::new(names);

    for url in subs {
        let sem = sem.clone();
        let cutoff = cutoff_date.clone();
        let names = names.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let tab_url = format!("{}/videos", url.trim_end_matches('/'));
            match fetch_playlist(&tab_url, playlist_end).await {
                Ok(mut videos) => {
                    if let Some(name) = names.get(url.trim_end_matches('/')) {
                        for v in &mut videos {
                            if v.channel.is_empty() {
                                v.channel = name.clone();
                            }
                        }
                    }
                    match cutoff {
                        Some(ref date) => videos
                            .into_iter()
                            .filter(|v| !v.upload_date.is_empty() && &v.upload_date >= date)
                            .collect(),
                        None => videos,
                    }
                }
                Err(_) => vec![],
            }
        });
    }

    let mut all = Vec::new();
    while let Some(result) = set.join_next().await {
        if let Ok(videos) = result {
            all.extend(videos);
        }
    }

    sort_videos_newest_first(&mut all);
    Ok(all)
}

pub fn sort_videos_newest_first(videos: &mut [Video]) {
    videos.sort_by(|a, b| match (b.timestamp, a.timestamp) {
        (Some(bt), Some(at)) => bt.cmp(&at),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => b.upload_date.cmp(&a.upload_date),
    });
}

// ---------------------------------------------------------------------------
// Feed cache
// ---------------------------------------------------------------------------

/// How long the feed cache is considered fresh (in seconds).
const FEED_CACHE_MAX_AGE_SECS: u64 = 15 * 60; // 15 minutes

#[derive(serde::Serialize, serde::Deserialize)]
struct FeedCache {
    /// Unix timestamp when the cache was written.
    cached_at: u64,
    videos: Vec<Video>,
}

/// Return the cached feed regardless of age, plus whether it is still fresh.
/// Stale caches are shown instantly while a background refresh runs.
pub fn load_feed_cache_with_age() -> Option<(Vec<Video>, bool)> {
    let path = youtube_feed_cache_file();
    let content = std::fs::read_to_string(&path).ok()?;
    let cache: FeedCache = serde_json::from_str(&content).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let fresh = now.saturating_sub(cache.cached_at) < FEED_CACHE_MAX_AGE_SECS;
    Some((cache.videos, fresh))
}

/// Write feed results to cache.
pub fn save_feed_cache(videos: &[Video]) {
    let path = youtube_feed_cache_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cache = FeedCache {
        cached_at: now,
        videos: videos.to_vec(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        std::fs::write(path, json).ok();
    }
}

// ---------------------------------------------------------------------------
// Search filter parsing
// ---------------------------------------------------------------------------

pub fn parse_search_filter(input: &str) -> (String, String) {
    let sp_map: &[(&str, &str)] = &[
        (":hour", "EgIIAQ%253D%253D"),
        (":today", "EgIIAg%253D%253D"),
        (":week", "EgIIAw%253D%253D"),
        (":month", "EgIIBA%253D%253D"),
        (":year", "EgIIBQ%253D%253D"),
        (":video", "EgIQAQ%253D%253D"),
        (":movie", "EgIQAg%253D%253D"),
        (":live", "EgJAAQ%253D%253D"),
        (":short", "EgIYAQ%253D%253D"),
        (":long", "EgIgAQ%253D%253D"),
        (":4k", "EgJwAQ%253D%253D"),
        (":hd", "EgIgAQ%253D%253D"),
        (":subtitles", "EgIoAQ%253D%253D"),
        (":360", "EgJ4AQ%253D%253D"),
        (":vr", "EgJ4Ag%253D%253D"),
        (":3d", "EgI4AQ%253D%253D"),
        (":hdr", "EgJ4BA%253D%253D"),
        (":newest", "CAI%253D"),
        (":views", "CAM%253D"),
        (":rating", "CAE%253D"),
    ];

    let mut sp_param = String::new();
    let mut query = input.to_string();

    for (prefix, sp) in sp_map {
        if let Some(rest) = query.strip_prefix(prefix) {
            sp_param = sp.to_string();
            query = rest.trim().to_string();
            break;
        }
    }

    (sp_param, query)
}

// ---------------------------------------------------------------------------
// Recent videos
// ---------------------------------------------------------------------------

pub fn load_recent() -> RecentVideos {
    let path = youtube_recent_file();
    if !path.exists() {
        return RecentVideos::default();
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_recent(recent: &RecentVideos, max: usize) -> anyhow::Result<()> {
    let path = youtube_recent_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Keep last `max` unique by id
    let mut seen = std::collections::HashSet::new();
    let mut entries: Vec<Video> = recent
        .entries
        .iter()
        .rev()
        .filter(|v| seen.insert(v.id.clone()))
        .cloned()
        .collect();
    entries.reverse();
    if entries.len() > max {
        entries = entries[entries.len() - max..].to_vec();
    }
    let out = RecentVideos { entries };
    let json = serde_json::to_string_pretty(&out)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn add_to_recent(video: &Video, max: usize) -> anyhow::Result<()> {
    let mut recent = load_recent();
    recent.entries.retain(|v| v.id != video.id);
    recent.entries.push(video.clone());
    save_recent(&recent, max)
}

// ---------------------------------------------------------------------------
// Saved videos
// ---------------------------------------------------------------------------

pub fn load_saved() -> SavedVideos {
    let path = youtube_saved_file();
    if !path.exists() {
        return SavedVideos::default();
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_saved(saved: &SavedVideos) -> anyhow::Result<()> {
    let path = youtube_saved_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(saved)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn save_video(video: &Video) -> anyhow::Result<()> {
    let mut saved = load_saved();
    if !saved.entries.iter().any(|v| v.id == video.id) {
        saved.entries.push(video.clone());
        save_saved(&saved)?;
    }
    Ok(())
}

pub fn unsave_video(video_id: &str) -> anyhow::Result<()> {
    let mut saved = load_saved();
    saved.entries.retain(|v| v.id != video_id);
    save_saved(&saved)
}

// ---------------------------------------------------------------------------
// Custom playlists
// ---------------------------------------------------------------------------

pub fn load_custom_playlists() -> Vec<CustomPlaylist> {
    let path = youtube_custom_playlists_file();
    if !path.exists() {
        return vec![];
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn save_custom_playlists(playlists: &[CustomPlaylist]) -> anyhow::Result<()> {
    let path = youtube_custom_playlists_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(playlists)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn save_playlist_as_custom(video: &Video) -> anyhow::Result<()> {
    let mut playlists = load_custom_playlists();
    let name = video
        .playlist_title
        .clone()
        .unwrap_or_else(|| video.title.clone());
    let playlist_url = video
        .playlist_url
        .clone()
        .unwrap_or_else(|| video.url.clone());
    let playlist_watch_url = format!(
        "https://www.youtube.com/watch?v={}&list={}",
        video.id,
        video
            .playlist_url
            .as_deref()
            .unwrap_or("")
            .split("list=")
            .nth(1)
            .unwrap_or("")
    );
    playlists.push(CustomPlaylist {
        name,
        playlist_url,
        playlist_watch_url,
    });
    save_custom_playlists(&playlists)
}

// ---------------------------------------------------------------------------
// Search history
// ---------------------------------------------------------------------------

pub fn load_search_history() -> Vec<String> {
    let path = youtube_search_history_file();
    if !path.exists() {
        return vec![];
    }
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

pub fn append_search_history(query: &str) -> anyhow::Result<()> {
    let path = youtube_search_history_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", query)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

pub fn subscribe_channel(url: &str) -> anyhow::Result<()> {
    let trimmed = url.trim();
    let existing = load_subscriptions();
    if existing
        .iter()
        .any(|s| s.trim_end_matches('/') == trimmed.trim_end_matches('/'))
    {
        anyhow::bail!("Already subscribed");
    }
    let path = crate::config::youtube_subs_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    use std::io::Write;
    writeln!(file, "{}", trimmed)?;
    Ok(())
}

/// Parse one subscriptions-file line into (url, optional name).
/// Format: `URL` or `URL  Optional Channel Name` (split at first whitespace).
fn parse_sub_line(line: &str) -> Option<(String, Option<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    match line.split_once(char::is_whitespace) {
        Some((url, rest)) => {
            let name = rest.trim();
            let name = (!name.is_empty()).then(|| name.to_string());
            Some((url.to_string(), name))
        }
        None => Some((line.to_string(), None)),
    }
}

pub fn load_subscriptions() -> Vec<String> {
    load_subscriptions_with_names()
        .into_iter()
        .map(|(url, _)| url)
        .collect()
}

/// Like `load_subscriptions` but also returns an inline channel name when the
/// line provides one (`URL  Channel Name`), letting the channels view skip the
/// slow yt-dlp name lookup entirely.
pub fn load_subscriptions_with_names() -> Vec<(String, Option<String>)> {
    let path = crate::config::youtube_subs_file();
    if !path.exists() {
        return vec![];
    }
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(parse_sub_line)
        .collect()
}

/// Extract a Channel from a subscription URL by fetching one video.
pub async fn channel_from_url(url: &str) -> Channel {
    // Fast path: one Innertube browse gives the channel title directly.
    if let Ok((name, _avatar)) = crate::innertube::channel_meta(url).await {
        return Channel {
            name,
            url: url.to_string(),
        };
    }
    // Try to get channel name from yt-dlp with a single video
    if let Ok(json) = run_yt_dlp(url, 1).await {
        let name = json
            .get("channel")
            .or_else(|| json.get("uploader"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !name.is_empty() {
            return Channel {
                name,
                url: url.to_string(),
            };
        }
        // Try first entry
        if let Some(Value::Array(entries)) = json.get("entries") {
            if let Some(first) = entries.first() {
                let name = first
                    .get("channel")
                    .or_else(|| first.get("uploader"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    return Channel {
                        name,
                        url: url.to_string(),
                    };
                }
            }
        }
    }
    // Fallback: use the URL itself as name
    Channel {
        name: url
            .trim_end_matches('/')
            .split('/')
            .next_back()
            .unwrap_or(url)
            .to_string(),
        url: url.to_string(),
    }
}

/// Disk cache of resolved channel names, keyed by subscription URL. Channel
/// names are stable, so this is persisted indefinitely; only newly added
/// subscriptions need a (slow) yt-dlp lookup.
fn load_channel_name_cache() -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(youtube_channel_names_file())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn save_channel_name_cache(map: &std::collections::HashMap<String, String>) {
    let path = youtube_channel_names_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(json) = serde_json::to_string(map) {
        std::fs::write(path, json).ok();
    }
}

pub async fn fetch_channels() -> Result<Vec<Channel>> {
    let subs = load_subscriptions_with_names();
    let cache = load_channel_name_cache();

    // Resolve only subscriptions that have neither an inline name nor a cached
    // one — inline names in the file mean zero yt-dlp work.
    let uncached: Vec<String> = subs
        .iter()
        .filter(|(url, name)| name.is_none() && !cache.contains_key(url))
        .map(|(url, _)| url.clone())
        .collect();

    let mut resolved = std::collections::HashMap::new();
    if !uncached.is_empty() {
        use tokio::task::JoinSet;
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
        let mut set: JoinSet<Channel> = JoinSet::new();
        for url in uncached {
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.ok();
                channel_from_url(&url).await
            });
        }
        while let Some(res) = set.join_next().await {
            if let Ok(ch) = res {
                resolved.insert(ch.url.clone(), ch.name);
            }
        }
    }

    // Build the result list: inline name → cached name → freshly resolved → URL.
    let mut channels: Vec<Channel> = subs
        .into_iter()
        .map(|(url, inline_name)| {
            let name = inline_name
                .or_else(|| cache.get(&url).cloned())
                .or_else(|| resolved.get(&url).cloned())
                .unwrap_or_else(|| url.clone());
            Channel { name, url }
        })
        .collect();

    // Persist the merged cache for next time.
    if !resolved.is_empty() {
        let mut merged = cache;
        merged.extend(resolved);
        save_channel_name_cache(&merged);
    }

    channels.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(channels)
}

/// Disk cache of resolved channel avatar image URLs, keyed by channel URL.
fn load_channel_avatar_cache() -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(youtube_channel_avatars_file())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn save_channel_avatar_cache(map: &std::collections::HashMap<String, String>) {
    let path = youtube_channel_avatars_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(json) = serde_json::to_string(map) {
        std::fs::write(path, json).ok();
    }
}

/// Pick a channel's avatar image URL from a yt-dlp `thumbnails` array.
/// yt-dlp lists both the wide banner and the square avatar; prefer the avatar
/// (explicit `avatar_uncropped`, else the most square image), not the banner.
fn best_avatar_url(json: &Value) -> Option<String> {
    let thumbs = json.get("thumbnails")?.as_array()?;

    let url_of = |t: &Value| t.get("url").and_then(|u| u.as_str()).map(|s| s.to_string());

    // 1. Explicit full-resolution avatar.
    if let Some(t) = thumbs
        .iter()
        .find(|t| t.get("id").and_then(|i| i.as_str()) == Some("avatar_uncropped"))
    {
        if let Some(u) = url_of(t) {
            return Some(u);
        }
    }

    // 2. Most square sized image (avatars are square; banners are wide).
    if let Some(t) = thumbs
        .iter()
        .filter_map(|t| {
            let w = t.get("width").and_then(|w| w.as_i64())?;
            let h = t.get("height").and_then(|h| h.as_i64())?;
            Some((t, (w - h).abs(), w * h))
        })
        // Smallest width/height difference wins; tiebreak on larger area.
        .min_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
        .map(|(t, _, _)| t)
    {
        if let Some(u) = url_of(t) {
            return Some(u);
        }
    }

    // 3. Fallback: last entry.
    thumbs.last().and_then(url_of)
}

/// Caps concurrent yt-dlp avatar resolutions so fast-scrolling the channel list
/// can't spawn a process storm.
fn avatar_resolve_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(3))
}

/// Resolve a channel's avatar image URL, using a disk cache to avoid repeat
/// yt-dlp calls. Only the first lookup for a given channel hits yt-dlp.
pub async fn channel_avatar_url(channel_url: &str) -> Option<String> {
    let cache = load_channel_avatar_cache();
    if let Some(url) = cache.get(channel_url) {
        return Some(url.clone());
    }

    let _permit = avatar_resolve_semaphore().acquire().await.ok()?;
    // Re-check the cache: another task may have resolved this while we waited.
    if let Some(url) = load_channel_avatar_cache().get(channel_url) {
        return Some(url.clone());
    }

    let avatar = match crate::innertube::channel_meta(channel_url).await {
        Ok((_, Some(avatar))) => avatar,
        _ => {
            let json = run_yt_dlp(channel_url, 1).await.ok()?;
            best_avatar_url(&json)?
        }
    };

    let mut merged = load_channel_avatar_cache();
    merged.insert(channel_url.to_string(), avatar.clone());
    save_channel_avatar_cache(&merged);

    Some(avatar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── urlencoding_simple ──────────────────────────────────────────────

    #[test]
    fn urlencoding_ascii_passthrough() {
        assert_eq!(urlencoding_simple("hello"), "hello");
        assert_eq!(urlencoding_simple("foo-bar_baz.qux~"), "foo-bar_baz.qux~");
    }

    #[test]
    fn urlencoding_spaces_become_plus() {
        assert_eq!(urlencoding_simple("hello world"), "hello+world");
    }

    #[test]
    fn urlencoding_special_chars() {
        assert_eq!(urlencoding_simple("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencoding_simple("100%"), "100%25");
    }

    #[test]
    fn urlencoding_non_ascii_encodes_utf8_bytes() {
        // é is U+00E9 → UTF-8 bytes 0xC3 0xA9
        assert_eq!(urlencoding_simple("café"), "caf%C3%A9");
        // ñ is U+00F1 → UTF-8 bytes 0xC3 0xB1
        assert_eq!(urlencoding_simple("niño"), "ni%C3%B1o");
    }

    #[test]
    fn urlencoding_cjk() {
        // 日 is U+65E5 → UTF-8 bytes 0xE6 0x97 0xA5
        assert_eq!(urlencoding_simple("日"), "%E6%97%A5");
    }

    // ── timestamp_to_yyyymmdd ───────────────────────────────────────────

    #[test]
    fn timestamp_epoch_zero() {
        assert_eq!(timestamp_to_yyyymmdd(0), "19700101");
    }

    #[test]
    fn timestamp_known_date() {
        // 2024-01-15 00:00:00 UTC = 1705276800
        assert_eq!(timestamp_to_yyyymmdd(1705276800), "20240115");
    }

    #[test]
    fn timestamp_y2k() {
        // 2000-01-01 00:00:00 UTC = 946684800
        assert_eq!(timestamp_to_yyyymmdd(946684800), "20000101");
    }

    #[test]
    fn timestamp_negative() {
        // 1969-12-31
        assert_eq!(timestamp_to_yyyymmdd(-86400), "19691231");
    }

    // ── parse_search_filter ─────────────────────────────────────────────

    #[test]
    fn search_filter_no_prefix() {
        let (sp, q) = parse_search_filter("rust programming");
        assert_eq!(sp, "");
        assert_eq!(q, "rust programming");
    }

    #[test]
    fn search_filter_with_prefix() {
        let (sp, q) = parse_search_filter(":today rust programming");
        assert_eq!(sp, "EgIIAg%253D%253D");
        assert_eq!(q, "rust programming");
    }

    #[test]
    fn search_filter_prefix_only() {
        let (sp, q) = parse_search_filter(":newest");
        assert_eq!(sp, "CAI%253D");
        assert_eq!(q, "");
    }

    // ── parse_playlist_json ─────────────────────────────────────────────

    #[test]
    fn parse_playlist_empty_entries() {
        let json = json!({"entries": []});
        assert!(parse_playlist_json(&json).is_empty());
    }

    #[test]
    fn parse_playlist_no_entries_key() {
        let json = json!({"foo": "bar"});
        assert!(parse_playlist_json(&json).is_empty());
    }

    #[test]
    fn parse_playlist_single_video_no_entries() {
        let json = json!({
            "id": "abc123",
            "title": "Test Video",
            "url": "https://www.youtube.com/watch?v=abc123",
            "channel": "TestChannel",
            "channel_url": "https://www.youtube.com/c/TestChannel",
            "upload_date": "20240101",
            "duration_string": "5:30"
        });
        let videos = parse_playlist_json(&json);
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].id, "abc123");
        assert_eq!(videos[0].title, "Test Video");
        assert_eq!(videos[0].channel, "TestChannel");
    }

    #[test]
    fn parse_playlist_with_entries() {
        let json = json!({
            "channel": "PlaylistOwner",
            "channel_url": "https://www.youtube.com/c/PlaylistOwner",
            "entries": [
                {
                    "id": "vid1",
                    "title": "First Video",
                    "duration_string": "3:00"
                },
                {
                    "id": "vid2",
                    "title": "Second Video",
                    "channel": "OtherChannel",
                    "duration_string": "10:00"
                }
            ]
        });
        let videos = parse_playlist_json(&json);
        assert_eq!(videos.len(), 2);
        // First entry falls back to root channel
        assert_eq!(videos[0].channel, "PlaylistOwner");
        // Second entry has its own channel
        assert_eq!(videos[1].channel, "OtherChannel");
    }

    #[test]
    fn shorts_url_marks_is_short() {
        let json = json!({
            "id": "s1",
            "title": "Short",
            "url": "https://www.youtube.com/shorts/s1"
        });
        let videos = parse_playlist_json(&json);
        assert!(videos[0].is_short);
    }

    #[test]
    fn shorts_filter_drops_shorts_by_default() {
        let videos = vec![
            Video {
                id: "a".into(),
                is_short: false,
                ..Default::default()
            },
            Video {
                id: "b".into(),
                is_short: true,
                ..Default::default()
            },
        ];
        let kept = apply_shorts_filter(videos.clone(), false);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "a");
        assert_eq!(apply_shorts_filter(videos, true).len(), 2);
    }

    #[test]
    fn parse_playlist_url_fallback_from_id() {
        let json = json!({"id": "xyz", "title": "T"});
        let videos = parse_playlist_json(&json);
        assert_eq!(videos[0].url, "https://www.youtube.com/watch?v=xyz");
    }

    #[test]
    fn parse_playlist_thumbnail_from_thumbnails_array() {
        let json = json!({
            "id": "t1",
            "title": "Thumb test",
            "thumbnails": [
                {"url": "https://example.com/low.jpg?sqp=abc"},
                {"url": "https://example.com/high.jpg?sqp=def"}
            ]
        });
        let videos = parse_playlist_json(&json);
        // Should pick last entry and strip query string
        assert_eq!(videos[0].thumbnail, "https://example.com/high.jpg");
    }

    #[test]
    fn parse_playlist_upload_date_from_timestamp() {
        let json = json!({
            "id": "ts1",
            "title": "Timestamp test",
            "timestamp": 1705276800  // 2024-01-15
        });
        let videos = parse_playlist_json(&json);
        assert_eq!(videos[0].upload_date, "20240115");
    }

    #[test]
    fn parse_playlist_uploader_fallback() {
        let json = json!({
            "uploader": "UploaderName",
            "uploader_url": "https://www.youtube.com/c/UploaderName",
            "entries": [{"id": "v1", "title": "V"}]
        });
        let videos = parse_playlist_json(&json);
        assert_eq!(videos[0].channel, "UploaderName");
    }

    // ── parse_sub_line ──────────────────────────────────────────────────

    #[test]
    fn parse_sub_line_url_only() {
        assert_eq!(
            parse_sub_line("https://youtube.com/channel/UC123"),
            Some(("https://youtube.com/channel/UC123".to_string(), None))
        );
    }

    #[test]
    fn parse_sub_line_with_name() {
        assert_eq!(
            parse_sub_line("https://youtube.com/channel/UC123  My Cool Channel"),
            Some((
                "https://youtube.com/channel/UC123".to_string(),
                Some("My Cool Channel".to_string())
            ))
        );
    }

    #[test]
    fn parse_sub_line_skips_comments_and_blanks() {
        assert_eq!(parse_sub_line("# a comment"), None);
        assert_eq!(parse_sub_line("   "), None);
    }

    // ── best_avatar_url ─────────────────────────────────────────────────

    #[test]
    fn best_avatar_prefers_uncropped() {
        let json = json!({"thumbnails": [
            {"id": "5", "url": "banner.jpg", "width": 2560, "height": 424},
            {"id": "7", "url": "square.jpg", "width": 900, "height": 900},
            {"id": "avatar_uncropped", "url": "avatar.jpg"},
        ]});
        assert_eq!(best_avatar_url(&json).as_deref(), Some("avatar.jpg"));
    }

    #[test]
    fn best_avatar_picks_square_over_banner() {
        // No explicit avatar_uncropped → most square wins, not widest banner.
        let json = json!({"thumbnails": [
            {"id": "5", "url": "banner.jpg", "width": 2560, "height": 424},
            {"id": "7", "url": "square.jpg", "width": 900, "height": 900},
        ]});
        assert_eq!(best_avatar_url(&json).as_deref(), Some("square.jpg"));
    }

    #[test]
    fn best_avatar_fallback_last() {
        let json = json!({"thumbnails": [
            {"id": "a", "url": "one.jpg"},
            {"id": "b", "url": "two.jpg"},
        ]});
        assert_eq!(best_avatar_url(&json).as_deref(), Some("two.jpg"));
    }

    #[test]
    fn parse_sub_line_tab_separated() {
        assert_eq!(
            parse_sub_line("https://youtube.com/@handle\tDisplay Name"),
            Some((
                "https://youtube.com/@handle".to_string(),
                Some("Display Name".to_string())
            ))
        );
    }
}
