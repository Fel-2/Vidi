use anyhow::Result;
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

/// Suspend TUI, run an external process, then restore TUI.
pub async fn launch_external(args: &[&str]) -> Result<()> {
    if args.is_empty() {
        return Ok(());
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

    status?;
    Ok(())
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

/// Drops ffmpeg's warnings (HLS keepalive retries, unimplemented H.264 SEI),
/// which bury the terminal mpv shares with vidi. Errors still print.
const QUIET_FFMPEG: &str = "--msg-level=ffmpeg=error";

/// Build mpv arguments for watching a video.
pub fn mpv_watch_args(url: &str, title: &str, quality: &str) -> Vec<String> {
    vec![
        "mpv".to_string(),
        url.to_string(),
        format!("--force-media-title={}", title),
        format!("--script-opts-append=mpris-title={}", title),
        format!("--ytdl-format={}", ytdl_format(quality)),
        QUIET_FFMPEG.to_string(),
    ]
}

/// Build mpv arguments to play several URLs back to back (the queue).
pub fn mpv_queue_args(urls: &[String], quality: &str) -> Vec<String> {
    let mut args = vec![
        "mpv".to_string(),
        format!("--ytdl-format={}", ytdl_format(quality)),
        QUIET_FFMPEG.to_string(),
    ];
    args.extend(urls.iter().cloned());
    args
}

/// mpv arguments that make yt-dlp mark SponsorBlock segments as chapters
/// (visible and skippable in mpv's OSC). Empty categories → no args.
pub fn mpv_sponsorblock_args(categories: &str) -> Vec<String> {
    let cats = categories.trim();
    if cats.is_empty() {
        return vec![];
    }
    vec![format!(
        "--ytdl-raw-options-append=sponsorblock-mark={}",
        cats
    )]
}

/// yt-dlp arguments that cut SponsorBlock segments out of downloads.
/// Empty categories → no args.
pub fn ytdlp_sponsorblock_args(categories: &str) -> Vec<String> {
    let cats = categories.trim();
    if cats.is_empty() {
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
pub fn ytdlp_download_args(url: &str, download_dir: &std::path::Path) -> Vec<String> {
    let output_template = download_dir
        .join("videos")
        .join("individual")
        .join("%(channel)s")
        .join("%(title)s.%(ext)s");
    vec![
        "yt-dlp".to_string(),
        url.to_string(),
        "--output".to_string(),
        output_template.to_string_lossy().to_string(),
    ]
}

/// Build yt-dlp download arguments for audio only.
pub fn ytdlp_download_audio_args(url: &str, download_dir: &std::path::Path) -> Vec<String> {
    let output_template = download_dir
        .join("audio")
        .join("individual")
        .join("%(channel)s")
        .join("%(title)s.%(ext)s");
    vec![
        "yt-dlp".to_string(),
        url.to_string(),
        "-x".to_string(),
        "-f".to_string(),
        "bestaudio".to_string(),
        "--audio-format".to_string(),
        "mp3".to_string(),
        "--output".to_string(),
        output_template.to_string_lossy().to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn mpv_args_uses_script_opts_append() {
        let args = mpv_watch_args("https://youtu.be/abc", "Test Title", "1080");
        // Must use --script-opts-append, NOT --script-opts (comma-in-title bug)
        let opts_arg = args.iter().find(|a| a.contains("script-opts")).unwrap();
        assert!(opts_arg.starts_with("--script-opts-append="));
    }

    #[test]
    fn mpv_args_title_with_comma() {
        let args = mpv_watch_args("https://youtu.be/abc", "Monopoly, but SERIOUS.", "720");
        let opts_arg = args.iter().find(|a| a.contains("mpris-title")).unwrap();
        assert_eq!(
            opts_arg,
            "--script-opts-append=mpris-title=Monopoly, but SERIOUS."
        );
    }

    #[test]
    fn mpv_args_title_with_equals() {
        let args = mpv_watch_args("https://youtu.be/abc", "a=b", "1080");
        let title_arg = args
            .iter()
            .find(|a| a.contains("force-media-title"))
            .unwrap();
        assert_eq!(title_arg, "--force-media-title=a=b");
    }

    #[test]
    fn mpv_args_quality_format() {
        let args = mpv_watch_args("https://youtu.be/abc", "T", "720");
        let fmt_arg = args.iter().find(|a| a.contains("ytdl-format")).unwrap();
        assert!(fmt_arg.contains("height<=720"));
        assert!(!fmt_arg.contains("height<=1080"));
    }

    #[test]
    fn mpv_args_contains_url() {
        let args = mpv_watch_args("https://youtu.be/abc", "T", "1080");
        assert_eq!(args[0], "mpv");
        assert_eq!(args[1], "https://youtu.be/abc");
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
        let args = ytdlp_download_args("https://youtu.be/x", Path::new("/tmp/dl"));
        assert_eq!(args[0], "yt-dlp");
        assert_eq!(args[1], "https://youtu.be/x");
        assert_eq!(args[2], "--output");
        assert!(args[3].contains("videos/individual"));
        assert!(args[3].contains("%(channel)s"));
    }

    #[test]
    fn ytdlp_audio_args_has_extract_audio() {
        let args = ytdlp_download_audio_args("https://youtu.be/x", Path::new("/tmp/dl"));
        assert!(args.contains(&"-x".to_string()));
        assert!(args.contains(&"bestaudio".to_string()));
        assert!(args.contains(&"mp3".to_string()));
        let output_arg = args.last().unwrap();
        assert!(output_arg.contains("audio/individual"));
    }
}
