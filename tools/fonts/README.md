# Bundled font subsets

`app/ui/assets/tagrex-*.woff2` are subsets of three open-licensed families
(SIL OFL 1.1, licence text in `app/ui/assets/THIRD-PARTY-FONT-LICENSES.md`).
`build_subsets.py` rebuilds them from the official releases.

| File | Source | Version | OpenType features |
|---|---|---|---|
| `tagrex-sans.woff2` | IBM Plex Sans Regular | 3.005 | all |
| `tagrex-sans-medium.woff2` | IBM Plex Sans SemiBold | 3.005 | all |
| `tagrex-sans-bold.woff2` | IBM Plex Sans Bold | 3.005 | all |
| `tagrex-ui-condensed.woff2` | IBM Plex Sans Condensed Regular | 3.000 | all |
| `tagrex-mono.woff2` | JetBrains Mono Regular | 2.304 | none (code ligatures stripped) |

Hinting is kept in all of them. Coverage: Latin, Latin-ext, Vietnamese, Greek,
Cyrillic, combining marks, digits and punctuation.

## Sources

Download and unpack these release archives into one folder; the script finds
each file in it by name and refuses any other version:

- `ibm-plex-sans.zip` from the IBM/plex release `@ibm/plex-sans@1.1.0`
- `ibm-plex-sans-condensed.zip` from `@ibm/plex-sans-condensed@2.0.0`
  (the 1.1.0 package still ships Condensed 1.003, without Cyrillic)
- `JetBrainsMono-2.304.zip` from the JetBrains/JetBrainsMono release `v2.304`

The source fonts are not committed.

## Use

The script needs `fonttools` and `brotli` (`python3 -m pip install fonttools brotli`,
in a virtual environment if you like). The app itself depends on neither.

Check that the recipe still reproduces what is bundled, writing nothing:

```bash
python3 tools/fonts/build_subsets.py ~/font-sources --check
```

Each font should report `matches the bundled file`. Then rebuild, adding the
code points you need (repeat `--add` for several ranges):

```bash
python3 tools/fonts/build_subsets.py ~/font-sources --add 0370-03FF
```

Every subset is built from the code points the bundled file already has plus
the added ones that the source contains, so a rebuild never drops a character.
Code points a source lacks are reported and skipped. Without `--out` the files
in `app/ui/assets` are replaced.

After a change, update the coverage line above and in
`THIRD-PARTY-FONT-LICENSES.md`, and the comments on the `@font-face` rules at
the top of `app/ui/style.css`.
