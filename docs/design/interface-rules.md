# Interface rules

For corner, spacing, typography or font changes, read the shared [Visual system](visual-system.md) and edit the named style owner.

Current direction agreed during the Settings and workbench restyles. Apply these rules when
iterating on the experimental interface; older design briefs are historical context.

- Use the shared [surface palettes](palettes.md), typography and familiar control
  shapes. A Theme choice applies coordinated surfaces and a stronger primary
  from the same colour family in both modes. Keep older custom preferences
  stored for recovery when migrating to complete presets.
- Use spacing, tone and rounded surfaces to group content. Avoid outlining every
  field, row or section. Add a divider only when it marks a useful boundary.
- Use the shared `--input-radius` for standard inputs, selects and compound
  fields. It is currently 10px. Keep action buttons' established pill shapes.
- Give a focused field one clean accent outline. Compound fields own that
  outline around the whole group; avoid a second frame on the inner input.
  Use the error colour for invalid fields.
- Prefer a quiet filled input over a strong resting border when its surrounding
  surface gives enough contrast. Retain clear hover, focus and error states.
- Shadows represent depth and use dark translucent colours in both themes.
  Never derive a shadow from the text colour, which creates a pale dark-mode glow.
- Keep a quiet border around the outer dialog to separate it from the app.
  Inset a recessed content area from the outer edge and round its corners. Avoid
  combining that surface change with a redundant separator line.
- Build hierarchy with page headings, section headings, labels and concise help.
  Keep related help close to its setting. Avoid adding a card around every group.
- Keep the folder bar with the file table and common commands in the toolbar.
  Show docking handles through View → Arrange panels rather than permanent chrome.
- Align the permanent player beneath the file pane as the docks resize. Anchor
  Preview and Apply beneath the inspector; keep the draft-clear control in the
  pinned staged summary, visible only while edits exist. Put the affected file count
  in the primary action's natural-language label. Keep feedback from resizing
  the table during an active edit.
- Use barely different row fills without divider borders; keep selection, staged
  changes, invalid values and keyboard focus distinct. Keep artwork centred.
- Preserve visible labels, keyboard access, selection and validation feedback.
  Check both themes and narrow windows. Quiet styling must stay discoverable.

These are shared visual defaults, not a reason to remove meaningful table,
selection, drag-position or error boundaries. Apply changes in reviewable steps.
