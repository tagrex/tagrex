#!/usr/bin/env python3
"""Build the bundled font subsets in app/ui/assets from the official releases.

The five tagrex-*.woff2 files are subsets of IBM Plex Sans, IBM Plex Sans
Condensed and JetBrains Mono. Each one is rebuilt from the code points the
bundled file already has, plus any ranges passed with --add, so a rebuild
never drops a character the interface relies on.

    python3 tools/fonts/build_subsets.py SOURCES --check
    python3 tools/fonts/build_subsets.py SOURCES --add 0370-03FF

SOURCES is a folder holding the unpacked release archives (see README.md).
--check rebuilds into a temporary folder and compares the result with the
bundled files without writing anything; run it before replacing them.
"""

import argparse
import logging
import sys
import tempfile
from pathlib import Path

try:
    from fontTools import subset
    from fontTools.ttLib import TTFont
except ImportError:
    sys.exit("needs fontTools and brotli: python3 -m pip install fonttools brotli")

ASSETS = Path(__file__).resolve().parents[2] / "app" / "ui" / "assets"

# bundled file, source file, expected source revision, OpenType features kept,
# name records kept. The mono keeps no features, so its code ligatures stay
# stripped and "->" never fuses into an arrow in a file name.
FONTS = [
    ("tagrex-sans", "IBMPlexSans-Regular.ttf", 3.005, ["*"], None),
    ("tagrex-sans-medium", "IBMPlexSans-SemiBold.ttf", 3.005, ["*"], None),
    ("tagrex-sans-bold", "IBMPlexSans-Bold.ttf", 3.005, ["*"], None),
    ("tagrex-ui-condensed", "IBMPlexSansCondensed-Regular.ttf", 3.000, ["*"], None),
    ("tagrex-mono", "JetBrainsMono-Regular.ttf", 2.304, [], ["*"]),
]


def parse_range(text):
    """'0370-03FF', 'U+0370-U+03FF' or a single '20AC' -> a range of code points."""
    parts = [p.strip().upper().removeprefix("U+") for p in text.split("-")]
    if len(parts) not in (1, 2) or not all(parts):
        raise argparse.ArgumentTypeError(f"not a code point range: {text}")
    try:
        start, end = int(parts[0], 16), int(parts[-1], 16)
    except ValueError:
        raise argparse.ArgumentTypeError(f"not a code point range: {text}") from None
    if start > end:
        raise argparse.ArgumentTypeError(f"range runs backwards: {text}")
    return range(start, end + 1)


def find_source(sources, file_name, revision):
    """The copy of file_name under sources whose revision is the expected one."""
    found = {}
    for path in sorted(sources.rglob(file_name)):
        found[path] = round(TTFont(path, lazy=True)["head"].fontRevision, 3)
    for path, rev in found.items():
        if rev == revision:
            return path
    seen = ", ".join(f"{p} ({r})" for p, r in found.items()) or "none"
    sys.exit(f"{file_name} {revision} not found under {sources}; found: {seen}")


def build(sources, out, extra):
    out.mkdir(parents=True, exist_ok=True)
    for name, file_name, revision, features, name_ids in FONTS:
        src = find_source(sources, file_name, revision)
        current = set(TTFont(ASSETS / f"{name}.woff2").getBestCmap())
        available = set(TTFont(src).getBestCmap())
        wanted = current | (extra & available)
        opts = subset.Options()
        opts.flavor = "woff2"
        opts.layout_features = features
        opts.hinting = True
        opts.notdef_outline = True
        if name_ids:
            opts.name_IDs = name_ids
        font = subset.load_font(str(src), opts)
        subsetter = subset.Subsetter(opts)
        subsetter.populate(unicodes=wanted)
        subsetter.subset(font)
        subset.save_font(font, str(out / f"{name}.woff2"), opts)
        missing = len(extra - available)
        note = f", {missing} requested code points not in the source" if extra and missing else ""
        print(f"{name}: {len(current)} -> {len(wanted)} code points{note}")


def features(font, tag):
    if tag not in font:
        return []
    return sorted({r.FeatureTag for r in font[tag].table.FeatureList.FeatureRecord})


def outline(font, glyph):
    return list(font["glyf"][glyph].getCoordinates(font["glyf"])[0])


def compare(bundled, rebuilt):
    """Differences between two builds of one font, empty when they match."""
    a, b = TTFont(bundled), TTFont(rebuilt)
    ca, cb = a.getBestCmap(), b.getBestCmap()
    metrics = ("usWinAscent", "usWinDescent", "sTypoAscender", "sTypoDescender", "sTypoLineGap")
    checks = {
        "glyph count": (len(a.getGlyphOrder()), len(b.getGlyphOrder())),
        "tables": (sorted(a.keys()), sorted(b.keys())),
        "GSUB features": (features(a, "GSUB"), features(b, "GSUB")),
        "GPOS features": (features(a, "GPOS"), features(b, "GPOS")),
        "names": tuple(sorted((n.nameID, n.platformID, n.langID, n.toUnicode()) for n in f["name"].names) for f in (a, b)),
        "OS/2 metrics": tuple([getattr(f["OS/2"], m) for m in metrics] for f in (a, b)),
        "hhea metrics": tuple((f["hhea"].ascent, f["hhea"].descent, f["hhea"].lineGap) for f in (a, b)),
        "code points": (sorted(ca), sorted(cb)),
    }
    diffs = [what for what, (x, y) in checks.items() if x != y]
    shared = set(ca) & set(cb)
    widths = sum(a["hmtx"][ca[c]] != b["hmtx"][cb[c]] for c in shared)
    shapes = sum(outline(a, ca[c]) != outline(b, cb[c]) for c in shared)
    if widths:
        diffs.append(f"{widths} advance widths")
    if shapes:
        diffs.append(f"{shapes} outlines")
    return diffs


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("sources", type=Path, help="folder with the unpacked release archives")
    parser.add_argument("--add", type=parse_range, action="append", default=[], metavar="RANGE",
                        help="extra code points, e.g. 0370-03FF; repeatable")
    parser.add_argument("--out", type=Path, default=ASSETS, help="where to write (default: app/ui/assets)")
    parser.add_argument("--check", action="store_true",
                        help="rebuild into a temporary folder and compare with the bundled files")
    args = parser.parse_args()
    logging.getLogger("fontTools.subset").setLevel(logging.ERROR)
    extra = {cp for r in args.add for cp in r}
    if not args.check:
        build(args.sources, args.out, extra)
        return 0
    with tempfile.TemporaryDirectory() as tmp:
        build(args.sources, Path(tmp), extra)
        failed = False
        for name, *_ in FONTS:
            diffs = compare(ASSETS / f"{name}.woff2", Path(tmp) / f"{name}.woff2")
            print(f"{name}: " + ("matches the bundled file" if not diffs else "differs in " + ", ".join(diffs)))
            failed |= bool(diffs)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
