#!/usr/bin/env python3
"""Annotate a vidi `subscriptions` file with inline channel names.

Rewrites each `URL`-only line as `URL\tChannel Name` so the Channels view loads
without any per-channel yt-dlp lookups. Names already present in the file are
left untouched. Missing names are taken from vidi's own cache
(~/.cache/vidi/channel_names.json) when available, and only resolved via yt-dlp
as a last resort (4 at a time).

Usage:
    scripts/name-subscriptions.py [SUBSCRIPTIONS_FILE]

Defaults to ~/.config/vidi/subscriptions. The original file is backed up to
`<file>.bak` before writing.
"""

import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

MAX_WORKERS = 4  # keep modest: each worker spawns a yt-dlp (Python) process


def config_home() -> Path:
    return Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))


def cache_home() -> Path:
    return Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))


def load_cache() -> dict:
    path = cache_home() / "vidi" / "channel_names.json"
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return {}


def resolve_name(url: str) -> str | None:
    """Fetch a channel's display name via yt-dlp (metadata only, no entries)."""
    try:
        out = subprocess.run(
            ["yt-dlp", url, "-J", "--flat-playlist", "--playlist-items", "0",
             "--socket-timeout", "15", "--retries", "1"],
            capture_output=True, text=True, timeout=60,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if out.returncode != 0 or not out.stdout:
        return None
    try:
        data = json.loads(out.stdout)
    except ValueError:
        return None
    for key in ("channel", "uploader"):
        val = data.get(key)
        if val:
            return val.strip()
    title = data.get("title")
    if title:
        # Channel tab titles look like "Name - Videos"; strip the suffix.
        return title.split(" - ")[0].strip()
    return None


def main() -> int:
    if len(sys.argv) > 2:
        print(__doc__)
        return 2
    sub_path = Path(sys.argv[1]) if len(sys.argv) == 2 else config_home() / "vidi" / "subscriptions"

    if not sub_path.exists():
        print(f"error: {sub_path} not found", file=sys.stderr)
        return 1

    lines = sub_path.read_text().splitlines()
    cache = load_cache()

    # Pass 1: figure out which URLs still need a name.
    need: list[str] = []
    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        parts = stripped.split(None, 1)
        url = parts[0]
        has_name = len(parts) == 2
        if not has_name and url not in cache:
            need.append(url)

    # Resolve missing names (cache miss) concurrently.
    if need:
        print(f"Resolving {len(need)} channel name(s) via yt-dlp…", file=sys.stderr)
        with ThreadPoolExecutor(max_workers=MAX_WORKERS) as pool:
            for url, name in zip(need, pool.map(resolve_name, need)):
                if name:
                    cache[url] = name
                else:
                    print(f"  warn: could not resolve {url}", file=sys.stderr)

    # Pass 2: rewrite the file, preserving comments and blank lines.
    out_lines: list[str] = []
    annotated = 0
    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            out_lines.append(line)
            continue
        parts = stripped.split(None, 1)
        url = parts[0]
        if len(parts) == 2:
            out_lines.append(line)  # already named
            continue
        name = cache.get(url)
        if name:
            out_lines.append(f"{url}\t{name}")
            annotated += 1
        else:
            out_lines.append(line)  # leave as-is, retry next run

    backup = sub_path.with_suffix(sub_path.suffix + ".bak")
    backup.write_text("\n".join(lines) + "\n")
    sub_path.write_text("\n".join(out_lines) + "\n")
    print(f"Annotated {annotated} line(s). Backup: {backup}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
