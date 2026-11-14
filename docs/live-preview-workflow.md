# Live preview workflow

Editor input, release imports and artwork choices share one pending operation.
The spreadsheet displays the result; **Apply** writes it and creates the normal
Undo batch. Typing, selecting a release and changing a preset do not write tags.

## Ordering and scope

1. Start with the saved tags.
2. Overlay the chosen release for each import scope.
3. Overlay fields the user has edited.
4. Run the checked transformations over those proposed values.
5. Validate the complete result before enabling Apply.

Raw input stays separate from transformed output. Turning a preset off and on
therefore does not repeat its effects on its previous output. The Editor keeps
the raw value available for correction, while the spreadsheet shows the final
proposed value. Copying spreadsheet cells copies that displayed value.

Changing the selected tracks keeps pending edits attached to their original
paths. A new release replaces the previous import for the same scope; imports
for other tracks remain pending. Manual field and artwork overrides remain in
force. Opening a release card only inspects it. Ctrl+Z outside text input can
undo a pending release choice; text input retains its native typing Undo.

The bottom status area reports the selection and modified-file scope. Discard
clears pending inputs and suppresses the same automatic preview until a new
input, selection or preset change. Invalid fields or rejected artwork block
the whole operation. Apply and Undo temporarily freeze the mutation surfaces;
Cancel remains reachable during Apply.

## Responsiveness

Raw values appear in the table synchronously. Native validation and transforms
run asynchronously after a short typing delay, with one request active and only
the newest waiting revision retained. The same limit applies to rapid release
import choices. Stale results cannot replace newer work. Apply waits for the
validated current revision.

Ordinary value updates patch affected visible rows and retain an active editing
cell. Horizontal-only movement does not rerun the virtual row renderer. Pixel
trackpad scrolling remains native; discrete horizontal wheel steps and
Shift-wheel move mostly immediately and settle over 60 ms. Pointer or keyboard
input interrupts the tween, and reduced motion removes it. The filename scrolls
with the tags; the small selection and artwork bands remain pinned.

Native tag metadata reuses the fingerprint-checked cache. Scoped Apply reads
back the changed tracks, and custom columns invalidate only the affected cached
paths and field dependencies. Artwork payloads use the existing per-plan pool.

## Boundaries

- Changes are pending in memory until Apply. Session resume stores view state,
  not an unsaved edit buffer.
- Export in the cover menu exports saved artwork. When artwork changes are
  pending, the label explicitly says **Export saved cover**.
- Structural move, block-conversion and spreadsheet batch tools retain their
  explicit review boundary.
- This iteration is debug-only. It does not replace the existing AppImage or
  update the submitted upstream pull request.

Verification details are recorded in `docs/ui-live-preview/verification.json`.
