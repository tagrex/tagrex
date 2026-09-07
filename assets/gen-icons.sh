#!/usr/bin/env bash
# Compose the platform app icons from the two source SVGs (#67).
#
#   assets/icon.svg        the full mark — tag + hole + eighth-note cutout
#   assets/icon-small.svg  the note-less silhouette, for the sizes where the note
#                          blurs into the hole and reads as noise (16/24/32 px)
#
# `cargo tauri icon` renders ONE source at every size, so a straight run would
# downscale the detailed mark into mush at the small sizes. This composes both
# instead: it keeps whatever the detailed run already produced under app/icons/
# and only rewrites the small raster entries — the .ico 16/24/32 frames, the
# .icns 16 and 32 px images, and the standalone 32x32.png — from the note-less
# mark. Everything >= 48 px keeps the detailed mark.
#
# Run it after regenerating app/icons/ from assets/icon.svg (or any time the
# mark changes). macOS only: it uses sips + iconutil.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icons="$root/app/icons"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# High-res (512 px) renders of each source; sips downscales from these.
echo "==> rendering the source SVGs"
"$HOME/.cargo/bin/cargo" tauri icon "$root/assets/icon.svg" -o "$tmp/detailed" >/dev/null
"$HOME/.cargo/bin/cargo" tauri icon "$root/assets/icon-small.svg" -o "$tmp/small" >/dev/null
detailed="$tmp/detailed/icon.png"
small="$tmp/small/icon.png"

# Downscale a 512 px base PNG to a square size.
render() { sips -z "$2" "$2" "$1" --out "$3" >/dev/null; }

echo "==> 32x32.png (note-less)"
render "$small" 32 "$icons/32x32.png"

echo "==> icon.icns: 16 and 32 px from the note-less mark"
iconset="$tmp/icon.iconset"
iconutil -c iconset "$icons/icon.icns" -o "$iconset"
render "$small" 16 "$iconset/icon_16x16.png"
render "$small" 32 "$iconset/icon_16x16@2x.png"
render "$small" 32 "$iconset/icon_32x32.png"
iconutil -c icns "$iconset" -o "$icons/icon.icns"

echo "==> icon.ico: 16/24/32 note-less, 48/64/256 detailed"
frames="$tmp/frames"
mkdir -p "$frames"
for size in 16 24 32; do render "$small" "$size" "$frames/$size.png"; done
for size in 48 64 256; do render "$detailed" "$size" "$frames/$size.png"; done
python3 "$root/assets/pack-ico.py" "$icons/icon.ico" \
  "$frames/16.png" "$frames/24.png" "$frames/32.png" \
  "$frames/48.png" "$frames/64.png" "$frames/256.png"

echo "==> done: app/icons/{32x32.png,icon.icns,icon.ico}"
