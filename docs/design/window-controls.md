# Design brief: closing the in-app windows, per platform

> For Claude Design. The repo is linked locally, so **read the current code
> directly** rather than trusting this summary: `app/ui/index.html`,
> `app/ui/style.css`. Your earlier rounds are the reference for the house
> style, above all `foundations/mask-builder.*`, `foundations/transform-windows.*`
> and `foundations/small-dialogs.*`. This brief is **intent only**; the answer
> is yours.

## The problem

Every in-app window closes with a × at the **right** end of its head:

- Settings (`#settings`, `.settings-head`, `#settings-close`), the largest;
- the mask builder (`#mask-builder`, `#mb-close`);
- the Transform window (`#transform-modal`, `#transform-modal-close`);
- the preset editor (`#pe-editor`, `#pe-editor-close`);
- Add column (`#colmask-modal`, `#colmask-close`).

On Windows that is where a close button lives. On macOS it reads as foreign:
the app window itself has its controls on the **left** (the traffic lights sit
in our own top bar, `body.mac-overlay-titlebar`), so a window inside it with a
× on the right looks borrowed from another system. On Linux it depends on the
desktop: GNOME and most others put the close button on the right, but the
layout is a user setting and can be on the left.

## What the app can know

- The platform, reliably: macOS, Windows or Linux.
- On Linux the desktop's own button layout can be read (GNOME's
  `org.gnome.desktop.wm.preferences button-layout`, for example
  `close,minimize,maximize:` puts close on the left); where it can't be read,
  right is the safe default.

## Open questions

1. **macOS**: where the close goes. The left end of the head, mirroring the
   traffic lights? As a traffic-light-like control, or the house's own ×? Or no
   close control at all, as many macOS sheets do, leaving Cancel / Done and
   Esc? The heads also carry a title (small caps, muted) that would then sit
   after it or in the middle.
2. **Windows and Linux**: keep the × on the right; on a Linux desktop set to
   close-on-the-left, follow it?
3. **Consistency across the five windows**: Settings has Cancel / Save in its
   foot, the builder has Use this mask, Transform has none (it applies as you
   tick). Does that change which windows need a close control at all?
4. Whether a window's head changes in any other way per platform (title
   alignment, the gap the close leaves).

## Deliverable

A preview card (light and dark) showing one head per platform for a large
window (Settings) and a small one (Add column), the CSS to fold back into
`app/ui/style.css`, and how the platform is expressed in markup (a class on
`body` such as `platform-mac`, `close-left`, or similar), as in the earlier
rounds. Prefer extending the existing `.modal-head` / `.settings-head` over new
components.
