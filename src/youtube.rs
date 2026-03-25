use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::{
    youtube_custom_playlists_file, youtube_recent_file, youtube_saved_file,
    youtube_search_history_file,
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

    if output.stdout.is_empty() {
        anyhow::bail!("yt-dlp returned no output");
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
    let view_count = j
        .get("view_count")
        .and_then(|v| v.as_u64());
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
    let json = run_yt_dlp("https://www.youtube.com/gaming", limit).await?;
    Ok(parse_playlist_json(&json))
}

pub async fn fetch_search(query: &str, sp: &str, limit: u32) -> Result<Vec<Video>> {
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

fn urlencoding_simple(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

pub async fn fetch_playlist(playlist_url: &str, limit: u32) -> Result<Vec<Video>> {
    let json = run_yt_dlp(playlist_url, limit).await?;
    Ok(parse_playlist_json(&json))
}

/// Search YouTube for channels matching `query`.
/// Uses the YouTube search channel-type filter (sp=EgIQAg==).
pub async fn search_channels(query: &str, limit: u32) -> Result<Vec<crate::models::Channel>> {
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

    for url in subs {
        let sem = sem.clone();
        let cutoff = cutoff_date.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let tab_url = format!("{}/videos", url.trim_end_matches('/'));
            match run_yt_dlp(&tab_url, playlist_end).await {
                Ok(json) => {
                    let videos = parse_playlist_json(&json);
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

    all.sort_by(|a, b| {
        // Newest first. Videos without a timestamp sink to the bottom.
        match (b.timestamp, a.timestamp) {
            (Some(bt), Some(at)) => bt.cmp(&at),
            (Some(_), None) => std::cmp::Ordering::Greater, // a (no ts) after b (has ts)
            (None, Some(_)) => std::cmp::Ordering::Less,    // a (has ts) before b (no ts)
            (None, None) => b.upload_date.cmp(&a.upload_date),
        }
    });
    Ok(all)
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
    let path = crate::config::youtube_subs_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    use std::io::Write;
    writeln!(file, "{}", url.trim())?;
    Ok(())
}

pub fn load_subscriptions() -> Vec<String> {
    let path = crate::config::youtube_subs_file();
    if !path.exists() {
        return vec![];
    }
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Extract a Channel from a subscription URL by fetching one video.
pub async fn channel_from_url(url: &str) -> Channel {
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
            .last()
            .unwrap_or(url)
            .to_string(),
        url: url.to_string(),
    }
}

pub async fn fetch_channels() -> Result<Vec<Channel>> {
    let subs = load_subscriptions();
    use tokio::task::JoinSet;
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
    let mut set: JoinSet<Channel> = JoinSet::new();
    for url in subs {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            channel_from_url(&url).await
        });
    }
    let mut channels = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(ch) = res {
            channels.push(ch);
        }
    }
    channels.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(channels)
}

