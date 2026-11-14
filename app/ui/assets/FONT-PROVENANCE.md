# Self-hosted interface fonts

Fetched and checked against primary distributor/upstream sources on 2026-11-14.
`fonts.css` is the font-face authority. Each interface family has separate real
upright and italic WOFF2 variable files, with a 400–700 weight axis and default
400. Weights 400, 500, 600 and 700 are actual interpolated designs. The optional
condensed data face remains a single real 400 face.

| UI / CSS family | Modified binary family | Design provenance |
| --- | --- | --- |
| TagRex Inter | TagRex Inter | Inter, Rasmus Andersson / Inter Project |
| TagRex Sans | TagRex Sans | IBM Plex Sans, IBM / Bold Monday |
| TagRex Source Sans | TagRex Humanist | Source Sans 3, Adobe / Paul D. Hunt |
| TagRex Mono | TagRex Mono | JetBrains Mono, JetBrains Project |
| TagRex Condensed | TagRex Condensed | IBM Plex Sans Condensed 3.000 |

The CSS alias for Source Sans is deliberately different from the modified
binary's family name: Adobe reserves the primary font name **Source**. IBM
reserves **Plex**. Renamed name-table primary identities, unique IDs, full names,
PostScript names, WWS names, variation PostScript prefixes and named-instance
PostScript identities use the TagRex names. Original copyright, licence,
authorship and trademark notices remain. OFL notice text is reproduced unchanged (only CRLF/trailing whitespace
is normalized for repository checks) in `OFL-*.txt`; the existing condensed notice remains in
`THIRD-PARTY-FONT-LICENSES.md`. Fonts are distributed under OFL 1.1, including
its conditions on distribution and modified names.

## Pinned inputs

`FONT-SOURCES.json` records every required TTF, OFL notice and family METADATA.pb
with SHA256 and byte count. The binary input distributor for Inter/Plex/Source Sans is
[google/fonts at 29b4ec6df2522a8aee279414b78a07910b065874](https://github.com/google/fonts/tree/29b4ec6df2522a8aee279414b78a07910b065874/ofl).
Mono comes directly from [JetBrains/JetBrainsMono at
19371302b95d218af43299bce79ddbddd0bc364d](https://github.com/JetBrains/JetBrainsMono/tree/19371302b95d218af43299bce79ddbddd0bc364d/fonts/variable),
using the explicit per-file URLs/checksums in the manifest. Google Fonts still
ships older Mono 2.211, which loses 56 baseline codepoints, so it was rejected.
These are the distributors' compiled font inputs; we do not claim to rebuild
upstream design sources. METADATA.pb preserves the upstream repository/release
pointers. The distributor commit and TTF checksums, rather than a mutable
release alias or inferred upstream build identity, pin the actual bytes used.

Inter derives from the distributor's 4.001 input (Inter 4.1 Google Fonts
archive); Plex Sans from 3.201. Check exact font revisions with fontTools and
consult the pinned METADATA.pb for designer/source details. Source Sans 3 is 3.052 and JetBrains Mono is 2.305 (verified
from the input name/head tables). All are pinned by their actual TTF bytes.

Condensed preserves the subset originally recorded at local checkpoint
`2eff3ae677cc17845afe5f7a09b048bcf55a4918`, with its original SHA256 retained as
provenance. The packaged, renamed subset is the canonical build input. Its
round-trip encoding is deterministic; the independent design check is the
outline/metric comparison below, rather than recreating the historical subset
from a new subsetting recipe. All 612 encoded glyph outlines and horizontal metrics were
independently compared with the actual
[IBM upstream 3.000 TTF](https://github.com/IBM/plex/blob/34cadd4234d443b041e7e109a4699ed508fd2b9d/packages/plex-sans-condensed/fonts/complete/ttf/IBMPlexSansCondensed-Regular.ttf)
and are equal. That upstream file is also commit/checksum pinned. The newer
3.001 upstream changes eight of those outlines; it was not substituted.

## Modifications and coverage

- Fix Inter's optical-size axis at 14 (its original default); fix Plex width at
  100 (its original default). Limit weight to 400–700 and set its default to 400.
- Preserve each of the eight interface source files' **entire Unicode cmap**;
  no arbitrary Unicode-range declarations or Latin-only subset. This preserves
  extended Latin, Cyrillic and all other character mappings in those inputs.
- Disable `calt`, `liga`, `clig` and `dlig` in Mono so literal file/tag data do
  not gain programming ligatures. Other shaping/features remain available.
- Rename identities and convert to WOFF2. Preserve the condensed face's exact
  existing glyph table, horizontal metrics and character mappings.
- Retain 700 in every interface variable file. The former separate sans-medium
  (actually 600) and sans-bold files are replaced by the variable sans face.

`FONT-VERIFICATION.json` records shipped hashes, bytes, character counts,
weight-outline hashes and measured variable-versus-four-static sizes. Variable
files are smaller for every normal/italic family in this full-coverage build.
Compared with the old Mono subset (2.304), the official Mono input restores
all its alphabetic/extended-Cyrillic characters. It omits the old U+00AD
soft-hyphen mapping, a discretionary formatting control handled by the text
engine; arbitrary input metadata still uses normal fallback.

The old asset totals are recorded in `FONT-SOURCES.json` under `baseline`.
Only the selected family/style is fetched by normal CSS font matching; declaring
all choices does not preload them. This is source behavior, not measured app
network timing.

The build verifies explicit English/German/Spanish/French/Italian/Russian/
Ukrainian alphabets, extended Latin punctuation and every alphabetic character
in all seven locale source files. The eight interface files contain them all.
This is character availability, not a claim that every combining sequence,
emoji, icon, arbitrary music-library script or shaping behavior was rendered.
The preserved condensed subset lacks capital sharp S U+1E9E, as before; normal
font fallback handles that character. It was deliberately not redesigned.

## Reproduce and verify

Use a Python environment on the task drive. Tested with CPython 3.14, fontTools
4.61.1 and Brotli 1.2.0. The small requirements file pins both build dependencies.
From the repository root:

```sh
python -m venv /mnt/Games/Development/tagrex/visual-consistency-delivery/fonts-work/venv
/mnt/Games/Development/tagrex/visual-consistency-delivery/fonts-work/venv/bin/pip install -r tools/build-ui-fonts-requirements.txt
/mnt/Games/Development/tagrex/visual-consistency-delivery/fonts-work/venv/bin/python tools/build-ui-fonts.py --work-dir /mnt/Games/Development/tagrex/visual-consistency-delivery/fonts-work
/mnt/Games/Development/tagrex/visual-consistency-delivery/fonts-work/venv/bin/python tools/build-ui-fonts.py --work-dir /mnt/Games/Development/tagrex/visual-consistency-delivery/fonts-work --verify-only
```

Build downloads only pinned input files into the work directory, checks each
checksum, writes assets and CSS, and records sizes/comparison evidence.
`--verify-only` uses the cached TTF sources to rebuild the eight variable
interface files, and deterministically re-encodes the canonical Condensed
subset. It requires byte equality with all nine shipped assets, checks
metadata/coverage/weights/outlines, and verifies the recorded report and CSS.
The build disables timestamp recalculation. Do not run Python with `-O`: the
verification assertions must remain enabled.

The renamed Condensed source subset is packaged at
`tools/font-sources/tagrex-condensed-subset.woff2`, with a pinned checksum in
`FONT-SOURCES.json`. Its original checkpoint remains provenance only; regeneration
does not require that Git object or any Git history. The build verifies every
retained glyph outline and metric against the independently pinned IBM source.
The packaged subset carries its existing renamed metadata; it introduces no new
glyph design. No external font URL,
local-font lookup or network font dependency exists at runtime.
