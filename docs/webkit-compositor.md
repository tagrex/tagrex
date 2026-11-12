# WebKitGTK 2.54 fade workaround

## Scope: the interface experiment

This contribution targets `next`. The flash was observed with the experimental
component interface and its transitions. Mike's preliminary tests did not
reproduce it in the static interface on `main`; this is not proof that `main`
can never trigger the underlying WebKit issue. There is currently no evidence
justifying this workaround, unsafe feature lookup or startup restart on `main`.
The other shared native contributions remain independent PRs against `main`.

The standalone probe below isolates a CSS fade. A result from that synthetic
page does not by itself establish that the actual interface is affected or
fixed. Both the probe and the experimental interface need native evidence.

The Linux shell selects TextureMapper and disables partial compositing only on
WebKitGTK 2.54 when `WEBKIT_USE_SKIA_FOR_COMPOSITION` is not explicitly set.
Feature functions are dynamically resolved from the linked WebKit library;
missing symbols leave defaults alone. Other platforms and WebKit series keep
their defaults. A one-shot termination callback recreates the initial web
process after settings change. This startup behaviour needs native validation.

## Standalone reproduction

Prerequisites: Linux, GTK3, WebKitGTK 4.1 API, Python PyGObject and a visible X11
session. No frontend compilation, npm installation or TagRex library is used.
Run these separately, keeping the small probe window visible and unobscured:

```
GDK_BACKEND=x11 python tests/native/compositor.py --compositor system
GDK_BACKEND=x11 python tests/native/compositor.py --compositor compatible
```

The probe samples GDK presentation pixels rather than reading CSS opacity. It
runs six open/close cycles and reports calibration and large reverse colour
steps. A useful result requires calibration in both modes; compare reversal
counts and visible flashes. Exit 0 means calibrated with no detected reversal,
1 indicates a failed comparison, and 2 means no completed result (for example a
missing display or timeout). It is a synthetic fade probe, not a claim that all
artwork transitions are covered.

Affected runtime observed in the original local investigation: WebKitGTK
2.54.1. The new standalone reproduction has only been syntax checked during
this split; its native before/after result remains to be confirmed on affected
hardware. The PR stays a draft until that evidence is available. Original
related upstream report: https://github.com/SimoHypers/limusic/issues/433.

Also test the debug app starts exactly once without a blank window on 2.54,
that an explicit environment override bypasses the workaround, and that older
and newer WebKit versions retain their startup behaviour.

## Launch-path acceptance gate

`take_launch_paths` drains the native queue before the frontend finishes opening
those files. This workaround restarts an already-loading web process; there is
no acknowledgement/replay protocol protecting a handoff consumed by the first
page. It must not be assumed safe solely because restart usually happens early.
No native launch-path preservation result is claimed for this draft.

Before marking this ready, use an isolated debug instance with disposable files
on WebKitGTK 2.54 and record the exact version, GPU/driver, display backend and
startup logs. Check cold starts with a file argument, a folder argument and
multiple files (including spaces/non-ASCII names). Verify the expected library
and every supplied file appear after the one-time restart, with no lost paths
or duplicate opens. Also check an OS "Open with" launch and a second-instance
handoff after startup. Exercise both defaults and the explicit override.

Use controlled startup delay/instrumentation to exercise a handoff before the
restart; repeated successful ordinary starts alone do not rule out that race.
If the first page can drain the queue before being terminated, preserve/replay
unacknowledged paths or avoid the restart before calling the workaround safe.
This remains a draft gate even if the rendering probe passes.

## Removal or narrowing criteria

The current match covers all `2.54.x`, including a future fixed point release.
For each candidate WebKit update, record its full version and the same machine's
GPU/driver, display backend, environment and probe results. On affected 2.54.1,
first establish a calibrated failing `system` result and a calibrated passing
`compatible` result; neither result has been established for this standalone
probe yet. A missing or uncalibrated result is not a pass.

For a candidate fixed version, run the probe repeatedly in both modes on that
same setup. Both must calibrate all twelve phases with no reversals, and the
actual experimental artwork/dialog transitions that originally flashed must
also remain clean with system defaults. Verify launch-path handling and startup
without the workaround. Record the results in this PR.

If a 2.54 point release meets those checks, exclude that exact release and later
verified fixed versions from the workaround instead of leaving the whole series
matched. Keep the known affected versions covered. Remove the workaround and
its Linux-only dependencies entirely once the experiment's minimum supported
WebKit version is verified fixed and older affected versions are no longer
supported. A version bump alone is not evidence of a rendering fix.
