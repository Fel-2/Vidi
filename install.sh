#!/bin/sh
# curl -fsSL https://raw.githubusercontent.com/Fel-2/Vidi/main/install.sh | sh
# PREFIX and VERSION override the install directory and release.
set -eu

REPO="Fel-2/Vidi"
PREFIX="${PREFIX:-$HOME/.local/bin}"

die() {
	echo "install: $*" >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

need curl
need tar

case "$(uname -s)" in
Linux) ;;
*) die "only Linux is supported; build from source with cargo install --git https://github.com/$REPO" ;;
esac

case "$(uname -m)" in
x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
*) die "unsupported architecture: $(uname -m)" ;;
esac

version="${VERSION:-}"
if [ -z "$version" ]; then
	version=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
		sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
	[ -n "$version" ] || die "could not determine the latest release"
fi

name="vidi-$version-$target"
url="https://github.com/$REPO/releases/download/$version/$name.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "Downloading $name…"
curl -fsSL "$url" -o "$tmp/$name.tar.gz" || die "download failed: $url"
curl -fsSL "$url.sha256" -o "$tmp/$name.tar.gz.sha256" || die "checksum download failed"

echo "Verifying checksum…"
if command -v sha256sum >/dev/null 2>&1; then
	(cd "$tmp" && sha256sum -c "$name.tar.gz.sha256" >/dev/null) || die "checksum mismatch"
elif command -v shasum >/dev/null 2>&1; then
	expected=$(cut -d' ' -f1 <"$tmp/$name.tar.gz.sha256")
	actual=$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d' ' -f1)
	[ "$expected" = "$actual" ] || die "checksum mismatch"
else
	echo "install: no sha256 tool found, skipping verification" >&2
fi

tar -C "$tmp" -xzf "$tmp/$name.tar.gz"
mkdir -p "$PREFIX"
install -m755 "$tmp/$name/vidi" "$PREFIX/vidi"
echo "Installed $version to $PREFIX/vidi"

case ":$PATH:" in
*":$PREFIX:"*) ;;
*) echo "Note: $PREFIX is not on your PATH" >&2 ;;
esac

missing=""
for bin in yt-dlp mpv; do
	command -v "$bin" >/dev/null 2>&1 || missing="$missing $bin"
done
[ -z "$missing" ] || echo "Note: install these for full functionality:$missing" >&2
