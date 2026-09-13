use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::{kick_subs_file, KickConfig};
use crate::models::{KickCategory, KickStream, KickVod};

const API_BASE: &str = "https://kick.com/api";
const WEB_BASE: &str = "https://kick.com";

async fn get_json(cfg: &KickConfig, url: &str) -> Result<Value> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", cfg.user_agent.clone())
        .send()
        .await
        .with_context(|| format!("Kick request failed: {}", url))?;
    if !resp.status().is_success() {
        anyhow::bail!("Kick returned {} for {}", resp.status(), url);
    }
    let text = resp.text().await.context("Failed to read Kick response")?;
    let json: Value =
        serde_json::from_str(&text).with_context(|| format!("Invalid Kick JSON from {}", url))?;
    if json.get("message").is_some() && json.as_object().map(|o| o.len()).unwrap_or(0) <= 1 {
        anyhow::bail!("Kick returned an empty response for {}", url);
    }
    Ok(json)
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn str_at(v: &Value, path: &str) -> String {
    v.pointer(path)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn u64_at(v: &Value, path: &str) -> u64 {
    v.pointer(path).and_then(|x| x.as_u64()).unwrap_or(0)
}

/// Build a `KickStream` from one entry of the live directory.
fn stream_from_entry(entry: &Value) -> KickStream {
    let slug = str_at(entry, "/channel/slug");
    let title = str_at(entry, "/session_title");
    let category = entry
        .get("categories")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .map(|c| str_at(c, "/name"))
        .unwrap_or_default();
    let uptime = entry
        .get("start_time")
        .and_then(|v| v.as_str())
        .map(uptime_from_datetime)
        .unwrap_or_default();
    KickStream {
        slug,
        title,
        category,
        viewers: u64_at(entry, "/viewers"),
        is_live: entry
            .get("is_live")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        uptime,
        thumbnail: str_at(entry, "/thumbnail/src"),
        avatar: str_at(entry, "/channel/user/profilepic"),
    }
}

/// Live directory page, optionally filtered to one category slug.
pub async fn fetch_streams(
    cfg: &KickConfig,
    limit: u32,
    category: Option<&str>,
) -> Result<Vec<KickStream>> {
    let mut url = format!("{}/stream/livestreams/en?page=1&limit={}", WEB_BASE, limit);
    if let Some(slug) = category {
        url.push_str(&format!("&subcategory={}", slug));
    }
    let json = get_json(cfg, &url).await?;
    let data = json
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(data.iter().map(stream_from_entry).collect())
}

pub async fn fetch_top_streams(cfg: &KickConfig, limit: u32) -> Result<Vec<KickStream>> {
    fetch_streams(cfg, limit, None).await
}

pub async fn fetch_category_streams(
    cfg: &KickConfig,
    slug: &str,
    limit: u32,
) -> Result<Vec<KickStream>> {
    fetch_streams(cfg, limit, Some(slug)).await
}

/// Top-level Kick categories (Games, IRL, Music, …).
pub async fn fetch_categories(cfg: &KickConfig) -> Result<Vec<KickCategory>> {
    let json = get_json(cfg, &format!("{}/v1/categories", API_BASE)).await?;
    let arr = json.as_array().cloned().unwrap_or_default();
    Ok(arr
        .iter()
        .map(|c| KickCategory {
            name: str_at(c, "/name"),
            slug: str_at(c, "/slug"),
            icon: str_at(c, "/icon"),
        })
        .collect())
}

/// Live status for a single channel.
pub async fn check_channel(cfg: &KickConfig, slug: &str) -> KickStream {
    let url = format!("{}/v2/channels/{}", API_BASE, slug);
    let Ok(json) = get_json(cfg, &url).await else {
        return KickStream {
            slug: slug.to_string(),
            ..Default::default()
        };
    };
    let live = json.get("livestream").filter(|l| !l.is_null());
    let Some(live) = live else {
        return KickStream {
            slug: slug.to_string(),
            ..Default::default()
        };
    };
    KickStream {
        slug: slug.to_string(),
        title: str_at(live, "/session_title"),
        category: live
            .get("categories")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .map(|c| str_at(c, "/name"))
            .unwrap_or_default(),
        viewers: u64_at(live, "/viewers"),
        is_live: true,
        uptime: live
            .get("start_time")
            .and_then(|v| v.as_str())
            .map(uptime_from_datetime)
            .unwrap_or_default(),
        thumbnail: str_at(live, "/thumbnail/src"),
        avatar: str_at(&json, "/user/profilepic"),
    }
}

/// Live status for every followed channel, sorted live-first then by viewers.
pub async fn fetch_subscriptions(cfg: &KickConfig) -> Result<Vec<KickStream>> {
    use tokio::task::JoinSet;

    let subs = load_kick_subs();
    if subs.is_empty() {
        return Ok(vec![]);
    }
    let mut set: JoinSet<KickStream> = JoinSet::new();
    for slug in subs {
        let cfg = cfg.clone();
        set.spawn(async move { check_channel(&cfg, &slug).await });
    }
    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(stream) = res {
            results.push(stream);
        }
    }
    results.sort_by(|a, b| {
        b.is_live
            .cmp(&a.is_live)
            .then(b.viewers.cmp(&a.viewers))
            .then(a.slug.cmp(&b.slug))
    });
    Ok(results)
}

/// Search the live directory for channels whose slug contains `query`.
pub async fn search(cfg: &KickConfig, query: &str) -> Result<Vec<KickStream>> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(vec![]);
    }
    let mut seen = std::collections::HashSet::new();
    let mut matches: Vec<KickStream> = Vec::new();
    for page in 1..=4u32 {
        let url = format!("{}/stream/livestreams/en?page={}&limit=100", WEB_BASE, page);
        let Ok(json) = get_json(cfg, &url).await else {
            continue;
        };
        let data = json
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if data.is_empty() {
            break;
        }
        for entry in data.iter() {
            let stream = stream_from_entry(entry);
            if stream.slug.to_lowercase().contains(&needle) && seen.insert(stream.slug.clone()) {
                matches.push(stream);
            }
        }
    }
    matches.sort_by(|a, b| b.viewers.cmp(&a.viewers).then(a.slug.cmp(&b.slug)));
    Ok(matches)
}

// ---------------------------------------------------------------------------
// VODs
// ---------------------------------------------------------------------------

/// A channel's recent VODs. Entries still airing are marked live.
pub async fn fetch_vods(cfg: &KickConfig, slug: &str) -> Result<Vec<KickVod>> {
    let url = format!("{}/v2/channels/{}/videos", API_BASE, slug);
    let json = get_json(cfg, &url).await?;
    let arr = json.as_array().cloned().unwrap_or_default();

    let vods = arr
        .iter()
        .map(|v| {
            let id = v
                .get("id")
                .map(|i| i.to_string().trim_matches('"').to_string())
                .unwrap_or_default();
            let live = v.get("is_live").and_then(|b| b.as_bool()).unwrap_or(false);
            // Kick reports VOD duration in milliseconds.
            let secs = u64_at(v, "/duration") / 1000;
            KickVod {
                url: format!("{}/{}?video={}", WEB_BASE, slug, id),
                title: str_at(v, "/session_title"),
                duration: if live {
                    "LIVE".to_string()
                } else {
                    fmt_duration_secs(secs)
                },
                upload_date: str_at(v, "/created_at")
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string(),
                thumbnail: str_at(v, "/thumbnail/src"),
                view_count: u64_at(v, "/viewer_count").max(u64_at(v, "/views")),
                id,
                game: v
                    .get("categories")
                    .and_then(|c| c.as_array())
                    .and_then(|c| c.first())
                    .map(|c| str_at(c, "/name"))
                    .unwrap_or_default(),
            }
        })
        .collect();

    Ok(vods)
}

fn fmt_duration_secs(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// `YYYY-MM-DD HH:MM:SS` (UTC) local to the API → seconds since the epoch.
fn datetime_to_epoch(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    let time = rest.trim_end_matches('Z');
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t
        .next()
        .unwrap_or("0")
        .split(['.', '+'])
        .next()
        .unwrap_or("0")
        .parse()
        .ok()?;

    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    Some(days * 86_400 + hour * 3600 + min * 60 + sec)
}

/// Compact uptime ("3h 12m") relative to now; empty on parse failure.
fn uptime_from_datetime(s: &str) -> String {
    let Some(started) = datetime_to_epoch(s) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let secs = now - started;
    if secs < 0 {
        return String::new();
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m", m)
    }
}

// ---------------------------------------------------------------------------
// Subscriptions file
// ---------------------------------------------------------------------------

pub fn load_kick_subs() -> Vec<String> {
    let path = kick_subs_file();
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

/// True if `slug` is already in the subscriptions file (case-insensitive).
pub fn is_followed(slug: &str) -> bool {
    let slug = slug.to_lowercase();
    load_kick_subs().iter().any(|s| s.to_lowercase() == slug)
}

pub fn follow(slug: &str) -> Result<()> {
    if is_followed(slug) {
        return Ok(());
    }
    let path = kick_subs_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(slug);
    content.push('\n');
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn unfollow(slug: &str) -> Result<()> {
    let path = kick_subs_file();
    if !path.exists() {
        return Ok(());
    }
    let target = slug.to_lowercase();
    let kept: Vec<String> = std::fs::read_to_string(&path)?
        .lines()
        .filter(|l| l.trim().to_lowercase() != target)
        .map(|l| l.to_string())
        .collect();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(&path, out)?;
    Ok(())
}

pub fn kick_stream_url(slug: &str) -> String {
    format!("kick.com/{}", slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_formats() {
        assert_eq!(fmt_duration_secs(2042), "34:02");
        assert_eq!(fmt_duration_secs(11_586), "3:13:06");
        assert_eq!(fmt_duration_secs(0), "0:00");
    }

    #[test]
    fn datetime_parses_space_and_t_separators() {
        assert_eq!(
            datetime_to_epoch("2026-06-20T09:15:00Z"),
            Some(1_781_946_900)
        );
        assert_eq!(
            datetime_to_epoch("2026-06-20 09:15:00"),
            Some(1_781_946_900)
        );
        assert_eq!(datetime_to_epoch("not-a-date"), None);
    }

    #[test]
    fn uptime_is_empty_for_future_and_garbage() {
        assert_eq!(uptime_from_datetime("not-a-date"), "");
    }

    #[test]
    fn millisecond_durations_are_scaled() {
        // The API reports 2042000 ms for a 34m 02s broadcast.
        assert_eq!(fmt_duration_secs(2_042_000 / 1000), "34:02");
    }

    #[test]
    fn stream_entry_parses() {
        let entry = serde_json::json!({
            "session_title": "Hello",
            "viewers": 42,
            "is_live": true,
            "start_time": "2026-06-20 09:15:00",
            "thumbnail": { "src": "https://img/thumb.webp" },
            "categories": [{ "name": "Just Chatting", "slug": "just-chatting" }],
            "channel": {
                "slug": "someone",
                "user": { "profilepic": "https://img/avatar.webp" }
            }
        });
        let s = stream_from_entry(&entry);
        assert_eq!(s.slug, "someone");
        assert_eq!(s.title, "Hello");
        assert_eq!(s.category, "Just Chatting");
        assert_eq!(s.viewers, 42);
        assert!(s.is_live);
        assert_eq!(s.avatar, "https://img/avatar.webp");
    }

    #[test]
    fn stream_entry_handles_missing_categories() {
        let entry = serde_json::json!({
            "session_title": "Hi",
            "channel": { "slug": "x" }
        });
        let s = stream_from_entry(&entry);
        assert_eq!(s.slug, "x");
        assert_eq!(s.category, "");
        assert!(s.is_live);
    }
}
