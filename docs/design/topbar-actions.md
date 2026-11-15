# Design brief: the top bar's action buttons beside the mode pill

> For Claude Design. The repo is linked locally, so **read the current code
> directly**: `app/ui/index.html` (row 1 of `.topbar`: `#undo`, `#redo`,
> `#settings-open`, the `.topbar-div` hairline, `#panel-toggle`, and the update
> pill when there is one) and `app/ui/style.css` (`button.icon`, `.topbar`,
> `#settings-open.gear` with its `has-token` dot, `/* ---- mode tabs ---- */`,
> `body.compact-tabs`, `body.tabs-classic`). The earlier rounds this follows
> are the cards `foundations/mode-tabs-pill` and `foundations/window-controls`.
> This brief is **intent only**; the answer is yours.

## The idea

Since the mode tabs became a segmented pill (a 36px rounded track holding
28px segments), the icon buttons at the right end of the same row read as
small: they are 28px squares, sized to the segments inside the pill rather
than to its outline. A Redo button has just joined Undo, so the right end now
holds four buttons: Undo, Redo, Settings, a hairline, then the panel toggle.

The user asks whether those buttons should become **one pill of their own**,
the same track as the modes, instead of separate squares. Two simpler answers
were also tried in the mock and are fair game: the same squares at 36px, or
36px circles.

## Open questions

1. **One track, two, or none?** Undo and Redo are a pair; Settings and the
   panel toggle act on the app's window rather than on the library. Does one
   track hold all four, do Undo/Redo get their own small track with the other
   two left as single buttons (or a second track), or do separate buttons
   sized to the pill read better? The hairline exists today to separate the
   panel toggle; what replaces it?
2. **States inside a track.** These are actions, not a choice: nothing is
   "active" the way a mode is. Hover, `:focus-visible` (`--ring`), pressed,
   and disabled (Undo and Redo are disabled most of the time; see the
   `disabled-states` card) must each read clearly inside the track, and a
   track whose two buttons are both disabled must not look broken.
3. **The settings dot and the update pill.** `#settings-open.has-token`
   carries a small status dot at its corner; the update pill appears in the
   same row when an update is ready. Where do they sit against a track?
4. **Narrow windows and the classic look.** With `body.compact-tabs` the mode
   labels drop to icons; with `body.tabs-classic` (a Settings choice) the
   modes are flat tabs with an underline and no track at all. What do the
   action buttons do in each? Matching the classic look may mean keeping
   them as they are there.
5. **Platforms.** On macOS row 1 starts after the traffic lights
   (`body.mac-overlay-titlebar`); on Windows and Linux the window controls
   from the `window-controls` card sit at the far right of the same row, next
   to these buttons. The answer has to sit well beside both.

## Deliverable

A preview card (light and dark) with the full row 1 in context: wide, narrow
(compact icon-only modes), the classic look, macOS with the traffic lights,
and Windows/Linux with the window controls; each button state side by side,
including Undo/Redo both disabled; and the CSS to fold into
`app/ui/style.css` (plus any markup change to `index.html`, kept to wrapping
the existing buttons), as in the earlier rounds.
