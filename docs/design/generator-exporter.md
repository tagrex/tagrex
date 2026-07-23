# Design brief — GENERATOR & EXPORTER mode panels

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/index.html` (`#panel-generator`, `#panel-exporter`),
> `app/ui/style.css`, and `app/ui/app.js` (`renderTransformRules`,
> `addTransformRule`, `syncExportKind`, `runExport`). This brief is intent only —
> it does not restate the markup.

## Context
Four right-hand mode panels sit over a persistent monospace file table; nothing
is written until a Preview diff is applied (journaled, undoable). RENAMER and
TAGGER have had a design pass; **GENERATOR and EXPORTER are the last two plain
panels.** Redesign both on the existing design system — reuse
`tokens.css`/`typography.css`/`states.css`/`inert.css` and recent components
(`editor.css` form-row language, `settings.html` `.prio-item` drag, `release-card`,
the `.seg`/`.seg-btn` control, `button.primary`/`button.icon`). Do **not** invent
new colours/spacing/type. Theme-aware (light + dark), self-contained, vanilla
HTML/CSS on our tokens — production-shaped, adaptable straight into
`app/ui/{index.html,style.css}`.

## GENERATOR — ordered cleanup-rule chain
The user builds an ordered list of rules (find-and-replace, change-case, remove-
diacritics), applied top-to-bottom to a chosen scope (all tags / one field / file
name), then previews. Redesign focus:
1. **Rule cards** on the system: a clear step number, the kind, a compact
   per-kind body, per-card actions. Order is semantic — make reordering
   first-class: a **drag handle** (like the Settings read-priority list) with
   ↑/↓ as keyboard/fallback, plus remove.
2. **Per-kind bodies**, tight: replace = find/replace + its three flags (regex /
   whole-word / match-case) as a neat inline group; case = the style choice as a
   `.seg` or select with the acronym note; diacritics = header-only.
3. **Chain affordances** — scope selector, "Add rule", "Preview changes" — in the
   system language; a real **empty state** on the inert motif.
4. Dense: a chain can be 8–10 rules in a ~320px panel.

## EXPORTER — write playlist / CSV / report into the library
A read-only export form (audio never touched): Format (Playlist M3U / CSV / mask
report), a Mask field shown only for the report, a File name, Export. Redesign
focus:
1. Design-system row language (aligned labels/inputs like EDITOR / Settings).
   Consider **Format as a `.seg` segmented control** — only three options — each
   with a one-line "what it produces" hint.
2. The conditional **Mask** field (report only) reveals cleanly.
3. Surface "read-only, writes into the library folder" as calm helper text, not a
   warning.

## Questions to answer in the deliverable
- **Q1** Reorder affordance: drag handle vs ↑/↓ in rule cards? Show the drag
  state; keep a keyboard path.
- **Q2** Rule-card density: one compact card that stays scannable at 8–10 rules.
- **Q3** EXPORTER format: segmented vs dropdown, and where the per-format hint
  sits.
- **Q4** Empty/default states: GENERATOR with no rules; EXPORTER's default form.

## Deliverable (into the "TagRex Design System" project)
Self-contained preview HTML/CSS: `foundations/generator.html` + `generator.css`,
`foundations/exporter.html` + `exporter.css`, plus a close-up of the three rule-
card types — each with a first-line `<!-- @dsCard group="GENERATOR" … -->` /
`group="EXPORTER"` marker, on our tokens, light + dark. A short README/spec with
the Q1–Q4 answers. No interactive `.dc.html` for integration.
