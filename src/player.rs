use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::stdout;

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

/// Build mpv arguments for watching a video.
pub fn mpv_watch_args(url: &str, title: &str, quality: &str) -> Vec<String> {
    vec![
        "mpv".to_string(),
        url.to_string(),
        format!("--force-media-title={}", title),
        format!("--script-opts-append=mpris-title={}", title),
        format!("--ytdl-format=bestvideo[height<={}]+bestaudio/best[height<={}]/best", quality, quality),
    ]
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
