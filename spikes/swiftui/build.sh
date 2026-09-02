#!/usr/bin/env bash
# Build the SwiftUI stand (#271) and assemble a runnable .app bundle.
#
#   ./build.sh            release build into build/TagRex Spike.app
#   ./build.sh --run      the same, then launch it
#
# The bundle is unsigned: macOS quarantines it on download, so a copy fetched
# from CI needs `xattr -dr com.apple.quarantine "TagRex Spike.app"` once.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
app="$here/build/TagRex Spike.app"

echo "==> Rust core (staticlib)"
cargo build --release -p tagrex-ffi --manifest-path "$repo/Cargo.toml"

echo "==> Fonts"
fonts="$here/Sources/TagRexSpike/Fonts"
fetch() {
  local name="$1" url="$2"
  [ -f "$fonts/$name" ] && return 0
  echo "    fetching $name"
  curl -sSfL "$url" -o "$fonts/$name" || {
    echo "    (offline — the stand will fall back to the system faces)"
    rm -f "$fonts/$name"
  }
}
# The same families the web UI bundles as woff2 subsets, in TTF: IBM Plex Sans
# (OFL) and JetBrains Mono (Apache-2.0), taken from the Google Fonts mirror.
fetch "IBMPlexSans.ttf" \
  "https://github.com/google/fonts/raw/main/ofl/ibmplexsans/IBMPlexSans%5Bwdth,wght%5D.ttf"
fetch "JetBrainsMono.ttf" \
  "https://github.com/google/fonts/raw/main/ofl/jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf"

echo "==> Swift"
cd "$here"
swift build -c release

echo "==> Bundle"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp ".build/release/TagRexSpike" "$app/Contents/MacOS/TagRexSpike"
# The fonts go straight into the bundle: SwiftPM's resource accessor resolves
# against the build tree it was compiled in, which is fine on the machine that
# built it and fatal on any other.
if compgen -G "$fonts/*.ttf" > /dev/null; then
  mkdir -p "$app/Contents/Resources/Fonts"
  cp "$fonts"/*.ttf "$app/Contents/Resources/Fonts/"
fi

cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>TagRex Spike</string>
  <key>CFBundleDisplayName</key><string>TagRex Spike (SwiftUI)</string>
  <key>CFBundleExecutable</key><string>TagRexSpike</string>
  <key>CFBundleIdentifier</key><string>dev.tagrex.spike.swiftui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticTermination</key><true/>
</dict>
</plist>
PLIST

echo "==> Built: $app"

if [ "${1:-}" = "--run" ]; then
  open "$app"
fi
