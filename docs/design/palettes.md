# Complete interface themes

Settings offers one **Theme** picker: Navy, Graphite, Cupertino, Catppuccin,
Nord, Tokyo Night, Rosé Pine, Gruvbox, Solarized and Everforest. Each applies
surfaces, text, selections, focus and a coordinated primary. No separate accent
picker or “use palette accent” step remains. Appearance Auto/Light/Dark stays
independent; Auto follows the operating system. Both variants of each theme
are bundled offline and need no runtime download.

Navy dark keeps the exact approved shell `#1a1f34`, filled surface `#252a40`
and stripe `#2b3046`. Its primary is now blue rather than an opposing amber.
Navy light uses neutral chrome and a white working surface instead of the old
blue-grey wash. Graphite and Cupertino are app-owned adaptations; Cupertino
uses Apple-style neutrals and blue, without copying platform controls.

The seven named community themes map their real palettes into GUI surface
roles. Their source backgrounds and primary colours are pinned in
[theme-references.json](theme-references.json), together with primary-source
URLs and captured source SHA-256 values. The light variants are Catppuccin
Latte, Tokyo Night Day, Rosé Pine Dawn, Gruvbox Light, Solarized Light and
Everforest Medium Light. Nord light adapts its official Snow Storm colours.
Dark variants use Mocha, Storm, Rosé Pine Main, Gruvbox Dark, Solarized Dark,
Everforest Medium Dark and Nord Polar Night respectively.

## Mapping and verification

- `--panel` is chrome/sidebar, `--bg` is the main filled work/control surface.
- Named theme backgrounds remain exact upstream colours. Navy, Graphite and
  Cupertino retain their own app-designed surface families.
- `--text`, `--muted` and `--field-ghost` retain the source hue and are shifted
  only as needed for readable GUI text: body 7:1, helper 5.3:1, placeholder 4.5:1
  against the shell, working surface, stripe and hover fill. Navy's existing
  approved dark values remain unchanged.
- Quiet stripes, hover and border tones are derived from that theme's own
  background/foreground. They share the same purpose, rather than forcing
  unrelated palettes to have identical numerical lightness.
- Primary fill/text/ink reuse the existing contrast derivation. Success and
  error semantics stay green/red, with text ink adjusted only for contrast
  against each theme and its validation tint. Opposing decorative accents are
  not used.

`palettes.test.js` checks every light/dark pair, source anchors, primary/button
contrast, initialization, migration and OS-mode changes. The UI matrix checks
all ten themes across five tools, settings, editor, staging and preset dialog.
Rendered evidence belongs outside the source repository.

## Persistence

`tagrex.palette` retains its legacy IDs (`forest` → Everforest, `plum` → Rosé
Pine), preserving saved selections. Deliberately choosing a preset records
`tagrex.themeAccent=preset`; its primary then follows light/dark automatically.
Old custom accents remain stored for recovery; the new complete-theme
preference takes over their presentation without erasing them. Appearance controls persist immediately through Cancel and reload.
Reset General chooses Navy/Auto and the matching primary. This is frontend-only
view state: backend settings and library/journal data are unchanged.

## Primary references

- [Apple colour guidance](https://developer.apple.com/design/human-interface-guidelines/color)
- [Catppuccin palette](https://catppuccin.com/palette/)
- [Nord palette and role guidance](https://www.nordtheme.com/docs/colors-and-palettes/)
- [Tokyo Night colours and screenshots](https://github.com/folke/tokyonight.nvim)
- [Rosé Pine palette and variants](https://rosepinetheme.com/palette/)
- [Gruvbox source and examples](https://github.com/morhetz/gruvbox)
- [Solarized palette, roles and screenshots](https://ethanschoonover.com/solarized/)
- [Everforest source and examples](https://github.com/sainnhe/everforest)
