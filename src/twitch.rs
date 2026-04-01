use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::{twitch_subs_file};
use crate::models::{TwitchStream, TwitchVod};

const CLIENT_ID: &str = "kd1unb4k3ax4ap17e6be367k5likhw";
const USER_AGENT: &str =
    "Dalvik/2.1.0 (Linux; U; Android 9; SM-G960F Build/PPR1.180610.011) tv.twitch.android.app/6.0.0";

// ---------------------------------------------------------------------------
// Twitch GQL search
// ---------------------------------------------------------------------------

pub async fn search_twitch(query: &str) -> Result<Vec<TwitchStream>> {
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "query": "query Search($q: String!) { searchFor(userQuery: $q, platform: \"mobile\", target: {index: CHANNELS}) { channels { items { login stream { viewersCount } broadcastSettings { title game { displayName } } } } } }",
        "variables": { "q": query }
    });

    let resp: Value = client
        .post("https://gql.twitch.tv/gql")
        .header("Client-Id", CLIENT_ID)
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .context("Twitch GQL request failed")?
        .json()
        .await
        .context("Failed to parse Twitch GQL response")?;

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
            let is_live = item.get("stream").map(|v| !v.is_null()).unwrap_or(false);
            let viewers = item
                .pointer("/stream/viewersCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let title = item
                .pointer("/broadcastSettings/title")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            let game = item
                .pointer("/broadcastSettings/game/displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_string();
            TwitchStream {
                login,
                title,
                game,
                viewers,
                is_live,
            }
        })
        .collect();

    Ok(streams)
}

// ---------------------------------------------------------------------------
// Check single stream via streamlink
// ---------------------------------------------------------------------------

pub async fn check_streamlink_status(user: &str) -> TwitchStream {
    let output = tokio::process::Command::new("streamlink")
        .args([&format!("twitch.tv/{}", user), "--json"])
        .output()
        .await;

    match output {
        Ok(out) => {
            let json: Value =
                serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
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
            }
        }
        Err(_) => TwitchStream {
            login: user.to_string(),
            is_live: false,
            ..Default::default()
        },
    }
}

// ---------------------------------------------------------------------------
// Check subscriptions in parallel
// ---------------------------------------------------------------------------

pub async fn check_subs_parallel() -> Result<Vec<TwitchStream>> {
    let subs = load_twitch_subs();
    if subs.is_empty() {
        return Ok(vec![]);
    }

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

    // Sort: LIVE first, then alphabetical
    results.sort_by(|a, b| b.is_live.cmp(&a.is_live).then(a.login.cmp(&b.login)));
    Ok(results)
}

// ---------------------------------------------------------------------------
// VODs via yt-dlp
// ---------------------------------------------------------------------------

pub async fn fetch_vods(user: &str) -> Result<Vec<TwitchVod>> {
    let url = format!(
        "https://www.twitch.tv/{}/videos?filter=archives&sort=time",
        user
    );
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("yt-dlp")
            .args([
                &url,
                "--flat-playlist",
                "--playlist-end",
                "20",
                "-J",
            ])
            .output(),
    )
    .await
    .context("yt-dlp timed out fetching VODs")?
    .context("Failed to run yt-dlp for Twitch VODs")?;

    if output.stdout.is_empty() {
        return Ok(vec![]);
    }

    let json: Value =
        serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
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
            let url = format!("https://www.twitch.tv/videos/{}", id.trim_start_matches('v'));
            TwitchVod {
                id,
                title,
                duration,
                upload_date,
                thumbnail,
                url,
            }
        })
        .collect();

    Ok(vods)
}

fn format_date(d: &str) -> String {
    if d.len() == 8 {
        format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
    } else {
        d.to_string()
    }
}

// ---------------------------------------------------------------------------
// Subscriptions
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

pub fn twitch_stream_url(login: &str) -> String {
    format!("twitch.tv/{}", login)
}
