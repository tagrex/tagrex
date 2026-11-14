#!/usr/bin/env python3
"""Build/verify offline UI fonts. See app/ui/assets/FONT-PROVENANCE.md.

Sources are commit- and SHA256-pinned in FONT-SOURCES.json. Downloads and all
candidate builds remain in --work-dir. No dependency on a network font service.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import urllib.parse
import urllib.request

import brotli
import fontTools
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.recordingPen import RecordingPen, DecomposingRecordingPen

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / 'app/ui/assets'
FAMILIES = [('inter', 'TagRex Inter', 'tagrex-inter'),
            ('ibmplexsans', 'TagRex Sans', 'tagrex-sans'),
            ('sourcesans3', 'TagRex Source Sans', 'tagrex-source-sans'),
            ('jetbrainsmono', 'TagRex Mono', 'tagrex-mono')]
WEIGHTS = (400, 500, 600, 700)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def rename(font, family, italic=False):
    """Rename primary/internal identities; preserve author, license and copyright."""
    if family == 'TagRex Source Sans':
        family = 'TagRex Humanist'  # Adobe reserves the primary name 'Source'.
    style = 'Italic' if italic else 'Regular'
    ps = family.replace(' ', '') + '-' + style
    names = {1: family, 2: style, 3: 'TagRex: ' + ps + ':1',
             4: family + ' ' + style, 6: ps, 16: family, 17: style,
             18: family + ' ' + style, 21: family, 22: style,
             25: family.replace(' ', '')}
    table = font['name']
    # Remove legacy platform/language versions to prevent conflicting identities.
    table.names = [n for n in table.names if n.nameID not in names]
    for key, value in names.items():
        table.setName(value, key, 3, 1, 0x409)
    for record in table.names:
        if record.nameID >= 256:
            text = record.toUnicode()
            for original in ('IBMPlexSansCondensed', 'IBMPlexSansCond', 'IBMPlexSans', 'IBM Plex Sans',
                             'SourceSans3', 'Source Sans 3', 'JetBrainsMono', 'JetBrains Mono', 'Inter'):
                if original in text:
                    text = text.replace(original, family.replace(' ', ''))
                    break
            record.string = text.encode(record.getEncoding())
    if 'fvar' in font:
        for i, instance in enumerate(font['fvar'].instances):
            if instance.postscriptNameID != 0xFFFF:
                table.setName(ps + '-Instance' + str(i), instance.postscriptNameID, 3, 1, 0x409)
    font.recalcTimestamp = False


def encoded(font):
    font.flavor = 'woff2'
    out = io.BytesIO()
    font.save(out)
    return out.getvalue()


def outline(font, weight, text='Hamburgefontsiv АБВ абв 0123456789'):
    # Use glyph-set interpolation directly: comparison measures real outlines.
    gs = font.getGlyphSet(location={'wght': weight})
    cmap = font.getBestCmap()
    pen = RecordingPen()
    for ch in text:
        gs[cmap[ord(ch)]].draw(pen)
    return digest(repr(pen.value).encode())


def required_characters():
    # Explicit alphabets supplement translated source strings (which may omit letters).
    alphabets = (
        'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'
        'ÀÂÆÇÉÈÊËÎÏÔŒÙÛÜŸàâæçéèêëîïôœùûüÿ'
        'ÄÖÜẞäöüßÁÉÍÓÚÜÑáéíóúüñ¿¡ÀÈÉÌÍÒÓÙÚàèéìíòóùú'
        'АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ'
        'абвгдеёжзийклмнопрстуфхцчшщъыьэюя'
        'АБВГҐДЕЄЖЗИІЇЙКЛМНОПРСТУФХЦЧШЩЬЮЯ'
        'абвгґдеєжзиіїйклмнопрстуфхцчшщьюя'
        '0123456789‘’“”„«»–—…•−×÷€£°'
    )
    return set(map(ord, alphabets))


def verify_asset(path, family, italic, source_cmap=None):
    font = TTFont(path, recalcTimestamp=False)
    binary_family = 'TagRex Humanist' if family == 'TagRex Source Sans' else family
    assert font['name'].getDebugName(1) == binary_family, path
    assert font['name'].getDebugName(6) == binary_family.replace(' ', '') + ('-Italic' if italic else '-Regular'), path
    assert bool(font['OS/2'].fsSelection & 1) == italic, path
    assert bool(font['head'].macStyle & 2) == italic, path
    assert font['OS/2'].usWeightClass == 400, path
    assert 'SIL OPEN FONT LICENSE' in font['name'].getDebugName(13).upper(), path
    assert font['name'].getDebugName(0), path
    cmap = set(font.getBestCmap())
    assert not (required_characters() - cmap), (path, sorted(required_characters() - cmap))
    if source_cmap is not None:
        assert cmap == source_cmap, ('Lost upstream characters', path)
    if family == 'TagRex Mono':
        for record in font['GSUB'].table.FeatureList.FeatureRecord:
            if record.FeatureTag in ('calt', 'liga', 'clig', 'dlig'):
                assert not record.Feature.LookupListIndex
    axis = font['fvar'].axes
    assert len(axis) == 1 and axis[0].axisTag == 'wght', path
    assert (axis[0].minValue, axis[0].defaultValue, axis[0].maxValue) == (400, 400, 700), path
    hashes = {str(w): outline(font, w) for w in WEIGHTS}
    assert len(set(hashes.values())) == 4, ('Identical weight outlines', path)
    for w in WEIGHTS:
        instance = instantiateVariableFont(font, {'wght': w}, inplace=False)
        assert instance['OS/2'].usWeightClass == w, (path, w)
    # Source files include non-font symbols in JS. Check all actual letters from
    # the seven locale source files, plus ordinary punctuation, without claiming
    # emoji/symbols or arbitrary music metadata are universally available.
    locale_checks = {}
    for locale in ('en', 'de', 'es', 'fr', 'it', 'ru', 'uk'):
        text = (ROOT / f'app/ui/js/i18n/{locale}.js').read_text()
        letters = {ord(c) for c in text if c.isalpha()}
        missing = letters - cmap
        assert not missing, (path, locale, sorted(missing))
        locale_checks[locale] = len(letters)
    return dict(bytes=path.stat().st_size, sha256=digest(path.read_bytes()),
                codepoints=len(cmap), weight_outline_sha256=hashes,
                locale_source_letters=locale_checks)


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--work-dir', required=True, type=Path)
    parser.add_argument('--verify-only', action='store_true')
    args = parser.parse_args()
    assert fontTools.__version__ == '4.61.1', fontTools.__version__
    assert brotli.__version__ == '1.2.0', brotli.__version__
    manifest = json.loads((ASSETS / 'FONT-SOURCES.json').read_text())
    args.work_dir.mkdir(parents=True, exist_ok=True)
    for entry in manifest['files']:
        local = args.work_dir / entry['path']
        if not local.exists():
            assert not args.verify_only, 'Verification requires cached pinned sources'
            local.parent.mkdir(parents=True, exist_ok=True)
            url = entry.get('url') or ('https://raw.githubusercontent.com/google/fonts/' + manifest['commit'] + '/' + urllib.parse.quote(entry['path']))
            local.write_bytes(urllib.request.urlopen(url).read())
        assert digest(local.read_bytes()) == entry['sha256'], local
        if entry['path'].endswith('/OFL.txt'):
            target = ASSETS / ('OFL-' + entry['path'].split('/')[1] + '.txt')
            # Preserve every notice word, normalizing only line/trailing whitespace.
            notice = '\n'.join(line.rstrip() for line in local.read_text().splitlines()) + '\n'
            if not args.verify_only:
                target.write_text(notice)
            assert target.read_text() == notice
    report = {'tool_versions': {'fonttools': fontTools.__version__, 'Brotli': brotli.__version__}, 'assets': {}}
    css = []
    for folder, family, stem in FAMILIES:
        for italic in (False, True):
            entry = next(e for e in manifest['files'] if e['path'].startswith('ofl/' + folder + '/')
                         and e['path'].endswith('.ttf') and ('Italic' in e['path']) == italic)
            local = args.work_dir / entry['path']
            if not local.exists():
                assert not args.verify_only, 'Verification requires cached pinned sources'
                local.parent.mkdir(parents=True, exist_ok=True)
                url = entry.get('url') or ('https://raw.githubusercontent.com/google/fonts/' + manifest['commit'] + '/' + urllib.parse.quote(entry['path']))
                local.write_bytes(urllib.request.urlopen(url).read())
            assert digest(local.read_bytes()) == entry['sha256'], local
            source = TTFont(local, recalcTimestamp=False)
            source_cmap = set(source.getBestCmap())
            dest = ASSETS / (stem + ('-italic' if italic else '') + '.woff2')
            limits = {a.axisTag: (400, 400, 700) if a.axisTag == 'wght' else a.defaultValue for a in source['fvar'].axes}
            font = instantiateVariableFont(source, limits, inplace=False)
            rename(font, family, italic)
            if folder == 'jetbrainsmono' and 'GSUB' in font:
                # Preserve literal file/tag reading: no programming ligatures.
                for record in font['GSUB'].table.FeatureList.FeatureRecord:
                    if record.FeatureTag in ('calt', 'liga', 'clig', 'dlig'):
                        record.Feature.LookupListIndex = []
                        record.Feature.LookupCount = 0
            variable = encoded(font)
            font = TTFont(io.BytesIO(variable), recalcTimestamp=False)
            if args.verify_only:
                assert variable == dest.read_bytes(), ('Non-reproducible asset', dest)
            else:
                # Measure the alternative: four static, equally complete faces.
                static_sizes = []
                for weight in WEIGHTS:
                    static = instantiateVariableFont(font, {'wght': weight}, inplace=False)
                    static_sizes.append(len(encoded(static)))
                dest.write_bytes(variable)
                report.setdefault('format_comparison', {})[dest.name] = dict(variable_bytes=len(variable), four_static_bytes=sum(static_sizes), static_bytes=static_sizes)
            report['assets'][dest.name] = verify_asset(dest, family, italic, source_cmap)
            css.append(f'@font-face {{\n  font-family: "{family}";\n  font-style: {"italic" if italic else "normal"};\n  font-weight: 400 700;\n  font-display: swap;\n  src: url("assets/{dest.name}") format("woff2");\n}}\n')
    # Preserve the optional data face from a packaged, renamed source subset.
    # No dependence on fork Git history: this also works in source archives.
    entry = manifest['condensed']
    packaged = entry['portable_source']
    raw = (ROOT / packaged['path']).read_bytes()
    assert digest(raw) == packaged['sha256']
    original = TTFont(io.BytesIO(raw), recalcTimestamp=False)
    upstream = entry['upstream']
    upstream_file = args.work_dir / 'IBM-Plex-Condensed-Regular.ttf'
    if not upstream_file.exists():
        assert not args.verify_only, 'Missing cached IBM source'
        upstream_file.write_bytes(urllib.request.urlopen(
            'https://raw.githubusercontent.com/IBM/plex/' + upstream['commit'] + '/' + upstream['path']).read())
    assert digest(upstream_file.read_bytes()) == upstream['sha256']
    full = TTFont(upstream_file)
    old_glyphs, full_glyphs = original.getGlyphSet(), full.getGlyphSet()
    for cp, name in original.getBestCmap().items():
        full_name = full.getBestCmap()[cp]
        left, right = DecomposingRecordingPen(old_glyphs), DecomposingRecordingPen(full_glyphs)
        old_glyphs[name].draw(left)
        full_glyphs[full_name].draw(right)
        assert left.value == right.value, ('Condensed upstream outline mismatch', cp)
        assert original['hmtx'][name] == full['hmtx'][full_name], cp
    condensed = ASSETS / 'tagrex-ui-condensed.woff2'
    rename(original, 'TagRex Condensed')
    if not args.verify_only:
        condensed.write_bytes(encoded(original))
    else:
        assert encoded(original) == condensed.read_bytes(), 'Non-reproducible condensed'
    actual = TTFont(condensed)
    assert actual.getBestCmap() == original.getBestCmap()
    assert actual['hmtx'].metrics == original['hmtx'].metrics
    assert actual['glyf'].compile(actual) == original['glyf'].compile(original)
    assert actual['name'].getDebugName(1) == 'TagRex Condensed'
    assert actual['OS/2'].usWeightClass == 400
    report['assets'][condensed.name] = dict(bytes=condensed.stat().st_size, sha256=digest(condensed.read_bytes()), codepoints=len(actual.getBestCmap()), unchanged_glyphs_and_metrics=True)
    css.append('@font-face {\n  font-family: "TagRex Condensed";\n  font-style: normal;\n  font-weight: 400;\n  font-display: swap;\n  src: url("assets/tagrex-ui-condensed.woff2") format("woff2");\n}\n')
    css_path = ROOT / 'app/ui/fonts.css'
    if not args.verify_only:
        css_path.write_text('\n'.join(css))
    assert css_path.read_text() == '\n'.join(css)
    report['total_woff2_bytes'] = sum(e['bytes'] for e in report['assets'].values())
    (args.work_dir / ('verify-report.json' if args.verify_only else 'build-report.json')).write_text(json.dumps(report, indent=2) + '\n')
    if args.verify_only:
        recorded = json.loads((ASSETS / 'FONT-VERIFICATION.json').read_text())
        assert recorded['assets'] == report['assets']
    else:
        (ASSETS / 'FONT-VERIFICATION.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
