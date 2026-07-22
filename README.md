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
- ⚡ Fast metadata via the Innertube API (no yt-dlp subprocess for lists; yt-dlp remains as fallback)
- 🔥 Trending videos
- 🔍 Search with history
- 📡 Subscription feed (parallel fetch, sorted by upload time, instant cached startup with background refresh)
- 📋 Browse subscribed channels (Videos / Shorts / Streams / Playlists)
- ➕ Subscribe to channels directly from Explore Channels
- 📥 Import subscriptions from NewPipe JSON, OPML or YouTube Takeout CSV (Miscellaneous → Import Subscriptions)
- ⏭ Play-next queue (`Tab` to queue from any list, YouTube → Queue to play)
- ⏯ Watch progress: playback position is tracked via mpv IPC and resumed on the next watch
- 🚫 SponsorBlock: mark segments as mpv chapters, cut them from downloads (`SPONSORBLOCK` in `vidi.conf`)
- 🙈 Shorts hidden everywhere by default — lists, search, feed and the channel Shorts tab (`SHOW_SHORTS: true` re-enables)
- 🎯 Custom playlists
- 🕐 Recently watched
- 🔎 Channel search via Miscellaneous → Explore Channels

**Twitch**
- 🔍 Live stream search
- 💜 Subscription status in one fast batched API call (live/offline, viewers, uptime), with a `streamlink` fallback
- 🔥 Top live streams (Twitch directory)
- 🗂 Browse categories → live streams per game, with box-art previews
- ➕ Follow / unfollow channels in-app (writes `twitch_subs`)
- 🎬 VOD browsing via fast GQL (Past Broadcasts / Highlights / Uploads / Premieres), with view counts, durations and thumbnails — for subscribed channels or any stream you find; resumes where you left off; download with audio/quality options
- 💬 Live chat viewer with real Twitch name colours and broadcaster/mod/vip/sub badges (auto-reconnects)

**General**
- Inline thumbnail previews (kitty / Ghostty graphics protocol, or iTerm2 / WezTerm)
- Inline video playback via `mpv` / `yt-dlp`
- Download support (video + audio-only, single + batch)
- Save/unsave videos to a local watchlist
- Filter bar on all list screens
- Help overlay on `?`

## Dependencies

- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp)
- [`mpv`](https://mpv.io/)
- [`streamlink`](https://streamlink.github.io/) — Twitch streams
- A graphics-capable terminal for thumbnail previews: [Kitty](https://sw.kovidgoyal.net/kitty/), [Ghostty](https://ghostty.org/), [iTerm2](https://iterm2.com/), or [WezTerm](https://wezterm.org/) (optional)

## Install

### Arch / Artix

```bash
git clone https://codeberg.org/Fel/Vidi.git
cd Vidi/packaging
makepkg -si
```

### From source

```bash
cargo build --release
```

Binary is at `target/release/vidi`.

## Configuration

Config files are created automatically on first run:

| File | Purpose |
|------|---------|
| `~/.config/vidi/vidi.conf` | Player, quality, result limits, `WATCH_PROGRESS`, `SPONSORBLOCK` |
| `~/.config/vidi/subscriptions` | YouTube channel URLs (one per line) |
| `~/.config/vidi/twitch.conf` | Twitch player and quality settings |
| `~/.config/vidi/twitch_subs` | Twitch usernames (one per line) |
| `~/.config/vidi/custom_playlists.json` | Custom playlist URLs |

In `subscriptions`, each line is a channel URL, optionally followed by whitespace
and a display name (`https://www.youtube.com/channel/UC…  My Channel`). Providing
the name inline lets the Channels view skip the per-channel `yt-dlp` lookup, so it
loads instantly. Lines without a name are resolved once and cached in
`~/.cache/vidi/channel_names.json`.

To annotate an existing URL-only file in place, run:

```bash
scripts/name-subscriptions.py
```

It reuses the cache where possible and only calls `yt-dlp` (4 at a time) for
names it doesn't already know. The original is backed up to `subscriptions.bak`.

## Keybindings

| Key | Action |
|-----|--------|
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `PgUp` / `PgDn` | Page up / down |
| `Enter` | Select |
| `Esc` | Back |
| `q` | Quit |
| `Tab` | Add video to queue (in Queue: remove) |
| `?` | Help overlay |
| Type anything | Filter list |
| `Backspace` | Delete filter character |
| Mouse wheel | Scroll lists |

Single-key overrides can be set in `vidi.conf` (`KEY_UP`, `KEY_DOWN`, `KEY_SELECT`,
`KEY_BACK`, `KEY_QUIT`, `KEY_PAGE_UP`, `KEY_PAGE_DOWN`). Arrow and vim keys always work.

## Troubleshooting

### A video occasionally fails to start

Now and then mpv exits immediately with something like:

```
[ffmpeg] https: HTTP error 403 Forbidden
Failed to open https://rr2---sn-....googlevideo.com/videoplayback?...
No video or audio streams selected.
```

**Just play it again — it usually works on the second or third try.**

This is a transient rejection by YouTube's media CDN, not a problem with vidi,
yt-dlp or your setup. Because `bestvideo+bestaudio` streams the video and audio
tracks from two separate URLs, one of them being refused is enough to abort
playback. The refused URL is still valid: requesting it again seconds later
succeeds. Measured on a sample of 60 launches, roughly 15% failed this way, in
bursts.

Retrying is safe here — the 403 comes from the media servers, not from YouTube's
API, so it is not a rate limit or a bot check and does not count against you.

One exception: do **not** keep retrying an error that says "Sign in to confirm
you're not a bot", "Video unavailable", or names a geo restriction. Those are
permanent for that request, and hammering the bot check can get your IP flagged
for real.

## License

MIT — see [LICENSE](LICENSE).
