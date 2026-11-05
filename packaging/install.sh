#!/usr/bin/env sh
# Install Nori for the current user.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bindir="${HOME}/.local/bin"
datadir="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="${datadir}/applications"
icons="${datadir}/icons/hicolor"

cargo build --manifest-path "$root/Cargo.toml" --release -p nori

mkdir -p "$bindir" "$apps"
install -m 755 "$root/target/release/nori" "$bindir/nori"
install -m 644 "$root/packaging/nori.desktop" "$apps/nori.desktop"

for size in 16 24 32 48 64 128 256; do
  dir="${icons}/${size}x${size}/apps"
  mkdir -p "$dir"
  install -m 644 "$root/packaging/icons/hicolor/${size}x${size}/apps/nori.png" "$dir/nori.png"
done

# The icon used to be a single scalable SVG. Drop the stale copy so themed
# lookups do not pick the old magnifier over the new sizes.
rm -f "${icons}/scalable/apps/nori.svg"
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f "$icons" || true
fi

echo "Installed Nori to $bindir/nori"
echo "Make sure $bindir is on your PATH."
