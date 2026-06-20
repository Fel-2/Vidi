use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::twitch_subs_file;
use crate::models::{TwitchGame, TwitchStream, TwitchVod};

const GQL_URL: &str = "https://gql.twitch.tv/gql";

// ---------------------------------------------------------------------------
// GQL helper
// ---------------------------------------------------------------------------

/// POST a raw GraphQL operation to the Twitch GQL endpoint.
async fn gql(client_id: &str, body: Value) -> Result<Value> {
    let client = reqwest::Client::new();
    let resp: Value = client
        .post(GQL_URL)
        .header("Client-Id", client_id)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Twitch GQL request failed")?
        .json()
        .await
        .context("Failed to parse Twitch GQL response")?;
    if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
        anyhow::bail!("Twitch GQL error: {}", err);
    }
    Ok(resp)
}

/// Build a `TwitchStream` from a node that has `viewersCount`, optional
/// `createdAt`, `game.displayName` and a broadcaster login/title.
fn stream_from_node(login: String, title: String, stream: &Value) -> TwitchStream {
    let is_live = !stream.is_null();
    let viewers = stream
        .get("viewersCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let game = stream
        .pointer("/game/displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("-")
        .to_string();
    let uptime = stream
        .get("createdAt")
        .and_then(|v| v.as_str())
        .map(uptime_from_iso)
        .unwrap_or_default();
    TwitchStream {
        login,
        title,
        game,
        viewers,
        is_live,
        uptime,
    }
}

// ---------------------------------------------------------------------------
// Search (web GQL schema)
// ---------------------------------------------------------------------------

pub async fn search_twitch(query: &str, client_id: &str) -> Result<Vec<TwitchStream>> {
    let payload = serde_json::json!({
        "query": "query Search($q: String!) { searchFor(userQuery: $q, platform: \"web\", target: {index: CHANNEL}) { channels { items { login stream { viewersCount createdAt game { displayName } } broadcastSettings { title } } } } }",
        "variables": { "q": query }
    });

    let resp = gql(client_id, payload).await?;
    let items = resp
        .pointer("/data/searchFor/channels/items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let streams = items
        .iter()
        .map(|item| {
            let login = item
                .get("login")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = item
                .pointer("/broadcastSettings/title")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let stream = item.get("stream").cloned().unwrap_or(Value::Null);
            stream_from_node(login, title, &stream)
        })
        .collect();

    Ok(streams)
}

// ---------------------------------------------------------------------------
// Subscriptions — one batched GQL call (with streamlink fallback)
// ---------------------------------------------------------------------------

/// Live status for every followed channel. Tries a single batched GQL
/// `users(logins:[...])` request first (fast, with viewer counts and uptime);
/// falls back to per-channel `streamlink` if GQL is unavailable.
pub async fn fetch_subscriptions(client_id: &str) -> Result<Vec<TwitchStream>> {
    let subs = load_twitch_subs();
    if subs.is_empty() {
        return Ok(vec![]);
    }

    let mut results = match check_subs_gql(&subs, client_id).await {
        Ok(streams) if !streams.is_empty() => streams,
        _ => streamlink_parallel(subs).await,
    };

    // Sort: LIVE first, then by viewer count desc, then alphabetical.
    results.sort_by(|a, b| {
        b.is_live
            .cmp(&a.is_live)
            .then(b.viewers.cmp(&a.viewers))
            .then(a.login.cmp(&b.login))
    });
    Ok(results)
}

async fn check_subs_gql(logins: &[String], client_id: &str) -> Result<Vec<TwitchStream>> {
    let payload = serde_json::json!({
        "query": "query Subs($l: [String!]) { users(logins: $l) { login stream { viewersCount createdAt game { displayName } } broadcastSettings { title } } }",
        "variables": { "l": logins }
    });

    let resp = gql(client_id, payload).await?;
    let users = resp
        .pointer("/data/users")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let streams = users
        .iter()
        .filter(|u| !u.is_null())
        .map(|u| {
            let login = u
                .get("login")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = u
                .pointer("/broadcastSettings/title")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let stream = u.get("stream").cloned().unwrap_or(Value::Null);
            stream_from_node(login, title, &stream)
        })
        .collect();

    Ok(streams)
}

// ---------------------------------------------------------------------------
// Top streams & categories (the Twitch directory)
// ---------------------------------------------------------------------------

pub async fn fetch_top_streams(client_id: &str, first: u32) -> Result<Vec<TwitchStream>> {
    let payload = serde_json::json!({
        "query": "query Top($n: Int!) { streams(first: $n) { edges { node { viewersCount createdAt game { displayName } broadcaster { login broadcastSettings { title } } } } } }",
        "variables": { "n": first }
    });

    let resp = gql(client_id, payload).await?;
    Ok(streams_from_edges(resp.pointer("/data/streams/edges")))
}

pub async fn fetch_game_streams(
    client_id: &str,
    game: &str,
    first: u32,
) -> Result<Vec<TwitchStream>> {
    let payload = serde_json::json!({
        "query": "query GameStreams($g: String!, $n: Int!) { game(name: $g) { streams(first: $n) { edges { node { viewersCount createdAt game { displayName } broadcaster { login broadcastSettings { title } } } } } } }",
        "variables": { "g": game, "n": first }
    });

    let resp = gql(client_id, payload).await?;
    Ok(streams_from_edges(resp.pointer("/data/game/streams/edges")))
}

/// Parse a `streams { edges { node { ... broadcaster { ... } } } }` array.
fn streams_from_edges(edges: Option<&Value>) -> Vec<TwitchStream> {
    edges
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|edge| {
                    let node = edge.get("node")?;
                    let login = node
                        .pointer("/broadcaster/login")
                        .and_then(|v| v.as_str())?
                        .to_string();
                    let title = node
                        .pointer("/broadcaster/broadcastSettings/title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string();
                    Some(stream_from_node(login, title, node))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub async fn fetch_top_games(client_id: &str, first: u32) -> Result<Vec<TwitchGame>> {
    let payload = serde_json::json!({
        "query": "query Games($n: Int!) { games(first: $n) { edges { node { name displayName viewersCount boxArtURL(width: 144, height: 192) } } } }",
        "variables": { "n": first }
    });

    let resp = gql(client_id, payload).await?;
    let edges = resp
        .pointer("/data/games/edges")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let games = edges
        .iter()
        .filter_map(|edge| {
            let node = edge.get("node")?;
            let name = node
                .get("displayName")
                .or_else(|| node.get("name"))
                .and_then(|v| v.as_str())?
                .to_string();
            let viewers = node
                .get("viewersCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let box_art = node
                .get("boxArtURL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(TwitchGame {
                name,
                viewers,
                box_art,
            })
        })
        .collect();

    Ok(games)
}

// ---------------------------------------------------------------------------
// Check single stream via streamlink (fallback path)
// ---------------------------------------------------------------------------

pub async fn check_streamlink_status(user: &str) -> TwitchStream {
    let output = tokio::process::Command::new("streamlink")
        .args([&format!("twitch.tv/{}", user), "--json"])
        .output()
        .await;

    match output {
        Ok(out) => {
            let json: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
            let is_live = json
                .get("streams")
                .and_then(|s| s.as_object())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            let title = json
                .pointer("/metadata/title")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let game = json
                .pointer("/metadata/category")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            TwitchStream {
                login: user.to_string(),
                title,
                game,
                viewers: 0, // streamlink doesn't provide viewer count
                is_live,
                uptime: String::new(),
            }
        }
        Err(_) => TwitchStream {
            login: user.to_string(),
            is_live: false,
            ..Default::default()
        },
    }
}

async fn streamlink_parallel(subs: Vec<String>) -> Vec<TwitchStream> {
    use tokio::task::JoinSet;
    let mut set: JoinSet<TwitchStream> = JoinSet::new();
    for user in subs {
        set.spawn(async move { check_streamlink_status(&user).await });
    }
    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(stream) = res {
            results.push(stream);
        }
    }
    results
}

// ---------------------------------------------------------------------------
// VODs
// ---------------------------------------------------------------------------

/// Twitch VOD categories, mapping to the GQL `BroadcastType` enum.
pub const VOD_TYPES: &[(&str, &str)] = &[
    ("Past Broadcasts", "ARCHIVE"),
    ("Highlights", "HIGHLIGHT"),
    ("Uploads", "UPLOAD"),
    ("Past Premieres", "PAST_PREMIERE"),
];

/// Fetch a channel's VODs. Tries the fast GQL `videos` query first (titles,
/// durations, view counts, thumbnails), falling back to `yt-dlp` if it fails.
/// `vod_type` is a GQL `BroadcastType` (e.g. `ARCHIVE`, `HIGHLIGHT`, `UPLOAD`).
pub async fn fetch_vods(client_id: &str, user: &str, vod_type: &str) -> Result<Vec<TwitchVod>> {
    match fetch_vods_gql(client_id, user, vod_type).await {
        Ok(vods) if !vods.is_empty() => Ok(vods),
        Ok(_) => Ok(vec![]),
        Err(_) => fetch_vods_ytdlp(user).await,
    }
}

async fn fetch_vods_gql(client_id: &str, user: &str, vod_type: &str) -> Result<Vec<TwitchVod>> {
    let payload = serde_json::json!({
        "query": "query Vods($l: String!, $t: BroadcastType, $n: Int!) { user(login: $l) { videos(first: $n, type: $t, sort: TIME) { edges { node { id title lengthSeconds viewCount publishedAt previewThumbnailURL(width: 320, height: 180) game { displayName } } } } } }",
        "variables": { "l": user, "t": vod_type, "n": 30 }
    });

    let resp = gql(client_id, payload).await?;
    let edges = resp
        .pointer("/data/user/videos/edges")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let vods = edges
        .iter()
        .filter_map(|edge| {
            let node = edge.get("node")?;
            let id = node.get("id").and_then(|v| v.as_str())?.to_string();
            let title = node
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(no title)")
                .to_string();
            let duration = node
                .get("lengthSeconds")
                .and_then(|v| v.as_u64())
                .map(fmt_duration_secs)
                .unwrap_or_default();
            let view_count = node.get("viewCount").and_then(|v| v.as_u64()).unwrap_or(0);
            let upload_date = node
                .get("publishedAt")
                .and_then(|v| v.as_str())
                .map(|s| s.get(0..10).unwrap_or(s).to_string())
                .unwrap_or_default();
            let thumbnail = node
                .get("previewThumbnailURL")
                .and_then(|v| v.as_str())
                .filter(|u| !u.contains("404_processing"))
                .unwrap_or("")
                .to_string();
            let game = node
                .pointer("/game/displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let url = format!("https://www.twitch.tv/videos/{}", id);
            Some(TwitchVod {
                id,
                title,
                duration,
                upload_date,
                thumbnail,
                url,
                view_count,
                game,
            })
        })
        .collect();

    Ok(vods)
}

async fn fetch_vods_ytdlp(user: &str) -> Result<Vec<TwitchVod>> {
    let url = format!(
        "https://www.twitch.tv/{}/videos?filter=archives&sort=time",
        user
    );
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("yt-dlp")
            .args([&url, "--flat-playlist", "--playlist-end", "20", "-J"])
            .output(),
    )
    .await
    .context("yt-dlp timed out fetching VODs")?
    .context("Failed to run yt-dlp for Twitch VODs")?;

    if output.stdout.is_empty() {
        return Ok(vec![]);
    }

    let json: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    let entries = json
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let vods = entries
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = entry
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(no title)")
                .to_string();
            let duration = entry
                .get("duration_string")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let upload_date = entry
                .get("upload_date")
                .and_then(|v| v.as_str())
                .map(format_date)
                .unwrap_or_default();
            let thumbnail = entry
                .get("thumbnail")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let view_count = entry
                .get("view_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let url = format!(
                "https://www.twitch.tv/videos/{}",
                id.trim_start_matches('v')
            );
            TwitchVod {
                id,
                title,
                duration,
                upload_date,
                thumbnail,
                url,
                view_count,
                game: String::new(),
            }
        })
        .collect();

    Ok(vods)
}

/// Seconds → `H:MM:SS` (or `M:SS` under an hour).
fn fmt_duration_secs(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

fn format_date(d: &str) -> String {
    if d.len() == 8 {
        format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
    } else {
        d.to_string()
    }
}

/// Convert an ISO-8601 UTC `createdAt` timestamp (e.g. `2026-06-20T09:15:00Z`)
/// into a compact uptime string relative to now (e.g. `3h 12m`). Empty on parse
/// failure or for timestamps in the future.
fn uptime_from_iso(iso: &str) -> String {
    let Some(started) = iso_to_epoch(iso) else {
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

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff]Z` into a Unix timestamp (UTC seconds).
fn iso_to_epoch(iso: &str) -> Option<i64> {
    let bytes = iso.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let n = |a: usize, b: usize| iso.get(a..b)?.parse::<i64>().ok();
    let year = n(0, 4)?;
    let month = n(5, 7)?;
    let day = n(8, 10)?;
    let hour = n(11, 13)?;
    let min = n(14, 16)?;
    let sec = n(17, 19)?;

    // days-from-civil (Howard Hinnant's algorithm), shifts March-based year.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

// ---------------------------------------------------------------------------
// Subscriptions file
// ---------------------------------------------------------------------------

pub fn load_twitch_subs() -> Vec<String> {
    let path = twitch_subs_file();
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

/// True if `login` is already in the subscriptions file (case-insensitive).
pub fn is_followed(login: &str) -> bool {
    let login = login.to_lowercase();
    load_twitch_subs().iter().any(|s| s.to_lowercase() == login)
}

/// Append `login` to the subscriptions file if not already present.
pub fn follow(login: &str) -> Result<()> {
    if is_followed(login) {
        return Ok(());
    }
    let path = twitch_subs_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(login);
    content.push('\n');
    std::fs::write(&path, content)?;
    Ok(())
}

/// Remove `login` from the subscriptions file (case-insensitive).
pub fn unfollow(login: &str) -> Result<()> {
    let path = twitch_subs_file();
    if !path.exists() {
        return Ok(());
    }
    let target = login.to_lowercase();
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

pub fn twitch_stream_url(login: &str) -> String {
    format!("twitch.tv/{}", login)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_date_pads() {
        assert_eq!(format_date("20260620"), "2026-06-20");
        assert_eq!(format_date("weird"), "weird");
    }

    #[test]
    fn iso_epoch_known_value() {
        // 2026-06-20T09:15:00Z == 1781946900
        assert_eq!(iso_to_epoch("2026-06-20T09:15:00Z"), Some(1781946900));
        // The Unix epoch itself.
        assert_eq!(iso_to_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_epoch("not-a-date"), None);
    }

    #[test]
    fn uptime_future_is_empty() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let future = epoch_to_iso(now + 3600);
        assert_eq!(uptime_from_iso(&future), "");
        assert_eq!(uptime_from_iso("not-a-date"), "");
    }

    #[test]
    fn uptime_past_formats() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert_eq!(uptime_from_iso(&epoch_to_iso(now - 75 * 60)), "1h 15m");
        assert_eq!(uptime_from_iso(&epoch_to_iso(now - 5 * 60)), "5m");
    }

    /// Test-only inverse of `iso_to_epoch` for building timestamps relative to now.
    fn epoch_to_iso(mut secs: i64) -> String {
        let days = secs.div_euclid(86400);
        secs = secs.rem_euclid(86400);
        let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
        // days-from-civil inverse.
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
    }
}
