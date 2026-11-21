#!/usr/bin/env python3
"""Regenerate every file in app/icons/ from the SVG masters in assets/.

Masters:
  icon.svg              Windows / Linux, 32 px and up (free-form tag)
  icon-small.svg        Windows / Linux, 16 and 24 px (simplified)
  icon-macos.svg        macOS, 32 px and up (tile with Apple's 824/1024 margin)
  icon-macos-small.svg  macOS, 16 px (simplified)

Rasterising goes through `cargo tauri icon` (resvg); the .icns is packed with
macOS `iconutil`, so run this on a Mac. Usage: python3 assets/build-icons.py

With --next, the same masters are recoloured from the accent green to yellow
and written to app/icons-next/, the icons of a TagRex Next build (#819), along
with app/ui/assets/logo-next.svg for its About dialog.
"""

import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
ICONS = ROOT / "app" / "icons"
NEXT_ICONS = ROOT / "app" / "icons-next"
UI_LOGO = ROOT / "app" / "ui" / "assets" / "logo.svg"

# The masters' greens and their yellow counterparts for a TagRex Next build.
NEXT_PALETTE = {
    "#3FD3A3": "#FFD84D",
    "#2FC493": "#F7C93A",
    "#0B7D5C": "#B5830A",
    "#0A6E52": "#9E7208",
    "#D2F3E6": "#FFF1C2",
    "#D6F4E8": "#FFF2C8",
}

# Pixel sizes at or below this come from the simplified masters.
SMALL_MAX = 24
ICO_SIZES = [16, 24, 32, 48, 64, 256]
# iconset name -> pixel size
ICNS_SLOTS = {
    "icon_16x16.png": 16,
    "icon_16x16@2x.png": 32,
    "icon_32x32.png": 32,
    "icon_32x32@2x.png": 64,
    "icon_128x128.png": 128,
    "icon_128x128@2x.png": 256,
    "icon_256x256.png": 256,
    "icon_256x256@2x.png": 512,
    "icon_512x512.png": 512,
    "icon_512x512@2x.png": 1024,
}


def cargo():
    found = shutil.which("cargo")
    if found:
        return found
    fallback = Path.home() / ".cargo" / "bin" / "cargo"
    if fallback.exists():
        return str(fallback)
    sys.exit("cargo not found")


def tauri_icon(src, out, sizes=None):
    cmd = [cargo(), "tauri", "icon", str(src), "-o", str(out)]
    if sizes:
        cmd += ["-p", ",".join(str(s) for s in sorted(set(sizes)))]
    done = subprocess.run(cmd, capture_output=True, text=True)
    if done.returncode != 0:
        sys.exit(f"{' '.join(cmd)} failed:\n{done.stdout}{done.stderr}")


def render(tmp, master, small_master, sizes):
    """Return {size: png bytes}, taking small sizes from the simplified master."""
    big = [s for s in sizes if s > SMALL_MAX]
    small = [s for s in sizes if s <= SMALL_MAX]
    out = {}
    for src, group in ((master, big), (small_master, small)):
        if not group:
            continue
        dest = tmp / f"{src.stem}-png"
        tauri_icon(src, dest, group)
        for s in group:
            out[s] = (dest / f"{s}x{s}.png").read_bytes()
    return out


def write_ico(path, pngs):
    """ICO with PNG-compressed entries (supported since Windows Vista)."""
    sizes = sorted(pngs)
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    data = b""
    for s in sizes:
        blob = pngs[s]
        dim = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
        data += blob
    path.write_bytes(header + entries + data)


def recolour(text):
    for green, yellow in NEXT_PALETTE.items():
        text = text.replace(green, yellow).replace(green.lower(), yellow)
    return text


def main():
    global ASSETS, ICONS
    next_build = "--next" in sys.argv[1:]
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        if next_build:
            masters = tmp / "masters"
            masters.mkdir()
            for name in ("icon.svg", "icon-small.svg", "icon-macos.svg", "icon-macos-small.svg"):
                (masters / name).write_text(recolour((ASSETS / name).read_text()))
            ASSETS = masters
            NEXT_ICONS.mkdir(exist_ok=True)
            for f in ICONS.iterdir():
                if f.is_file() and not (NEXT_ICONS / f.name).exists():
                    shutil.copyfile(f, NEXT_ICONS / f.name)
            ICONS = NEXT_ICONS
            logo = UI_LOGO.with_name("logo-next.svg")
            logo.write_text(recolour(UI_LOGO.read_text()))
            print("wrote", logo.relative_to(ROOT))

        # Linux PNGs and the Windows Store tiles: the default tauri set from icon.svg.
        default = tmp / "default"
        tauri_icon(ASSETS / "icon.svg", default)
        for f in sorted(ICONS.iterdir()):
            if f.name in ("icon.icns", "icon.ico"):
                continue
            generated = default / f.name
            if generated.is_file():
                shutil.copyfile(generated, f)
                print("wrote", f.relative_to(ROOT))

        # Windows .ico
        win = render(tmp, ASSETS / "icon.svg", ASSETS / "icon-small.svg", ICO_SIZES)
        write_ico(ICONS / "icon.ico", win)
        print("wrote", (ICONS / "icon.ico").relative_to(ROOT))

        # macOS .icns
        mac = render(
            tmp, ASSETS / "icon-macos.svg", ASSETS / "icon-macos-small.svg",
            list(ICNS_SLOTS.values()),
        )
        iconset = tmp / "icon.iconset"
        iconset.mkdir()
        for name, s in ICNS_SLOTS.items():
            (iconset / name).write_bytes(mac[s])
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(ICONS / "icon.icns")],
            check=True,
        )
        print("wrote", (ICONS / "icon.icns").relative_to(ROOT))


if __name__ == "__main__":
    main()
