use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::peertube_subs_file;
use crate::models::{Channel, Video};

pub fn normalize_instance(input: &str) -> String {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return String::new();
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{}", s)
    }
}

pub fn instance_host(instance: &str) -> String {
    instance
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

async fn api_get(url: &str) -> Result<Value> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("PeerTube request failed: {}", url))?;
    if !resp.status().is_success() {
        anyhow::bail!("PeerTube returned {} for {}", resp.status(), url);
    }
    resp.json()
        .await
        .context("Failed to parse the PeerTube response")
}

fn data_array(json: &Value) -> Vec<Value> {
    json.get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

pub fn iso8601_to_timestamp(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: i64 = d.next()?.parse().ok()?;
    let da: i64 = d.next()?.parse().ok()?;
    let time = time.trim_end_matches('Z');
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t
        .next()
        .map(|v| v.split(['.', '+', '-']).next().unwrap_or("0"))
        .unwrap_or("0")
        .parse()
        .ok()?;

    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    Some(days * 86_400 + h * 3600 + mi * 60 + sec)
}

fn str_at(v: &Value, path: &str) -> String {
    v.pointer(path)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn avatar_url(v: &Value, host: &str) -> Option<String> {
    let best = v
        .get("avatars")
        .and_then(|a| a.as_array())
        .and_then(|avatars| {
            avatars
                .iter()
                .filter(|a| a.get("width").and_then(|w| w.as_u64()).unwrap_or(0) <= 600)
                .max_by_key(|a| a.get("width").and_then(|w| w.as_u64()).unwrap_or(0))
        })
        .and_then(|a| a.get("path"))
        .and_then(|p| p.as_str());
    let path = best.or_else(|| v.pointer("/avatar/path").and_then(|p| p.as_str()))?;
    Some(absolute(path, host))
}

fn absolute(path: &str, host: &str) -> String {
    if path.starts_with("http") {
        path.to_string()
    } else {
        format!("https://{}{}", host, path)
    }
}

fn video_from_json(v: &Value, asset_host: Option<&str>) -> Option<Video> {
    let id = v
        .get("uuid")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("shortUUID").and_then(|x| x.as_str()))?
        .to_string();
    let title = v.get("name").and_then(|x| x.as_str())?.to_string();

    let channel_host = v
        .pointer("/channel/host")
        .and_then(|x| x.as_str())
        .or_else(|| v.pointer("/account/host").and_then(|x| x.as_str()))
        .unwrap_or("")
        .to_string();
    let host = asset_host.unwrap_or(&channel_host).to_string();

    let short = v
        .get("shortUUID")
        .and_then(|x| x.as_str())
        .unwrap_or(&id)
        .to_string();
    let url = match v.get("url").and_then(|x| x.as_str()) {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => format!("https://{}/w/{}", channel_host, short),
    };

    let is_live = v.get("isLive").and_then(|x| x.as_bool()).unwrap_or(false);
    let duration = v.get("duration").and_then(|x| x.as_u64()).unwrap_or(0);
    let duration_string = if is_live {
        "LIVE".to_string()
    } else {
        format_duration(duration)
    };

    let published = v
        .get("publishedAt")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("createdAt").and_then(|x| x.as_str()))
        .unwrap_or_default();
    let timestamp = iso8601_to_timestamp(published);

    let channel_name = match str_at(v, "/channel/displayName") {
        s if !s.is_empty() => s,
        _ => str_at(v, "/account/displayName"),
    };
    let channel_url = match str_at(v, "/channel/url") {
        s if !s.is_empty() => s,
        _ => {
            let name = str_at(v, "/channel/name");
            if name.is_empty() {
                String::new()
            } else {
                format!("https://{}/c/{}", channel_host, name)
            }
        }
    };

    let thumbnail = v
        .get("thumbnailPath")
        .or_else(|| v.get("previewPath"))
        .and_then(|x| x.as_str())
        .map(|p| absolute(p, &host))
        .unwrap_or_default();

    Some(Video {
        id,
        title,
        url,
        channel: channel_name,
        channel_url,
        upload_date: published.split('T').next().unwrap_or("").replace('-', ""),
        duration_string,
        view_count: v.get("views").and_then(|x| x.as_u64()),
        thumbnail,
        playlist_url: None,
        playlist_title: None,
        description: v
            .get("description")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        timestamp,
        is_short: false,
    })
}

fn videos_from(json: &Value, asset_host: Option<&str>) -> Vec<Video> {
    data_array(json)
        .iter()
        .filter_map(|v| video_from_json(v, asset_host))
        .collect()
}

fn channel_from_json(v: &Value) -> Option<Channel> {
    let name = v.get("name").and_then(|x| x.as_str())?.to_string();
    let host = v.get("host").and_then(|x| x.as_str()).unwrap_or("");
    if host.is_empty() {
        return None;
    }
    let display = v
        .get("displayName")
        .and_then(|x| x.as_str())
        .unwrap_or(&name)
        .to_string();
    let url = match v.get("url").and_then(|x| x.as_str()) {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => format!("https://{}/c/{}", host, name),
    };
    Some(Channel {
        name: format!("{} ({}@{})", display, name, host),
        url,
        avatar: avatar_url(v, host),
    })
}

// ---------------------------------------------------------------------------
// Fetchers
// ---------------------------------------------------------------------------

pub async fn fetch_trending(instance: &str, count: u32) -> Result<Vec<Video>> {
    let url = format!(
        "{}/api/v1/videos?sort=-trending&count={}&nsfw=false&skipCount=true",
        instance.trim_end_matches('/'),
        count.min(100)
    );
    let json = api_get(&url).await?;
    Ok(videos_from(&json, Some(&instance_host(instance))))
}

pub async fn fetch_recent(instance: &str, count: u32) -> Result<Vec<Video>> {
    let url = format!(
        "{}/api/v1/videos?sort=-publishedAt&count={}&nsfw=false&skipCount=true",
        instance.trim_end_matches('/'),
        count.min(100)
    );
    let json = api_get(&url).await?;
    Ok(videos_from(&json, Some(&instance_host(instance))))
}

pub async fn search_videos(index: &str, query: &str, count: u32) -> Result<Vec<Video>> {
    let url = format!(
        "{}/api/v1/search/videos?search={}&count={}&sort=-match&nsfw=false",
        index.trim_end_matches('/'),
        crate::youtube::urlencoding_simple(query),
        count.min(100)
    );
    let json = api_get(&url).await?;
    Ok(videos_from(&json, None))
}

pub async fn search_channels(index: &str, query: &str, count: u32) -> Result<Vec<Channel>> {
    let url = format!(
        "{}/api/v1/search/video-channels?search={}&count={}",
        index.trim_end_matches('/'),
        crate::youtube::urlencoding_simple(query),
        count.min(100)
    );
    let json = api_get(&url).await?;
    let channels: Vec<Channel> = data_array(&json)
        .iter()
        .filter_map(channel_from_json)
        .collect();
    if channels.is_empty() {
        anyhow::bail!("No channels found for \"{}\"", query);
    }
    Ok(channels)
}

pub async fn fetch_channel_videos(handle: &str, count: u32) -> Result<Vec<Video>> {
    let (name, host) = split_handle(handle)
        .with_context(|| format!("Not a PeerTube channel handle: {}", handle))?;
    let url = format!(
        "https://{}/api/v1/video-channels/{}/videos?sort=-publishedAt&count={}&nsfw=false&skipCount=true",
        host,
        name,
        count.min(100)
    );
    let json = api_get(&url).await?;
    Ok(videos_from(&json, Some(&host)))
}

pub async fn fetch_channel_meta(handle: &str) -> Result<Channel> {
    let (name, host) = split_handle(handle)
        .with_context(|| format!("Not a PeerTube channel handle: {}", handle))?;
    let url = format!("https://{}/api/v1/video-channels/{}", host, name);
    let json = api_get(&url).await?;
    channel_from_json(&json).context("Channel not found")
}

pub async fn fetch_subscription_feed(
    subs: Vec<String>,
    per_channel: u32,
    max_concurrent: usize,
) -> Result<Vec<Video>> {
    use tokio::task::JoinSet;

    let mut set: JoinSet<Vec<Video>> = JoinSet::new();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrent));

    for handle in subs {
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            fetch_channel_videos(&handle, per_channel)
                .await
                .unwrap_or_default()
        });
    }

    let mut all = Vec::new();
    while let Some(result) = set.join_next().await {
        if let Ok(videos) = result {
            all.extend(videos);
        }
    }
    crate::youtube::sort_videos_newest_first(&mut all);
    Ok(all)
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

pub fn parse_handle(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.starts_with('#') {
        return None;
    }
    if !s.contains('/') {
        let (name, host) = s.trim_start_matches('@').split_once('@')?;
        if name.is_empty() || host.is_empty() {
            return None;
        }
        return Some(format!("{}@{}", name, host));
    }
    let stripped = s
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let (host, rest) = stripped.split_once('/')?;
    let segments: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let name = segments
        .iter()
        .position(|p| matches!(*p, "c" | "video-channels" | "a" | "accounts"))
        .and_then(|i| segments.get(i + 1))
        .or_else(|| segments.first())
        .map(|n| n.split('?').next().unwrap_or(n))?;
    if name.is_empty() {
        return None;
    }
    match name.split_once('@') {
        Some((n, h)) => Some(format!("{}@{}", n, h)),
        None => Some(format!("{}@{}", name, host)),
    }
}

pub fn split_handle(handle: &str) -> Option<(String, String)> {
    let h = parse_handle(handle)?;
    let (name, host) = h.split_once('@')?;
    Some((name.to_string(), host.to_string()))
}

pub fn channel_url(handle: &str) -> String {
    match split_handle(handle) {
        Some((name, host)) => format!("https://{}/c/{}", host, name),
        None => handle.to_string(),
    }
}

pub fn load_subs() -> Vec<String> {
    let path = peertube_subs_file();
    if !path.exists() {
        return vec![];
    }
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(parse_handle)
        .collect()
}

pub fn is_subscribed(handle: &str) -> bool {
    let Some(target) = parse_handle(handle) else {
        return false;
    };
    let target = target.to_lowercase();
    load_subs().iter().any(|s| s.to_lowercase() == target)
}

pub fn subscribe(handle: &str) -> Result<()> {
    let handle = parse_handle(handle).context("Not a PeerTube channel")?;
    if is_subscribed(&handle) {
        return Ok(());
    }
    let path = peertube_subs_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&handle);
    content.push('\n');
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn unsubscribe(handle: &str) -> Result<()> {
    let path = peertube_subs_file();
    if !path.exists() {
        return Ok(());
    }
    let target = parse_handle(handle)
        .context("Not a PeerTube channel")?
        .to_lowercase();
    let kept: Vec<String> = std::fs::read_to_string(&path)?
        .lines()
        .filter(|l| parse_handle(l).map(|h| h.to_lowercase()) != Some(target.clone()))
        .map(|l| l.to_string())
        .collect();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(&path, out)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Feed cache
// ---------------------------------------------------------------------------

const FEED_CACHE_MAX_AGE_SECS: u64 = 15 * 60;

#[derive(serde::Serialize, serde::Deserialize)]
struct FeedCache {
    cached_at: u64,
    videos: Vec<Video>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn load_feed_cache_with_age() -> Option<(Vec<Video>, bool)> {
    let content = std::fs::read_to_string(crate::config::peertube_feed_cache_file()).ok()?;
    let cache: FeedCache = serde_json::from_str(&content).ok()?;
    let fresh = now_secs().saturating_sub(cache.cached_at) < FEED_CACHE_MAX_AGE_SECS;
    Some((cache.videos, fresh))
}

pub fn save_feed_cache(videos: &[Video]) {
    let path = crate::config::peertube_feed_cache_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let cache = FeedCache {
        cached_at: now_secs(),
        videos: videos.to_vec(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        std::fs::write(path, json).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_instance_adds_scheme_and_trims() {
        assert_eq!(normalize_instance("makertube.net"), "https://makertube.net");
        assert_eq!(
            normalize_instance(" https://tilvids.com/ "),
            "https://tilvids.com"
        );
        assert_eq!(
            normalize_instance("http://localhost:9000"),
            "http://localhost:9000"
        );
        assert_eq!(normalize_instance("  "), "");
    }

    #[test]
    fn instance_host_strips_scheme() {
        assert_eq!(instance_host("https://makertube.net/"), "makertube.net");
        assert_eq!(instance_host("makertube.net"), "makertube.net");
    }

    #[test]
    fn format_duration_short_and_long() {
        assert_eq!(format_duration(75), "1:15");
        assert_eq!(format_duration(3725), "1:02:05");
        assert_eq!(format_duration(0), "0:00");
    }

    #[test]
    fn iso8601_parses_utc() {
        assert_eq!(iso8601_to_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            iso8601_to_timestamp("2024-05-12T10:33:21.000Z"),
            Some(1_715_510_001)
        );
        assert_eq!(iso8601_to_timestamp("garbage"), None);
    }

    #[test]
    fn parse_handle_accepts_handles_and_urls() {
        assert_eq!(
            parse_handle("blender@makertube.net").as_deref(),
            Some("blender@makertube.net")
        );
        assert_eq!(
            parse_handle("@blender@makertube.net").as_deref(),
            Some("blender@makertube.net")
        );
        assert_eq!(
            parse_handle("https://makertube.net/c/blender/videos").as_deref(),
            Some("blender@makertube.net")
        );
        assert_eq!(
            parse_handle("https://makertube.net/c/blender").as_deref(),
            Some("blender@makertube.net")
        );
        assert_eq!(
            parse_handle("https://tilvids.com/video-channels/blender@makertube.net").as_deref(),
            Some("blender@makertube.net")
        );
        assert_eq!(parse_handle("# comment"), None);
        assert_eq!(parse_handle(""), None);
    }

    #[test]
    fn split_handle_returns_name_and_host() {
        assert_eq!(
            split_handle("blender@makertube.net"),
            Some(("blender".to_string(), "makertube.net".to_string()))
        );
    }

    #[test]
    fn video_from_json_maps_fields() {
        let json = serde_json::json!({
            "uuid": "abc-123",
            "shortUUID": "sh0rt",
            "name": "A video",
            "duration": 3725,
            "views": 42,
            "publishedAt": "2024-05-12T10:33:21.000Z",
            "thumbnailPath": "/lazy-static/previews/x.jpg",
            "description": "hello",
            "channel": {
                "name": "blender",
                "displayName": "Blender",
                "host": "makertube.net",
                "url": "https://makertube.net/c/blender"
            }
        });
        let v = video_from_json(&json, None).unwrap();
        assert_eq!(v.id, "abc-123");
        assert_eq!(v.url, "https://makertube.net/w/sh0rt");
        assert_eq!(v.channel, "Blender");
        assert_eq!(v.duration_string, "1:02:05");
        assert_eq!(v.upload_date, "20240512");
        assert_eq!(v.view_count, Some(42));
        assert_eq!(
            v.thumbnail,
            "https://makertube.net/lazy-static/previews/x.jpg"
        );
        assert!(!v.is_short);
    }

    #[test]
    fn video_from_json_marks_live_and_uses_asset_host() {
        let json = serde_json::json!({
            "uuid": "abc",
            "name": "Live now",
            "isLive": true,
            "duration": 0,
            "url": "https://origin.tld/w/abc",
            "thumbnailPath": "/static/thumbnails/x.jpg",
            "channel": { "name": "c", "displayName": "C", "host": "origin.tld" }
        });
        let v = video_from_json(&json, Some("mirror.tld")).unwrap();
        assert_eq!(v.duration_string, "LIVE");
        assert_eq!(v.url, "https://origin.tld/w/abc");
        assert_eq!(v.thumbnail, "https://mirror.tld/static/thumbnails/x.jpg");
    }

    #[test]
    fn channel_from_json_builds_handle_name() {
        let json = serde_json::json!({
            "name": "blender",
            "displayName": "Blender",
            "host": "makertube.net",
            "avatars": [
                { "path": "/lazy-static/avatars/small.png", "width": 48 },
                { "path": "/lazy-static/avatars/a.png", "width": 600 },
                { "path": "/lazy-static/avatars/huge.png", "width": 1500 }
            ]
        });
        let c = channel_from_json(&json).unwrap();
        assert_eq!(c.name, "Blender (blender@makertube.net)");
        assert_eq!(c.url, "https://makertube.net/c/blender");
        assert_eq!(
            c.avatar.as_deref(),
            Some("https://makertube.net/lazy-static/avatars/a.png")
        );
    }
}
