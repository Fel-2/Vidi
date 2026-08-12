//! Direct YouTube Innertube (`youtubei/v1`) client.
//!
//! Fetches list metadata (search, trending, channel tabs, playlists) with a
//! single HTTP request instead of spawning yt-dlp, which is 10–50× faster.
//! yt-dlp remains the fallback when parsing fails and is still used for
//! stream URLs and downloads.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::models::{Channel, Video};

const INNERTUBE_BASE: &str = "https://www.youtube.com/youtubei/v1";
const CLIENT_VERSION: &str = "2.20250520.01.00";

// ---------------------------------------------------------------------------
// HTTP plumbing
// ---------------------------------------------------------------------------

fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        // One connection per parallel fetch: multiplexing the feed's ~140
        // browse responses over a single HTTP/2 connection is measurably slower.
        reqwest::Client::builder()
            .http1_only()
            .pool_max_idle_per_host(32)
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
            .build()
            .expect("reqwest client")
    })
}

fn client_context() -> Value {
    json!({
        "client": {
            "clientName": "WEB",
            "clientVersion": CLIENT_VERSION,
            "hl": "en",
            "gl": "US",
        }
    })
}

async fn post(endpoint: &str, mut body: Value) -> Result<Value> {
    body["context"] = client_context();
    let url = format!("{}/{}?prettyPrint=false", INNERTUBE_BASE, endpoint);
    let resp = http_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("innertube {} request failed", endpoint))?;
    if !resp.status().is_success() {
        anyhow::bail!("innertube {} returned HTTP {}", endpoint, resp.status());
    }
    resp.json()
        .await
        .with_context(|| format!("innertube {} returned invalid JSON", endpoint))
}

// ---------------------------------------------------------------------------
// Generic response walking
// ---------------------------------------------------------------------------

/// Recursively collect every object stored under `key` anywhere in the tree,
/// in document order. Innertube nests renderers differently per surface, but
/// the leaf renderer names are stable, so this stays robust across layouts.
fn collect_key<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == key {
                    out.push(val);
                }
                collect_key(val, key, out);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                collect_key(val, key, out);
            }
        }
        _ => {}
    }
}

/// First continuation token found in the tree, if any.
fn find_continuation(v: &Value) -> Option<String> {
    let mut tokens = Vec::new();
    collect_key(v, "continuationCommand", &mut tokens);
    tokens
        .first()
        .and_then(|c| c.get("token"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}

/// Text from either `{"simpleText": …}` or `{"runs": [{"text": …}, …]}`.
fn text_of(v: &Value) -> Option<String> {
    if let Some(s) = v.get("simpleText").and_then(|s| s.as_str()) {
        return Some(s.to_string());
    }
    let runs = v.get("runs")?.as_array()?;
    let joined: String = runs
        .iter()
        .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
        .collect();
    (!joined.is_empty()).then_some(joined)
}

fn last_thumbnail(v: &Value) -> Option<String> {
    v.get("thumbnail")
        .and_then(|t| t.get("thumbnails"))
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(|u| u.split('?').next().unwrap_or(u).to_string())
}

// ---------------------------------------------------------------------------
// Field parsing
// ---------------------------------------------------------------------------

/// Parse "3 days ago" / "Streamed 2 weeks ago" / "Premiered 1 hour ago" into
/// an approximate Unix timestamp, mirroring yt-dlp's `approximate_date`.
pub fn parse_relative_time(text: &str, now: i64) -> Option<i64> {
    let mut words = text.split_whitespace().peekable();
    // Skip prefixes like "Streamed"/"Premiered".
    let mut n: Option<i64> = None;
    let mut unit: Option<&str> = None;
    while let Some(w) = words.next() {
        if let Ok(parsed) = w.parse::<i64>() {
            n = Some(parsed);
            unit = words.next();
            break;
        }
    }
    let n = n?;
    let unit = unit?.trim_end_matches('s');
    let secs = match unit {
        "second" => 1,
        "minute" => 60,
        "hour" => 3600,
        "day" => 86400,
        "week" => 7 * 86400,
        "month" => 30 * 86400,
        "year" => 365 * 86400,
        _ => return None,
    };
    Some(now - n * secs)
}

/// Parse "1,234,567 views" → 1234567. Returns None for "No views" etc.
pub fn parse_view_count(text: &str) -> Option<u64> {
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Convert the app's double-URL-encoded `sp` filter ("EgIIAg%253D%253D") into
/// the raw base64 `params` value Innertube expects ("EgIIAg==").
pub fn decode_sp(sp: &str) -> String {
    sp.replace("%253D", "=").replace("%3D", "=")
}

// ---------------------------------------------------------------------------
// Renderer → model conversion
// ---------------------------------------------------------------------------

fn channel_url_from(renderer: &Value) -> String {
    // Prefer the canonical /channel/UC… URL from the byline browse endpoint.
    for key in ["ownerText", "longBylineText", "shortBylineText"] {
        if let Some(byline) = renderer.get(key) {
            let mut endpoints = Vec::new();
            collect_key(byline, "browseEndpoint", &mut endpoints);
            if let Some(ep) = endpoints.first() {
                if let Some(base) = ep.get("canonicalBaseUrl").and_then(|u| u.as_str()) {
                    return format!("https://www.youtube.com{}", base);
                }
                if let Some(id) = ep.get("browseId").and_then(|i| i.as_str()) {
                    return format!("https://www.youtube.com/channel/{}", id);
                }
            }
        }
    }
    String::new()
}

fn channel_name_from(renderer: &Value) -> String {
    for key in ["ownerText", "longBylineText", "shortBylineText"] {
        if let Some(name) = renderer.get(key).and_then(text_of) {
            return name;
        }
    }
    String::new()
}

/// Detect whether a `videoRenderer` is actually a Short: it either navigates
/// to a reel watch endpoint or carries the SHORTS time-status overlay.
fn renderer_is_short(r: &Value) -> bool {
    let mut reels = Vec::new();
    collect_key(r, "reelWatchEndpoint", &mut reels);
    if !reels.is_empty() {
        return true;
    }
    r.get("thumbnailOverlays")
        .and_then(|o| o.as_array())
        .map(|overlays| {
            overlays.iter().any(|o| {
                o.pointer("/thumbnailOverlayTimeStatusRenderer/style")
                    .and_then(|s| s.as_str())
                    == Some("SHORTS")
            })
        })
        .unwrap_or(false)
}

/// Convert a `videoRenderer` / `playlistVideoRenderer` / `gridVideoRenderer`
/// object into a Video. Returns None when there is no videoId.
fn video_from_renderer(r: &Value, now: i64) -> Option<Video> {
    let id = r.get("videoId")?.as_str()?.to_string();
    let title = r
        .get("title")
        .and_then(text_of)
        .unwrap_or_else(|| "(no title)".to_string());
    let timestamp = r
        .get("publishedTimeText")
        .and_then(text_of)
        .and_then(|t| parse_relative_time(&t, now));
    let duration_string = r.get("lengthText").and_then(text_of).unwrap_or_default();
    let view_count = r
        .get("viewCountText")
        .and_then(text_of)
        .and_then(|t| parse_view_count(&t));
    let thumbnail =
        last_thumbnail(r).unwrap_or_else(|| format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", id));

    Some(Video {
        url: format!("https://www.youtube.com/watch?v={}", id),
        title,
        channel: channel_name_from(r),
        channel_url: channel_url_from(r),
        upload_date: timestamp
            .map(crate::youtube::timestamp_to_yyyymmdd)
            .unwrap_or_default(),
        duration_string,
        view_count,
        thumbnail,
        playlist_url: None,
        playlist_title: None,
        description: r
            .get("detailedMetadataSnippets")
            .and_then(|s| s.as_array())
            .and_then(|arr| arr.first())
            .and_then(|s| s.get("snippetText"))
            .and_then(text_of),
        timestamp,
        is_short: renderer_is_short(r),
        id,
    })
}

/// Shorts tabs use `shortsLockupViewModel` instead of a videoRenderer.
fn video_from_shorts_lockup(r: &Value, _now: i64) -> Option<Video> {
    let mut endpoints = Vec::new();
    collect_key(r, "reelWatchEndpoint", &mut endpoints);
    let id = endpoints
        .first()
        .and_then(|e| e.get("videoId"))
        .and_then(|i| i.as_str())?
        .to_string();
    let title = r
        .pointer("/overlayMetadata/primaryText/content")
        .and_then(|t| t.as_str())
        .unwrap_or("(short)")
        .to_string();
    let view_count = r
        .pointer("/overlayMetadata/secondaryText/content")
        .and_then(|t| t.as_str())
        .and_then(parse_short_view_count);
    Some(Video {
        url: format!("https://www.youtube.com/watch?v={}", id),
        title,
        thumbnail: format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", id),
        view_count,
        is_short: true,
        ..Default::default()
    })
    .map(|mut v| {
        v.id = id;
        v
    })
}

/// Channel tabs return `lockupViewModel` instead of a gridVideoRenderer.
/// Playlist and channel lockups share the shape, so skip anything that is not
/// a video or short.
fn video_from_lockup(r: &Value, now: i64) -> Option<Video> {
    let content_type = r.get("contentType").and_then(|t| t.as_str()).unwrap_or("");
    let is_short = content_type == "LOCKUP_CONTENT_TYPE_SHORTS";
    if content_type != "LOCKUP_CONTENT_TYPE_VIDEO" && !is_short {
        return None;
    }
    let id = r.get("contentId")?.as_str()?.to_string();
    let meta = r.pointer("/metadata/lockupMetadataViewModel");
    let title = meta
        .and_then(|m| m.pointer("/title/content"))
        .and_then(|t| t.as_str())
        .unwrap_or("(no title)")
        .to_string();

    // Metadata rows carry "672K views" and "1 day ago", plus a channel byline
    // on surfaces that have one (channel tabs omit it).
    let mut parts = Vec::new();
    if let Some(rows) = meta.and_then(|m| m.get("metadata")) {
        collect_key(rows, "metadataParts", &mut parts);
    }
    let texts: Vec<&Value> = parts
        .iter()
        .filter_map(|p| p.as_array())
        .flatten()
        .filter_map(|p| p.get("text"))
        .collect();

    let mut view_count = None;
    let mut timestamp = None;
    let mut channel = String::new();
    let mut channel_url = String::new();
    for t in texts {
        let Some(content) = t.get("content").and_then(|c| c.as_str()) else {
            continue;
        };
        // A byline part links to the channel; plain parts are views/age.
        if let Some(ep) = t
            .pointer("/commandRuns/0/onTap/innertubeCommand/browseEndpoint")
            .filter(|_| channel.is_empty())
        {
            channel = content.to_string();
            channel_url = ep
                .get("canonicalBaseUrl")
                .and_then(|u| u.as_str())
                .map(|base| format!("https://www.youtube.com{}", base))
                .or_else(|| {
                    ep.get("browseId")
                        .and_then(|i| i.as_str())
                        .map(|id| format!("https://www.youtube.com/channel/{}", id))
                })
                .unwrap_or_default();
        } else if view_count.is_none() && content.contains("view") {
            view_count = parse_short_view_count(content);
        } else if timestamp.is_none() {
            timestamp = parse_relative_time(content, now);
        }
    }

    // The duration lives in the thumbnail badge ("7:02"); live entries carry
    // text like "LIVE" instead.
    let mut badges = Vec::new();
    if let Some(image) = r.get("contentImage") {
        collect_key(image, "thumbnailBadgeViewModel", &mut badges);
    }
    let duration_string = badges
        .iter()
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .find(|t| t.contains(':') || t.eq_ignore_ascii_case("LIVE"))
        .unwrap_or_default()
        .to_string();

    let thumbnail = r
        .pointer("/contentImage/thumbnailViewModel/image/sources")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.last())
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .map(|u| u.split('?').next().unwrap_or(u).to_string())
        .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", id));

    Some(Video {
        url: format!("https://www.youtube.com/watch?v={}", id),
        title,
        channel,
        channel_url,
        upload_date: timestamp
            .map(crate::youtube::timestamp_to_yyyymmdd)
            .unwrap_or_default(),
        duration_string,
        view_count,
        thumbnail,
        timestamp,
        is_short,
        id,
        ..Default::default()
    })
}

/// Parse short-form counts like "1.2M views" / "987K views" / "512 views".
pub fn parse_short_view_count(text: &str) -> Option<u64> {
    let first = text.split_whitespace().next()?;
    let (num_part, mult) = match first.chars().last()? {
        'K' | 'k' => (&first[..first.len() - 1], 1_000f64),
        'M' | 'm' => (&first[..first.len() - 1], 1_000_000f64),
        'B' | 'b' => (&first[..first.len() - 1], 1_000_000_000f64),
        _ => (first, 1f64),
    };
    let n: f64 = num_part.replace(',', "").parse().ok()?;
    Some((n * mult) as u64)
}

fn videos_from_response(resp: &Value, now: i64) -> Vec<Video> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for key in [
        "videoRenderer",
        "gridVideoRenderer",
        "playlistVideoRenderer",
    ] {
        let mut renderers = Vec::new();
        collect_key(resp, key, &mut renderers);
        for r in renderers {
            if let Some(v) = video_from_renderer(r, now) {
                if seen.insert(v.id.clone()) {
                    out.push(v);
                }
            }
        }
    }
    let mut shorts = Vec::new();
    collect_key(resp, "shortsLockupViewModel", &mut shorts);
    for r in shorts {
        if let Some(v) = video_from_shorts_lockup(r, now) {
            if seen.insert(v.id.clone()) {
                out.push(v);
            }
        }
    }
    let mut lockups = Vec::new();
    collect_key(resp, "lockupViewModel", &mut lockups);
    for r in lockups {
        if let Some(v) = video_from_lockup(r, now) {
            if seen.insert(v.id.clone()) {
                out.push(v);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Continuation loop
// ---------------------------------------------------------------------------

/// Run `browse`/`search` plus as many continuation pages as needed to reach
/// `limit` items (max 5 extra pages as a safety stop).
async fn paginate(endpoint: &str, first_body: Value, limit: usize) -> Result<Vec<Video>> {
    let now = now_unix();
    let mut resp = post(endpoint, first_body).await?;
    let mut videos = videos_from_response(&resp, now);
    let mut pages = 0;
    while videos.len() < limit && pages < 5 {
        let Some(token) = find_continuation(&resp) else {
            break;
        };
        resp = post(endpoint, json!({ "continuation": token })).await?;
        let more = videos_from_response(&resp, now);
        if more.is_empty() {
            break;
        }
        let existing: std::collections::HashSet<String> =
            videos.iter().map(|v| v.id.clone()).collect();
        videos.extend(more.into_iter().filter(|v| !existing.contains(&v.id)));
        pages += 1;
    }
    videos.truncate(limit);
    Ok(videos)
}

// ---------------------------------------------------------------------------
// Public fetchers
// ---------------------------------------------------------------------------

pub async fn search_videos(query: &str, sp: &str, limit: usize) -> Result<Vec<Video>> {
    let mut body = json!({ "query": query });
    let params = decode_sp(sp);
    if !params.is_empty() {
        body["params"] = json!(params);
    }
    let videos = paginate("search", body, limit).await?;
    if videos.is_empty() {
        anyhow::bail!("innertube search returned no videos");
    }
    Ok(videos)
}

pub async fn search_channels(query: &str, limit: usize) -> Result<Vec<Channel>> {
    // sp=EgIQAg== is the "Type: Channel" filter.
    let resp = post("search", json!({ "query": query, "params": "EgIQAg==" })).await?;
    let mut renderers = Vec::new();
    collect_key(&resp, "channelRenderer", &mut renderers);
    let channels: Vec<Channel> = renderers
        .iter()
        .filter_map(|r| {
            let name = r.get("title").and_then(text_of)?;
            let url = r
                .pointer("/navigationEndpoint/browseEndpoint/canonicalBaseUrl")
                .and_then(|u| u.as_str())
                .map(|base| format!("https://www.youtube.com{}", base))
                .or_else(|| {
                    r.get("channelId")
                        .and_then(|i| i.as_str())
                        .map(|id| format!("https://www.youtube.com/channel/{}", id))
                })?;
            Some(Channel { name, url })
        })
        .take(limit)
        .collect();
    if channels.is_empty() {
        anyhow::bail!("innertube channel search returned no channels");
    }
    Ok(channels)
}

pub async fn trending(limit: usize) -> Result<Vec<Video>> {
    let videos = paginate("browse", json!({ "browseId": "FEtrending" }), limit).await?;
    if videos.is_empty() {
        anyhow::bail!("innertube trending returned no videos");
    }
    Ok(videos)
}

pub async fn playlist_videos(playlist_id: &str, limit: usize) -> Result<Vec<Video>> {
    let browse_id = if playlist_id.starts_with("VL") {
        playlist_id.to_string()
    } else {
        format!("VL{}", playlist_id)
    };
    let mut videos = paginate("browse", json!({ "browseId": browse_id }), limit).await?;
    if videos.is_empty() {
        anyhow::bail!("innertube playlist returned no videos");
    }
    let watch_url = format!("https://www.youtube.com/playlist?list={}", playlist_id);
    for v in &mut videos {
        v.playlist_url = Some(watch_url.clone());
    }
    Ok(videos)
}

// ---------------------------------------------------------------------------
// Channel browseId resolution (disk-cached)
// ---------------------------------------------------------------------------

fn browse_id_cache_file() -> std::path::PathBuf {
    crate::config::youtube_cache_dir().join("browse_ids.json")
}

fn load_browse_id_cache() -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(browse_id_cache_file())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn save_browse_id_cache(map: &std::collections::HashMap<String, String>) {
    let path = browse_id_cache_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(json) = serde_json::to_string(map) {
        std::fs::write(path, json).ok();
    }
}

/// Extract the UC… browseId straight from a /channel/ URL, if present.
pub fn browse_id_from_url(url: &str) -> Option<String> {
    let rest = url.split("/channel/").nth(1)?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    id.starts_with("UC").then_some(id)
}

/// Resolve any channel URL (/channel/UC…, /@handle, /c/…, /user/…) to its
/// UC… browseId, using the navigation/resolve_url endpoint with a disk cache.
pub async fn resolve_browse_id(channel_url: &str) -> Result<String> {
    let url = channel_url.trim_end_matches('/');
    if let Some(id) = browse_id_from_url(url) {
        return Ok(id);
    }
    if let Some(id) = load_browse_id_cache().get(url) {
        return Ok(id.clone());
    }
    let resp = post("navigation/resolve_url", json!({ "url": url })).await?;
    let mut endpoints = Vec::new();
    collect_key(&resp, "browseEndpoint", &mut endpoints);
    let id = endpoints
        .first()
        .and_then(|e| e.get("browseId"))
        .and_then(|i| i.as_str())
        .filter(|i| i.starts_with("UC"))
        .context("could not resolve channel URL to browseId")?
        .to_string();
    let mut cache = load_browse_id_cache();
    cache.insert(url.to_string(), id.clone());
    save_browse_id_cache(&cache);
    Ok(id)
}

// ---------------------------------------------------------------------------
// Channel tabs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelTab {
    Videos,
    Shorts,
    Streams,
}

impl ChannelTab {
    fn params(self) -> &'static str {
        match self {
            ChannelTab::Videos => "EgZ2aWRlb3PyBgQKAjoA",
            ChannelTab::Shorts => "EgZzaG9ydHPyBgUKA5oBAA==",
            ChannelTab::Streams => "EgdzdHJlYW1z8gYECgJ6AA==",
        }
    }

    /// Map a channel tab URL suffix to the tab, e.g. ".../videos".
    pub fn from_url(url: &str) -> Option<(String, ChannelTab)> {
        let url = url.trim_end_matches('/');
        for (suffix, tab) in [
            ("/videos", ChannelTab::Videos),
            ("/shorts", ChannelTab::Shorts),
            ("/streams", ChannelTab::Streams),
        ] {
            if let Some(base) = url.strip_suffix(suffix) {
                return Some((base.to_string(), tab));
            }
        }
        None
    }
}

pub async fn channel_tab_videos(
    channel_url: &str,
    tab: ChannelTab,
    limit: usize,
) -> Result<Vec<Video>> {
    let browse_id = resolve_browse_id(channel_url).await?;
    let resp_videos = paginate(
        "browse",
        json!({ "browseId": browse_id, "params": tab.params() }),
        limit,
    )
    .await?;
    if resp_videos.is_empty() {
        anyhow::bail!("innertube channel tab returned no videos");
    }
    // Channel tab renderers omit the byline; fill in the channel URL so
    // "browse channel" actions still work from feed entries.
    let mut videos = resp_videos;
    for v in &mut videos {
        if v.channel_url.is_empty() {
            v.channel_url = channel_url.trim_end_matches('/').to_string();
        }
    }
    Ok(videos)
}

/// Channel display name + avatar URL via channel metadata (one request).
pub async fn channel_meta(channel_url: &str) -> Result<(String, Option<String>)> {
    let browse_id = resolve_browse_id(channel_url).await?;
    let resp = post("browse", json!({ "browseId": browse_id })).await?;
    let meta = resp
        .pointer("/metadata/channelMetadataRenderer")
        .context("no channelMetadataRenderer in browse response")?;
    let name = meta
        .get("title")
        .and_then(|t| t.as_str())
        .context("channel metadata missing title")?
        .to_string();
    let avatar = meta
        .pointer("/avatar/thumbnails")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());
    Ok((name, avatar))
}

// ---------------------------------------------------------------------------
// URL router for the generic fetch_playlist path
// ---------------------------------------------------------------------------

/// What an arbitrary "playlist-ish" URL actually points at.
#[derive(Debug, PartialEq, Eq)]
pub enum UrlKind {
    ChannelTab(String, ChannelTab),
    Playlist(String),
    SearchResults { query: String, sp: String },
    Unsupported,
}

pub fn classify_url(url: &str) -> UrlKind {
    if let Some((base, tab)) = ChannelTab::from_url(url) {
        return UrlKind::ChannelTab(base, tab);
    }
    if let Some(list) = url
        .split("list=")
        .nth(1)
        .map(|r| r.split('&').next().unwrap_or(r).to_string())
    {
        return UrlKind::Playlist(list);
    }
    if url.contains("/results?search_query=") {
        let q_raw = url
            .split("search_query=")
            .nth(1)
            .map(|r| r.split('&').next().unwrap_or(r))
            .unwrap_or_default();
        let sp = url
            .split("&sp=")
            .nth(1)
            .map(|r| r.split('&').next().unwrap_or(r))
            .unwrap_or_default();
        let query = percent_decode_plus(q_raw);
        return UrlKind::SearchResults {
            query,
            sp: sp.to_string(),
        };
    }
    UrlKind::Unsupported
}

/// Reverse of `urlencoding_simple`: '+' → space, %XX → byte.
fn percent_decode_plus(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        match b {
            b'+' => bytes.push(b' '),
            b'%' => {
                let hi = iter.next();
                let lo = iter.next();
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    let hex = [hi, lo];
                    if let Ok(s) = std::str::from_utf8(&hex) {
                        if let Ok(v) = u8::from_str_radix(s, 16) {
                            bytes.push(v);
                            continue;
                        }
                    }
                }
                bytes.push(b'%');
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── parse_relative_time ─────────────────────────────────────────────

    #[test]
    fn relative_time_days() {
        assert_eq!(
            parse_relative_time("3 days ago", 1_000_000),
            Some(1_000_000 - 3 * 86400)
        );
    }

    #[test]
    fn relative_time_streamed_prefix() {
        assert_eq!(
            parse_relative_time("Streamed 2 weeks ago", 2_000_000),
            Some(2_000_000 - 2 * 7 * 86400)
        );
    }

    #[test]
    fn relative_time_singular() {
        assert_eq!(
            parse_relative_time("1 hour ago", 10_000),
            Some(10_000 - 3600)
        );
    }

    #[test]
    fn relative_time_garbage() {
        assert_eq!(parse_relative_time("LIVE", 10_000), None);
        assert_eq!(parse_relative_time("", 10_000), None);
    }

    // ── view counts ─────────────────────────────────────────────────────

    #[test]
    fn view_count_commas() {
        assert_eq!(parse_view_count("1,234,567 views"), Some(1_234_567));
    }

    #[test]
    fn view_count_no_views() {
        assert_eq!(parse_view_count("No views"), None);
    }

    #[test]
    fn short_view_count_millions() {
        assert_eq!(parse_short_view_count("1.2M views"), Some(1_200_000));
    }

    #[test]
    fn short_view_count_plain() {
        assert_eq!(parse_short_view_count("512 views"), Some(512));
    }

    // ── decode_sp ───────────────────────────────────────────────────────

    #[test]
    fn decode_sp_double_encoded() {
        assert_eq!(decode_sp("EgIIAg%253D%253D"), "EgIIAg==");
        assert_eq!(decode_sp("CAI%253D"), "CAI=");
        assert_eq!(decode_sp(""), "");
    }

    // ── text_of ─────────────────────────────────────────────────────────

    #[test]
    fn text_of_simple() {
        assert_eq!(text_of(&json!({"simpleText": "hi"})).as_deref(), Some("hi"));
    }

    #[test]
    fn text_of_runs() {
        let v = json!({"runs": [{"text": "a"}, {"text": "b"}]});
        assert_eq!(text_of(&v).as_deref(), Some("ab"));
    }

    // ── video_from_renderer ─────────────────────────────────────────────

    #[test]
    fn video_renderer_basic() {
        let r = json!({
            "videoId": "abc123",
            "title": {"runs": [{"text": "Test Video"}]},
            "publishedTimeText": {"simpleText": "1 day ago"},
            "lengthText": {"simpleText": "10:00"},
            "viewCountText": {"simpleText": "1,000 views"},
            "ownerText": {"runs": [{"text": "Chan", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCx", "canonicalBaseUrl": "/@chan"}}}]},
            "thumbnail": {"thumbnails": [{"url": "https://i.ytimg.com/vi/abc123/hq720.jpg?sqp=x"}]}
        });
        let v = video_from_renderer(&r, 86400 * 10).unwrap();
        assert_eq!(v.id, "abc123");
        assert_eq!(v.title, "Test Video");
        assert_eq!(v.channel, "Chan");
        assert_eq!(v.channel_url, "https://www.youtube.com/@chan");
        assert_eq!(v.duration_string, "10:00");
        assert_eq!(v.view_count, Some(1000));
        assert_eq!(v.timestamp, Some(86400 * 9));
        assert_eq!(v.thumbnail, "https://i.ytimg.com/vi/abc123/hq720.jpg");
    }

    #[test]
    fn video_renderer_no_id_is_none() {
        assert!(video_from_renderer(&json!({"title": {"simpleText": "x"}}), 0).is_none());
    }

    #[test]
    fn video_renderer_not_short_by_default() {
        let r = json!({"videoId": "v1", "title": {"simpleText": "x"}});
        assert!(!video_from_renderer(&r, 0).unwrap().is_short);
    }

    #[test]
    fn video_renderer_short_via_overlay() {
        let r = json!({
            "videoId": "v1",
            "title": {"simpleText": "x"},
            "thumbnailOverlays": [
                {"thumbnailOverlayTimeStatusRenderer": {"style": "SHORTS"}}
            ]
        });
        assert!(video_from_renderer(&r, 0).unwrap().is_short);
    }

    #[test]
    fn video_renderer_short_via_reel_endpoint() {
        let r = json!({
            "videoId": "v1",
            "title": {"simpleText": "x"},
            "navigationEndpoint": {"reelWatchEndpoint": {"videoId": "v1"}}
        });
        assert!(video_from_renderer(&r, 0).unwrap().is_short);
    }

    #[test]
    fn shorts_lockup_marked_short() {
        let r = json!({
            "onTap": {"innertubeCommand": {"reelWatchEndpoint": {"videoId": "s1"}}},
            "overlayMetadata": {"primaryText": {"content": "A short"}}
        });
        assert!(video_from_shorts_lockup(&r, 0).unwrap().is_short);
    }

    // ── browse_id_from_url ──────────────────────────────────────────────

    #[test]
    fn browse_id_direct() {
        assert_eq!(
            browse_id_from_url("https://www.youtube.com/channel/UCabc_-123/videos").as_deref(),
            Some("UCabc_-123")
        );
    }

    #[test]
    fn browse_id_handle_is_none() {
        assert_eq!(browse_id_from_url("https://www.youtube.com/@handle"), None);
    }

    // ── classify_url ────────────────────────────────────────────────────

    #[test]
    fn classify_channel_tab() {
        assert_eq!(
            classify_url("https://www.youtube.com/@x/videos"),
            UrlKind::ChannelTab("https://www.youtube.com/@x".into(), ChannelTab::Videos)
        );
    }

    #[test]
    fn classify_playlist() {
        assert_eq!(
            classify_url("https://www.youtube.com/playlist?list=PL123&foo=1"),
            UrlKind::Playlist("PL123".into())
        );
    }

    #[test]
    fn classify_search() {
        assert_eq!(
            classify_url(
                "https://www.youtube.com/results?search_query=hello+world&sp=EgIQAw%253D%253D"
            ),
            UrlKind::SearchResults {
                query: "hello world".into(),
                sp: "EgIQAw%253D%253D".into()
            }
        );
    }

    #[test]
    fn classify_unsupported() {
        assert_eq!(
            classify_url("https://www.youtube.com/@x/playlists"),
            UrlKind::Unsupported
        );
    }

    // ── percent_decode_plus ─────────────────────────────────────────────

    #[test]
    fn percent_decode_utf8() {
        assert_eq!(percent_decode_plus("caf%C3%A9+now"), "café now");
    }

    // ── collect_key / find_continuation ─────────────────────────────────

    #[test]
    fn collect_nested() {
        let v = json!({"a": {"videoRenderer": {"videoId": "1"}}, "b": [{"videoRenderer": {"videoId": "2"}}]});
        let mut out = Vec::new();
        collect_key(&v, "videoRenderer", &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn continuation_token_found() {
        let v = json!({"x": {"continuationCommand": {"token": "tok"}}});
        assert_eq!(find_continuation(&v).as_deref(), Some("tok"));
    }

    // ── lockupViewModel (channel tabs) ───────────────────────────────────

    fn lockup(content_type: &str) -> Value {
        json!({
            "contentId": "aB5LGrHISqY",
            "contentType": content_type,
            "contentImage": {"thumbnailViewModel": {
                "image": {"sources": [{"url": "https://i.ytimg.com/vi/aB5LGrHISqY/hq.jpg?sqp=x"}]},
                "overlays": [{"thumbnailBottomOverlayViewModel": {"badges": [
                    {"thumbnailBadgeViewModel": {"text": "7:02"}}
                ]}}]
            }},
            "metadata": {"lockupMetadataViewModel": {
                "title": {"content": "A title"},
                "metadata": {"contentMetadataViewModel": {"metadataRows": [
                    {"metadataParts": [
                        {"text": {"content": "Some Channel", "commandRuns": [{"onTap": {"innertubeCommand":
                            {"browseEndpoint": {"browseId": "UCxyz", "canonicalBaseUrl": "/@some"}}}}]}},
                        {"text": {"content": "672K views"}},
                        {"text": {"content": "1 day ago"}}
                    ]}
                ]}}
            }}
        })
    }

    #[test]
    fn lockup_video_parsed() {
        let v = video_from_lockup(&lockup("LOCKUP_CONTENT_TYPE_VIDEO"), 1_000_000).unwrap();
        assert_eq!(v.id, "aB5LGrHISqY");
        assert_eq!(v.title, "A title");
        assert_eq!(v.channel, "Some Channel");
        assert_eq!(v.channel_url, "https://www.youtube.com/@some");
        assert_eq!(v.duration_string, "7:02");
        assert_eq!(v.view_count, Some(672_000));
        assert_eq!(v.timestamp, Some(1_000_000 - 86400));
        assert_eq!(v.thumbnail, "https://i.ytimg.com/vi/aB5LGrHISqY/hq.jpg");
        assert!(!v.is_short);
    }

    #[test]
    fn lockup_shorts_flagged() {
        let v = video_from_lockup(&lockup("LOCKUP_CONTENT_TYPE_SHORTS"), 1_000_000).unwrap();
        assert!(v.is_short);
    }

    #[test]
    fn lockup_playlist_skipped() {
        assert!(video_from_lockup(&lockup("LOCKUP_CONTENT_TYPE_PLAYLIST"), 1_000_000).is_none());
    }
}
