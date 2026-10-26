#!/usr/bin/env sh
# Install Nori for the current user.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bindir="${HOME}/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"

cargo build --manifest-path "$root/Cargo.toml" --release -p nori

mkdir -p "$bindir" "$apps" "$icons"
install -m 755 "$root/target/release/nori" "$bindir/nori"
install -m 644 "$root/packaging/nori.desktop" "$apps/nori.desktop"
install -m 644 "$root/packaging/nori.svg" "$icons/nori.svg"

echo "Installed Nori to $bindir/nori"
echo "Make sure $bindir is on your PATH."
