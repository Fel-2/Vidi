# vidi

A terminal UI for YouTube and Twitch, written in Rust.

```
██╗░░██╗██╗██████╗░██╗
██║░░██║██║██╔══██╗██║
╚██╗██╔╝██║██║░░██║██║
░████╔╝░██║██║░░██║██║
░╚██╔╝░░██║██████╔╝██║
░░╚═╝░░░╚═╝╚═════╝░╚═╝
```

## Features

**YouTube**
- 🔥 Trending videos
- 🔍 Search with history
- 📡 Subscription feed (parallel fetch, sorted by upload time)
- 📋 Browse subscribed channels (Videos / Shorts / Streams / Playlists)
- ➕ Subscribe to channels directly from Explore Channels
- 🎯 Custom playlists
- 🕐 Recently watched
- 🔎 Channel search via Miscellaneous → Explore Channels

**Twitch**
- 🔍 Live stream search
- 💜 Subscription status (live/offline)
- 🎬 VOD browsing
- 💬 Live chat viewer

**General**
- Kitty terminal graphics protocol for thumbnail previews
- Inline video playback via `mpv` / `yt-dlp`
- Download support (video + audio-only, single + batch)
- Save/unsave videos to a local watchlist
- Filter bar on all list screens

## Dependencies

- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp)
- [`mpv`](https://mpv.io/)
- [`streamlink`](https://streamlink.github.io/) — Twitch streams
- [Kitty terminal](https://sw.kovidgoyal.net/kitty/) — thumbnail previews

## Build

```bash
cargo build --release
```

Binary is at `target/release/vidi`.

## Configuration

Config files are created automatically on first run:

| File | Purpose |
|------|---------|
| `~/.config/vidi/vidi.conf` | Player, quality, result limits |
| `~/.config/vidi/subscriptions` | YouTube channel URLs (one per line) |
| `~/.config/vidi/twitch.conf` | Twitch player and quality settings |
| `~/.config/vidi/twitch_subs` | Twitch usernames (one per line) |
| `~/.config/vidi/custom_playlists.json` | Custom playlist URLs |

## Keybindings

| Key | Action |
|-----|--------|
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `PgUp` / `PgDn` | Page up / down |
| `Enter` | Select |
| `Esc` | Back |
| `q` | Quit |
| Type anything | Filter list |
| `Backspace` | Delete filter character |
