mod app;
mod chat;
mod config;
mod events;
mod innertube;
mod models;
mod player;
mod preview;
mod subs_import;
mod twitch;
mod ui;
mod youtube;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;

use app::App;

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Ensure config dirs & defaults exist
    config::write_default_youtube_config().ok();
    config::write_default_twitch_config().ok();

    let cfg = config::load_config().unwrap_or_default();

    // Warn about missing external tools before grabbing the terminal.
    check_dependencies(&cfg);

    // Restore the terminal before printing a panic, otherwise the shell is
    // left in raw mode with no echo and the panic message is unreadable.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture);
        default_panic(info);
    }));

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let mut app = App::new(cfg);

    // Pre-load saved video IDs
    let saved = youtube::load_saved();
    for v in &saved.entries {
        app.saved_ids.insert(v.id.clone());
    }

    // Pre-load watched video IDs
    let recent = youtube::load_recent();
    for v in &recent.entries {
        app.watched_ids.insert(v.id.clone());
    }

    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Dependency check
// ─────────────────────────────────────────────────────────────────────────────

/// Return true if `bin` is found on PATH.
fn binary_exists(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
}

/// Warn (to stderr) about missing external tools. Does not abort: the user may
/// only use a subset of features, or have a custom player configured.
fn check_dependencies(cfg: &config::Config) {
    // (binary, why it's needed)
    let mut checks: Vec<(&str, &str)> = vec![
        ("yt-dlp", "YouTube/Twitch metadata and playback"),
        ("streamlink", "Twitch live stream status and playback"),
    ];
    // Honour the configured player binaries instead of assuming mpv.
    let player = cfg
        .youtube
        .player
        .split_whitespace()
        .next()
        .unwrap_or("mpv");
    checks.push((player, "video playback (PLAYER in vidi.conf)"));
    let twitch_player = cfg.twitch.player.split_whitespace().next().unwrap_or("mpv");
    if twitch_player != player {
        checks.push((twitch_player, "Twitch playback (PLAYER in twitch.conf)"));
    }

    let missing: Vec<(&str, &str)> = checks
        .into_iter()
        .filter(|(bin, _)| !binary_exists(bin))
        .collect();

    if missing.is_empty() {
        return;
    }

    eprintln!("vidi: the following required tools were not found on your PATH:");
    for (bin, why) in &missing {
        eprintln!("  - {:<12} (needed for {})", bin, why);
    }
    eprintln!("Some features will not work until they are installed.");
    eprintln!("Starting in 2s… (Ctrl-C to abort)");
    std::thread::sleep(Duration::from_secs(2));
}

// ─────────────────────────────────────────────────────────────────────────────
// Main event loop
// ─────────────────────────────────────────────────────────────────────────────

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        // Time the draw so we can detect if it was unexpectedly slow.
        let draw_start = std::time::Instant::now();
        terminal.draw(|f| ui::render(f, app))?;
        let draw_slow = draw_start.elapsed() > Duration::from_millis(150);
        preview::kitty_update_display(app); // overlay kitty image AFTER ratatui draws

        // Drain async events first (non-blocking).
        // Track whether loading just cleared so we can discard buffered keypresses.
        let was_loading = app.loading.is_some();
        while let Ok(ev) = app.rx.try_recv() {
            events::handle_app_event(app, ev);
        }
        let loading_just_cleared = was_loading && app.loading.is_none();

        if app.should_quit {
            break;
        }

        // Discard any keyboard input buffered while the UI was busy (loading overlay
        // was shown, or the draw call itself was slow due to heavy rendering).
        if loading_just_cleared || draw_slow {
            while event::poll(Duration::ZERO)? {
                let _ = event::read();
            }
        } else if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => events::handle_key(app, key).await,
                Event::Mouse(m) => events::handle_mouse(app, m).await,
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
