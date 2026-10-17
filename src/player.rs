use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::stdout;
use std::sync::atomic::{AtomicBool, Ordering};

static TUI_SUSPENDED: AtomicBool = AtomicBool::new(false);

/// Whether the TUI was suspended since the last call, clearing the flag.
/// The restored alternate screen is empty, so the caller must repaint fully
/// instead of letting ratatui diff against the pre-suspend frame.
pub fn take_tui_suspended() -> bool {
    TUI_SUSPENDED.swap(false, Ordering::Relaxed)
}

/// Suspend TUI, run an external process, then restore TUI, returning its exit
/// status so callers can react to a failed run (e.g. retry playback).
pub async fn launch_external_status(args: &[&str]) -> Result<std::process::ExitStatus> {
    if args.is_empty() {
        anyhow::bail!("no command to run");
    }
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    let status = tokio::process::Command::new(args[0])
        .args(&args[1..])
        .status()
        .await;

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    TUI_SUSPENDED.store(true, Ordering::Relaxed);

    status.context("failed to run external process")
}

/// Suspend TUI, run an external process, then restore TUI.
pub async fn launch_external(args: &[&str]) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }
    launch_external_status(args).await.map(|_| ())
}

/// Spawn an external process in the background (no TUI suspend).
pub fn spawn_detached(args: &[&str]) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }
    tokio::process::Command::new(args[0])
        .args(&args[1..])
        .spawn()?;
    Ok(())
}

/// Run an external process in the background and wait for it to finish.
/// Does NOT touch the terminal — safe to call from `tokio::spawn`.
pub async fn run_background(args: &[&str]) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }
    let status = tokio::process::Command::new(args[0])
        .args(&args[1..])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("Process exited with {}", status);
    }
    Ok(())
}

/// Build an mpv `--ytdl-format` value for the requested quality.
/// "best" (or empty) selects the highest available; otherwise `quality` is
/// treated as a max height in pixels (e.g. "1080").
pub fn ytdl_format(quality: &str) -> String {
    let q = quality.trim();
    if q.is_empty() || q.eq_ignore_ascii_case("best") {
        "bestvideo+bestaudio/best".to_string()
    } else {
        format!("bestvideo[height<={q}]+bestaudio/best[height<={q}]/best")
    }
}

/// Maps a streamlink-style Twitch quality ("best", "1080p60", "720p") to a
/// max-height value understood by `ytdl_format`.
pub fn twitch_quality_to_height(quality: &str) -> String {
    let digits: String = quality.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        "best".to_string()
    } else {
        digits
    }
}

/// Default a live quality to "best" when unset.
pub fn live_quality(quality: &str) -> String {
    let q = quality.trim();
    if q.is_empty() {
        "best".to_string()
    } else {
        q.to_string()
    }
}

/// Drops ffmpeg's warnings (HLS keepalive retries, unimplemented H.264 SEI),
/// which bury the terminal mpv shares with vidi. Errors still print.
const QUIET_FFMPEG: &str = "--msg-level=ffmpeg=error";

/// YouTube player clients vidi asks yt-dlp to use, using the upstream
/// `youtube:player_client` extractor argument.
///
/// yt-dlp's own choice of clients is not vidi's to rely on. Upstream changes
/// the defaults without notice, and a bad default breaks playback for every
/// vidi user at once. The `tv_samsung` client added in 2026-10 served
/// DRM-blocked / 403 formats for most videos, and yt-dlp only advances its
/// client chain when *extraction* fails — extraction succeeded, so playback
/// died with nothing to recover it. Pinning the client here keeps vidi's
/// behaviour ours regardless of what upstream decides next.
///
/// `visionos` leads because it is the only client that needs neither a PO
/// token nor a JS runtime; `web` is the backstop (its format 18 is exempt from
/// PO tokens, so it still plays without one).
pub const YTDLP_CLIENTS: &str = "visionos,web";

/// Last-resort clients for the playback retry, when the pinned chain's formats
/// are all refused. Lower quality, but plays when a whole client is gated.
pub const YTDLP_CLIENTS_FALLBACK: &str = "web";

/// First yt-dlp release that knows the `visionos` client (PR #17184, landed
/// 2026-07-09; the first release after it is 2026.08.19), i.e. the oldest build
/// that can honour vidi's pin. Older builds skip clients they don't recognise
/// and silently use their own defaults instead.
pub const MIN_YTDLP_VERSION: (u32, u32, u32) = (2026, 8, 19);

/// Parse a yt-dlp version like `2026.08.19` or `2026.08.19.r34.g8973412` into a
/// comparable tuple. Returns `None` for anything that isn't date-versioned, so
/// an unusual build is never mistaken for an old one.
pub fn parse_ytdlp_version(raw: &str) -> Option<(u32, u32, u32)> {
    let mut parts = raw.trim().split('.');
    let mut fields = [0u32; 3];
    for field in &mut fields {
        *field = parts.next()?.parse().ok()?;
    }
    Some((fields[0], fields[1], fields[2]))
}

/// `--extractor-args youtube:player_client=…` for yt-dlp invocations.
/// Empty for non-YouTube URLs, or when `clients` is blank.
pub fn ytdlp_client_args(clients: &str, url: &str) -> Vec<String> {
    let clients = clients.trim();
    if clients.is_empty() || !is_youtube_url(url) {
        return Vec::new();
    }
    vec![
        "--extractor-args".to_string(),
        format!("youtube:player_client={clients}"),
    ]
}

/// The same pin for mpv, which runs its own yt-dlp through ytdl_hook.
pub fn mpv_client_args(clients: &str, url: &str) -> Vec<String> {
    let clients = clients.trim();
    if clients.is_empty() || !is_youtube_url(url) {
        return Vec::new();
    }
    vec![format!(
        "--ytdl-raw-options-append=extractor-args=youtube:player_client={clients}"
    )]
}

/// Build mpv arguments for watching a video. `clients` is the YouTube player
/// client chain to pin (see [`YTDLP_CLIENTS`]); it is ignored for other sites.
pub fn mpv_watch_args(url: &str, title: &str, quality: &str, clients: &str) -> Vec<String> {
    let mut args = vec!["mpv".to_string(), url.to_string()];
    if !title.is_empty() {
        args.push(format!("--force-media-title={}", title));
        args.push(format!("--script-opts-append=mpris-title={}", title));
    }
    args.push(format!("--ytdl-format={}", ytdl_format(quality)));
    args.extend(mpv_client_args(clients, url));
    args.push(QUIET_FFMPEG.to_string());
    args
}

/// Resolve a stream URL for `url` with `yt-dlp`, without downloading.
pub async fn resolve_stream_url(url: &str, quality: &str) -> Result<String> {
    let selector = ytdl_format(quality);
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        tokio::process::Command::new("yt-dlp")
            .args(["--no-warnings", "--no-playlist", "-f", &selector, "-g", url])
            .output(),
    )
    .await
    .context("yt-dlp timed out resolving the stream")?
    .context("Failed to run yt-dlp")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("yt-dlp could not resolve the stream: {}", err.trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if url.is_empty() {
        anyhow::bail!("yt-dlp returned no stream URL");
    }
    Ok(url.to_string())
}

/// Build mpv arguments to play several URLs back to back (the queue).
pub fn mpv_queue_args(urls: &[String], quality: &str, clients: &str) -> Vec<String> {
    let mut args = vec![
        "mpv".to_string(),
        format!("--ytdl-format={}", ytdl_format(quality)),
        QUIET_FFMPEG.to_string(),
    ];
    if let Some(yt) = urls.iter().find(|u| is_youtube_url(u)) {
        args.extend(mpv_client_args(clients, yt));
    }
    args.extend(urls.iter().cloned());
    args
}

/// Fresh path for mpv's `--log-file`, used to classify playback failures.
pub fn log_path() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    crate::config::youtube_cache_dir().join(format!("mpv-{}-{}.log", std::process::id(), nanos))
}

/// Refusals that will not go away by trying again. Per the README: do not
/// hammer a bot check, and do not retry permanent refusals.
const PERMANENT_FAILURES: &[&str] = &[
    // Matches both the ASCII and the typographic apostrophe yt-dlp uses.
    "Sign in to confirm you",
    "not a bot",
    "Video unavailable",
    "This video is not available",
    "Private video",
    "available in your country",
    "Join this channel",
    "Premieres in",
    "Incomplete YouTube ID",
    "Unsupported URL",
];

/// Symptoms of the shared-media failures worth one more attempt: YouTube's CDN
/// refuses individual URLs transiently, and a gated player client refuses all
/// of them at once. Both need a fresh extraction to recover.
const RETRYABLE_FAILURES: &[&str] = &[
    "Failed to open",
    "Could not open source file",
    "No video or audio streams selected",
    "Failed to recognize file format",
    "HTTP error 403",
    "HTTP error 404",
    "HTTP error 5",
];

/// True when the failure is permanent, so replaying it is pointless.
pub fn failure_is_permanent(log: &str) -> bool {
    PERMANENT_FAILURES.iter().any(|needle| log.contains(needle))
}

/// Whether mpv's log describes a failure worth retrying. Anything not
/// recognised as a transient media failure is left alone.
pub fn should_retry_playback(log: &str) -> bool {
    if log.is_empty() || failure_is_permanent(log) {
        return false;
    }
    RETRYABLE_FAILURES.iter().any(|needle| log.contains(needle))
}

pub fn is_youtube_url(url: &str) -> bool {
    let u = url.to_lowercase();
    u.contains("youtube.com") || u.contains("youtu.be")
}

/// mpv arguments that make yt-dlp mark SponsorBlock segments as chapters
/// (visible and skippable in mpv's OSC). Empty categories → no args.
pub fn mpv_sponsorblock_args(categories: &str, url: &str) -> Vec<String> {
    let cats = categories.trim();
    if cats.is_empty() || !is_youtube_url(url) {
        return vec![];
    }
    vec![format!(
        "--ytdl-raw-options-append=sponsorblock-mark={}",
        cats
    )]
}

/// yt-dlp arguments that cut SponsorBlock segments out of downloads.
/// Empty categories → no args.
pub fn ytdlp_sponsorblock_args(categories: &str, url: &str) -> Vec<String> {
    let cats = categories.trim();
    if cats.is_empty() || !is_youtube_url(url) {
        return vec![];
    }
    vec!["--sponsorblock-remove".to_string(), cats.to_string()]
}

/// Build streamlink arguments.
pub fn streamlink_args(url: &str, quality: &str, player: &str) -> Vec<String> {
    vec![
        "streamlink".to_string(),
        "--player".to_string(),
        player.to_string(),
        url.to_string(),
        quality.to_string(),
    ]
}

/// Build yt-dlp download arguments for video.
pub fn ytdlp_download_args(
    url: &str,
    download_dir: &std::path::Path,
    clients: &str,
) -> Vec<String> {
    let output_template = download_dir
        .join("videos")
        .join("individual")
        .join("%(channel)s")
        .join("%(title)s.%(ext)s");
    let mut args = vec![
        "yt-dlp".to_string(),
        url.to_string(),
        "--output".to_string(),
        output_template.to_string_lossy().to_string(),
    ];
    args.extend(ytdlp_client_args(clients, url));
    args
}

/// Build yt-dlp download arguments for audio only.
pub fn ytdlp_download_audio_args(
    url: &str,
    download_dir: &std::path::Path,
    clients: &str,
) -> Vec<String> {
    let output_template = download_dir
        .join("audio")
        .join("individual")
        .join("%(channel)s")
        .join("%(title)s.%(ext)s");
    let mut args = vec![
        "yt-dlp".to_string(),
        url.to_string(),
        "-x".to_string(),
        "-f".to_string(),
        "bestaudio".to_string(),
        "--audio-format".to_string(),
        "mp3".to_string(),
        "--output".to_string(),
        output_template.to_string_lossy().to_string(),
    ];
    args.extend(ytdlp_client_args(clients, url));
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn mpv_args_uses_script_opts_append() {
        let args = mpv_watch_args("https://youtu.be/abc", "Test Title", "1080", YTDLP_CLIENTS);
        // Must use --script-opts-append, NOT --script-opts (comma-in-title bug)
        let opts_arg = args.iter().find(|a| a.contains("script-opts")).unwrap();
        assert!(opts_arg.starts_with("--script-opts-append="));
    }

    #[test]
    fn mpv_args_title_with_comma() {
        let args = mpv_watch_args(
            "https://youtu.be/abc",
            "Monopoly, but SERIOUS.",
            "720",
            YTDLP_CLIENTS,
        );
        let opts_arg = args.iter().find(|a| a.contains("mpris-title")).unwrap();
        assert_eq!(
            opts_arg,
            "--script-opts-append=mpris-title=Monopoly, but SERIOUS."
        );
    }

    #[test]
    fn mpv_args_title_with_equals() {
        let args = mpv_watch_args("https://youtu.be/abc", "a=b", "1080", YTDLP_CLIENTS);
        let title_arg = args
            .iter()
            .find(|a| a.contains("force-media-title"))
            .unwrap();
        assert_eq!(title_arg, "--force-media-title=a=b");
    }

    #[test]
    fn mpv_args_quality_format() {
        let args = mpv_watch_args("https://youtu.be/abc", "T", "720", YTDLP_CLIENTS);
        let fmt_arg = args.iter().find(|a| a.contains("ytdl-format")).unwrap();
        assert!(fmt_arg.contains("height<=720"));
        assert!(!fmt_arg.contains("height<=1080"));
    }

    #[test]
    fn mpv_args_contains_url() {
        let args = mpv_watch_args("https://youtu.be/abc", "T", "1080", YTDLP_CLIENTS);
        assert_eq!(args[0], "mpv");
        assert_eq!(args[1], "https://youtu.be/abc");
    }

    #[test]
    fn mpv_args_pin_youtube_clients_only() {
        let yt = mpv_watch_args("https://youtu.be/abc", "T", "1080", YTDLP_CLIENTS);
        let pinned = yt
            .iter()
            .find(|a| a.contains("player_client"))
            .expect("youtube playback must pin the client");
        // Append, so a user's own raw options are not clobbered.
        assert!(pinned.starts_with("--ytdl-raw-options-append="));
        assert!(pinned.ends_with(&format!("player_client={YTDLP_CLIENTS}")));

        // Other platforms must not be sent a YouTube extractor argument.
        let twitch = mpv_watch_args("https://twitch.tv/x", "T", "1080", YTDLP_CLIENTS);
        assert!(!twitch.iter().any(|a| a.contains("player_client")));
    }

    #[test]
    fn mpv_args_blank_clients_pin_nothing() {
        let args = mpv_watch_args("https://youtu.be/abc", "T", "1080", "  ");
        assert!(!args.iter().any(|a| a.contains("player_client")));
    }

    #[test]
    fn queue_pins_clients_when_any_entry_is_youtube() {
        let urls = vec![
            "https://twitch.tv/x".to_string(),
            "https://youtu.be/abc".to_string(),
        ];
        let args = mpv_queue_args(&urls, "1080", YTDLP_CLIENTS);
        assert!(args.iter().any(|a| a.contains("player_client=")));
        assert_eq!(
            args.iter().filter(|a| a.contains("player_client")).count(),
            1
        );

        let all_twitch = vec!["https://twitch.tv/x".to_string()];
        let args = mpv_queue_args(&all_twitch, "1080", YTDLP_CLIENTS);
        assert!(!args.iter().any(|a| a.contains("player_client")));
    }

    #[test]
    fn bot_check_is_not_retried() {
        // Typographic apostrophe, as yt-dlp actually emits it.
        let log = "[ytdl_hook] ERROR: [youtube] x: Sign in to confirm you\u{2019}re \
                   not a bot. Use --cookies-from-browser\nFailed to open https://rr1\
                   ---sn-x.googlevideo.com/videoplayback?itag=251\nNo video or audio \
                   streams selected.";
        assert!(failure_is_permanent(log));
        assert!(!should_retry_playback(log));
    }

    #[test]
    fn permanent_refusals_are_not_retried() {
        for log in [
            "[youtube] Video unavailable",
            "[youtube] This video is not available",
            "[youtube] Private video. Sign in if you've been granted access",
            "[youtube] The uploader has not made this video available in your country",
            "[youtube] Join this channel to get access to members-only content",
        ] {
            assert!(failure_is_permanent(log), "{log}");
            assert!(!should_retry_playback(log), "{log}");
        }
    }

    #[test]
    fn transient_media_failures_are_retried() {
        for log in [
            "[ffmpeg] https: HTTP error 403 Forbidden\nFailed to open https://rr1\
             ---sn-x.googlevideo.com/videoplayback?itag=251\nNo video or audio streams selected.",
            "EDL: Could not open source file 'https://rr2---sn-x.googlevideo.com/videoplayback'",
            "[ffmpeg] https: HTTP error 503 Service Unavailable",
        ] {
            assert!(should_retry_playback(log), "{log}");
        }
    }

    #[test]
    fn unrecognised_or_empty_failures_are_left_alone() {
        // Nothing to go on, and anything we cannot name is not our business to
        // replay — a retry loop is worse than a single clear failure.
        assert!(!should_retry_playback(""));
        assert!(!should_retry_playback("mpv: some unrelated complaint"));
    }

    #[test]
    fn parses_dated_ytdlp_versions() {
        assert_eq!(parse_ytdlp_version("2026.08.19"), Some((2026, 8, 19)));
        // The version vidi last saw on an Arch `yt-dlp-git` build.
        assert_eq!(
            parse_ytdlp_version("2026.08.19.r34.g8973412\n"),
            Some((2026, 8, 19))
        );
        assert_eq!(
            parse_ytdlp_version("2026.10.10.123456"),
            Some((2026, 10, 10))
        );
    }

    #[test]
    fn unversioned_builds_are_not_treated_as_old() {
        // An odd version string must not produce a spurious upgrade warning.
        assert_eq!(parse_ytdlp_version(""), None);
        assert_eq!(parse_ytdlp_version("nightly"), None);
        assert_eq!(parse_ytdlp_version("2026"), None);
        assert_eq!(parse_ytdlp_version("v2026.08"), None);
    }

    #[test]
    fn version_floor_matches_the_client_pin() {
        // The floor exists only so the pinned clients are understood.
        assert_eq!(parse_ytdlp_version("2026.07.04"), Some((2026, 7, 4)));
        assert!(parse_ytdlp_version("2026.07.04").unwrap() < MIN_YTDLP_VERSION);
        assert!(parse_ytdlp_version("2026.08.19").unwrap() >= MIN_YTDLP_VERSION);
    }

    #[test]
    fn twitch_quality_maps_to_height() {
        assert_eq!(twitch_quality_to_height("best"), "best");
        assert_eq!(twitch_quality_to_height("1080p60"), "1080");
        assert_eq!(twitch_quality_to_height("720p"), "720");
        assert_eq!(twitch_quality_to_height("audio_only"), "best");
    }

    #[test]
    fn live_quality_passes_names_through() {
        assert_eq!(live_quality("best"), "best");
        assert_eq!(live_quality("1080p60"), "1080p60");
        assert_eq!(live_quality("  "), "best");
        assert_eq!(live_quality(""), "best");
    }

    #[test]
    fn streamlink_args_order() {
        let args = streamlink_args("twitch.tv/user", "best", "mpv");
        assert_eq!(
            args,
            vec!["streamlink", "--player", "mpv", "twitch.tv/user", "best"]
        );
    }

    #[test]
    fn ytdlp_download_args_output_template() {
        let args = ytdlp_download_args("https://youtu.be/x", Path::new("/tmp/dl"), YTDLP_CLIENTS);
        assert_eq!(args[0], "yt-dlp");
        assert_eq!(args[1], "https://youtu.be/x");
        assert_eq!(args[2], "--output");
        assert!(args[3].contains("videos/individual"));
        assert!(args[3].contains("%(channel)s"));
        assert!(args.contains(&format!("youtube:player_client={YTDLP_CLIENTS}")));
    }

    #[test]
    fn download_args_pin_youtube_clients_only() {
        let twitch = ytdlp_download_args("https://twitch.tv/videos/1", Path::new("/tmp/dl"), "");
        assert!(!twitch.iter().any(|a| a.contains("player_client")));
    }

    #[test]
    fn ytdlp_audio_args_has_extract_audio() {
        let args =
            ytdlp_download_audio_args("https://youtu.be/x", Path::new("/tmp/dl"), YTDLP_CLIENTS);
        assert!(args.contains(&"-x".to_string()));
        assert!(args.contains(&"bestaudio".to_string()));
        assert!(args.contains(&"mp3".to_string()));
        // The client pin is appended, so find the template by its content.
        let output_arg = args
            .iter()
            .find(|a| a.contains("audio/individual"))
            .expect("output template");
        assert!(output_arg.contains("%(title)s"));
    }
}
