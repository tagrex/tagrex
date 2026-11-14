# Filter workbench design

Status: the first implementation is on `ui/filter-workbench`: quick search,
common typed conditions, ALL/ANY matching, editable chips, versioned saved
searches, pending/error feedback and Apply guards. The compact dock is retained.
Numeric fields, nested groups and worker-based regex evaluation remain future work.
The clean fork contribution is prepared separately on `contribution/filter-workbench`
from the published foundation, without later local UI or private audit history.

## Product intent

Find a useful subset quickly, refine it without learning syntax, and keep the
table usable on large libraries. Match the existing compact workbench rather
than introducing a permanently expanded form or a separate filter screen.

## Progressive disclosure

1. Keep the collapsed strip. When active, show a readable summary, an indication
   of active conditions and Clear. Closing the panel must never disable a filter.
2. Expanded, show quick text search and Add filter. Ctrl/Cmd+F reveals and focuses
   search. Keep results live; Enter is not a separate Apply action.
3. Add filter opens a searchable, keyboard-accessible field picker. Start with
   Artist, Album, Title, Filename and Artwork. Extend to the existing tag fields,
   format and numeric metadata as the engine supports their types.
4. Reveal value controls only after a field is chosen. Text: contains, equals,
   starts with, is empty and is not empty. Artwork: embedded, missing, unknown.
   Numeric fields later get comparisons and ranges rather than text matching.
5. Represent committed conditions as removable, editable chips. A concrete
   initial target is `Search: aphex` plus `Artwork: missing`. Conditions combine
   with ALL by default; expose ANY only when multiple conditions exist.
6. Put case sensitivity, regex and saved searches under explicit secondary
   controls. Surface an active advanced mode in the summary so it cannot become
   an invisible reason for unexpected results. Defer nested boolean groups.

Do not turn ordinary words into conditions automatically. Keep existing field
queries such as `artist:aphex` and `cover:missing` working. Introduce richer text
syntax only with a defined grammar and lossless editing behavior.

## Interaction and safety

- Search narrows; it never edits tags or starts a transformation.
- Show matched and total counts, distinct from selected counts. Retain existing
  selections and explicitly show how many selected files are hidden. Preserve
  the current preview and Apply scope contracts.
- Keep health conditions visible as conditions too. Clearing an individual text
  condition must not clear a health condition. Clear all is explicitly broader.
- Escape closes a picker first, then returns focus to its trigger. It should not
  erase a query or unexpectedly affect the surrounding editor/preview workflow.
- Chips have separate edit and remove buttons with accessible names. Backspace
  in an empty search must not silently delete a condition.
- Announce settled counts politely. Avoid announcing every keystroke or moving
  keyboard focus when results change. Keep table keyboard navigation intact.
- On narrow windows, wrap chips and let the picker use available width. Avoid
  horizontal scrolling in the filter controls. Reuse current controls, tokens,
  visible focus styles and reduced-motion behavior in light and dark themes.

## Pending, empty and error states

- Reflect input immediately; debounce expensive matching briefly. During a long
  evaluation retain the last complete table with a quiet Updating indication.
  Do not pair the new query with an old count as if it were current.
- Prevent mutating actions from treating an older visible result set as the
  completed new scope. Preserve the established explicit preview/Apply boundary.
- For no results, show No files match, retain the conditions and offer clear or
  edit actions. Distinguish this from an empty library and a failed library open.
- Invalid regex or incomplete conditions show an inline explanation. Retain the
  last valid query and mark it clearly; never silently broaden the operation
  scope. This intentionally changes the current invalid-regex fallback and needs
  dedicated tests and a focused implementation change.

## Query and performance architecture

- Use one typed query model for quick search, conditions and saved searches.
  Give fields and operators stable IDs; translated labels are presentation.
  Compile predicates once per query revision and publish matches atomically.
- Keep the existing library DTOs and matching index outside deep Svelte state.
  Svelte owns the small query/control state; use derived state for summaries.
  Do not render a Svelte component for every library file.
- Reuse the existing virtual table, metadata cache and cancellable chunked scan.
  Cover matching uses the existing scan flag, never artwork decoding or per-file
  IPC. Expensive facet suggestions are lazy, bounded and revision-aware.
- Query, library or content changes invalidate in-flight results. Filtering must
  not mutate source rows, selected paths, staged values or an Undo record.
- Keep a pure evaluator boundary so matching can move to a worker if measured
  main-thread costs justify it. Avoid copying the entire library for each keypress.
- User regex needs separate adversarial-input evaluation: a time-sliced outer
  loop cannot interrupt a single pathological JavaScript regex call. Before
  exposing regex more prominently, choose a cancellable worker or a compatible
  bounded engine and document syntax differences.
- Saved searches contain query data and display preferences, not file selections,
  edits, tokens or native commands. Validate and version their schema.

## Small implementation PRs

1. Cover filtering prerequisite, already implemented: three stable statuses,
   label matching, locale behavior and the existing case/regex modes.
2. Typed query model and pure evaluator with compatibility tests for existing
   queries. No appearance change and no general boolean language yet.
3. Add filter picker, common conditions, chips and explicit ALL behavior. Test
   keyboard focus, responsive layout and preservation of selections/drafts.
4. Pending/error handling and explicit scope information, including Apply guards.
5. Saved searches, additional field types and ANY matching as separate additions.
6. Worker/index optimization only after matched measurements identify a need.

Each layer must build and have its own behavioral tests. No shared native changes
are bundled into an interface PR; those retain the existing main/next routing.

## Evidence and acceptance

The prerequisite passed 15 focused browser tests on this branch, including
20,000 synthetic rows, virtualization and superseding an in-flight query.
The new Tauri MCP connected to the live debug app and inspected only the main
window's filter controls without changing the app state. The Svelte MCP reviewed
the existing FilterPanel component and returned no issues or suggestions.

These are not native throughput measurements for the proposed engine. Benchmark
10,000 and 20,000 identical files, grouped and ungrouped, across all three native
webviews. Explore 50,000 and 100,000 synthetic rows for stress coverage. Record
input latency, completion latency, long tasks, memory and mounted row count.
Include rapid typing, cancellation, library changes, regex, many distinct facet
values and preserved selections/previews. Set performance budgets from those
measurements before describing the feature as ready for enterprise workloads.
