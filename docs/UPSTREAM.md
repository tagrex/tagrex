# Contribution coverage

## Current interface proposal

[Upstream #729](https://github.com/tagrex/tagrex/pull/729) remains the one-time component-interface foundation against `next`. Its original head `0e103998bc4665d6b9c410089185435e817c4e0d` included the earlier workbench, selection, filters, compact player, settings and transformation-preset work. The following fork checkpoints are now included as distinct review commits on that foundation, without duplicate PRs:

| Fork checkpoint | Proposed behavior | Curated commit |
| --- | --- | --- |
| `3294b25` | Shared workbench surfaces and action footer | `bd3b4b09e8d0feb76ff3c2bc768b4698c43f4483` |
| `2eff3ae` | Tagging inspector and grouped previews | `3d7832026155f71358165da393494a9dfc0e7b22` |
| `56a3c56` | Visual system and four offline font families | `0f4aac1221670fb021123b3ce50d449ec5050348` |
| `cef52f1` | Dropdown/context-menu positioning and focus | `53c408321d9c12693c795ef83889f2db970cd47b` |
| `fe75c3b` | Files/Staged review and inspector refinements | `1f3f76026f1f60fff6ce67ec3ac0bf4eedddb560` |
| `b8199e4` | Ten paired light/dark theme presets | `43987a6784a3ac80d1b67c810b33d1a2cbe91071` |
| Catalogue validation correction | Six staging-key quotes recognized by native validation | `a35b36116ad4c4e3d523fa5a928b09a27dc24ca1` |

The additive compilation metadata and regressions from [#761](https://github.com/tagrex/tagrex/pull/761), head `5affdb3233a62e26adb1e85a4be9b05cb3789404` (including accepted-cache integration and the cached-row DTO regression), are included as an explicit **pending-main dependency** so this inspector's compilation controls have complete backend support. The standalone proposal must be reviewed through `main`, then integrated into `next` and reconciled with this copy. Inclusion here is not upstream acceptance.

## Shared native proposals and merge order

| Proposal | Route / status |
| --- | --- |
| [#737](https://github.com/tagrex/tagrex/pull/737) command queues | accepted/merged, `main` |
| [#738](https://github.com/tagrex/tagrex/pull/738) rollback recovery | accepted/merged, `main` |
| [#739](https://github.com/tagrex/tagrex/pull/739) metadata-only history | accepted/merged, `main` |
| [#740](https://github.com/tagrex/tagrex/pull/740) provider clients/streamed hashes | accepted/merged, `main` |
| [#750](https://github.com/tagrex/tagrex/pull/750) natural comparison | accepted/merged, `main` |
| [#751](https://github.com/tagrex/tagrex/pull/751) sidecar batching | accepted/merged, `main` |
| [#741](https://github.com/tagrex/tagrex/pull/741) display cache | accepted/merged, `main` |
| [#742](https://github.com/tagrex/tagrex/pull/742) shared artwork | open/proposed, `main`; separate maintainer reconciliation |
| [#743](https://github.com/tagrex/tagrex/pull/743) guarded exports | open/proposed, `main`; separate filesystem/maintainer gates |
| [#761](https://github.com/tagrex/tagrex/pull/761) native compilation metadata | open/proposed, `main`; pending dependency included here |
| [#744](https://github.com/tagrex/tagrex/pull/744) compositor workaround | open/proposed, `next`; separate native-rendering gates |

The remaining open proposals are ready-for-review rather than draft as of this refresh; this is not approval. #741 was accepted during reconciliation and its actual main history was merged into this foundation without changing the already-integrated source. Closed #734 is superseded by focused native proposals and must not be merged as additional coverage.

The bundled native snapshot is now concretely reconciled with accepted upstream `main` at `1d7fc3d81a6f935a1e8f7bfacc5206f74f12c68a` (including merged #741) and the following separately reviewed proposal heads. Their branches were not modified:

| Integration | Source head | Reconciliation commit |
| --- | --- | --- |
| Accepted main, AppleDouble scanner, dependency safeguards and ordered lock IPC | `0b8029a8a2617bd5fdc93aee5f6f17ac9b7aa279` | `cbf2ebc` |
| Accepted #741 main history | `1d7fc3d81a6f935a1e8f7bfacc5206f74f12c68a` | `a70d346` |
| #741 display cache documentation/current-main refresh | `f8601c2ac55b9e9662378e4e0bf69a1e3058b70f` | `a2c92ac` |
| #742 simpler request-local artwork map | `526108c3ebde8c158c0c6cd5d02bcc1dbc1680af`, accepted-cache ancestry `1530d6ea57d5cfd2220a9204b3f9b21e5dfe4f02` | `533ac9c`, `afa34fd` |
| #743 guarded outputs, FAT fallback and filesystem CI | `26c2d7d1e82fd01b354bce53f64aea9c875948cb`, workflow corrections `499c3988465c3234ba55d6fa522c13faad65437c`, `f70891db05e860b3d4716421a8f8ff5912e2c995` | `0234485`, `e592a49`, `c763ae4` |

The cache, encoder and output modules match those source heads exactly. The interface's `read_track`, `read_track_rows`, `preview_live_edits_with_base`, `render_columns`, plan/base-plan pooling, preview validity, cache invalidation and BPM session-generation guards remain. Main's waveform shell command/state/registration is restored alongside the unchanged stable player API. Ordered field-lock IPC is adapted to retain mounted component controls and `reflectLockButton`.

The latest #761 head `5affdb3233a62e26adb1e85a4be9b05cb3789404` is integrated at `4215fc2`. Its cached-row DTO regression verifies that both initial and cached `list_tracks` reads retain the native compilation key. That integration added only the regression; the field was already preserved in `track_dto`. The later fallback correction below changes core key selection while retaining that cached-row wiring.

The simpler artwork module is deliberately used upstream. In the parent's disclosed release microbenchmark (100 references to a 1.9 MB image, nine repetitions), median time changed from 3.26 ms to 88.84 ms with identical output. This is a measured simplicity/performance tradeoff, not equivalent performance; the personal fork retains its original faster encoder. The maintainer was informed in [#742's discussion](https://github.com/tagrex/tagrex/pull/742#issuecomment-6879665997).

Source reconciliation is complete for these exact heads; upstream acceptance is not. Review/merge the remaining #742/#743/#761 proposals through `main`, flow accepted main into `next`, then reconcile any later source movement before merging #729. Their inclusion here does not authorize or perform upstream merges. Hosted filesystem/platform gates and experiment evaluation remain separate. Future UI contributions should be focused follow-ups once this foundation lands.

## Compatibility and outstanding evaluation

This refresh preserves #729's inline stable-readable journal, regular inline journal compatibility tests, stable waveform APIs, Next identity (`com.tagrex.next`), disabled native updater and release workflow. The fork's pooled journal format, personal branding, versions, releases and updater policy are excluded. No existing journal is converted. The standalone `undo_compat` example and Linux-only ignored capacity probe were removed as requested in #742; regular inline-format and sharing/round-trip tests remain. The UI checkpoint stops at `b8199e4`; the listed native proposal updates are explicitly included. Later parent work and fork-only adaptations remain separately owned.

Files/Staged views share a draft and retain explicit Preview/Apply/Undo. Table gestures retain model-order selection including grouped/offscreen rows; pending/invalid-filter guards remain. These workflow choices need their own evaluation, rather than replacing stable main's policies by assertion.

Exact-head hosted Linux, Windows, macOS and advisory gates remain required after publication. Local Linux checks do not establish native Windows/macOS appearance, native webview/input-to-paint comparison, large-library performance parity, packaging or releases. Maintainer judgment, acceptance/main-to-next integration of the pending native proposals and the `docs/next.md` experiment measurements remain separate blockers even with green CI.

## Reconciled local verification

The combined native/frontend source passes 731 workspace Rust tests (12 ignored), 87 JavaScript unit tests and 403 Chromium browser tests (6 optional scale skips), production frontend and native workspace builds, Rust formatting, workspace all-target Clippy with warnings denied and two private D-Bus MPRIS tests. These are local Linux checks. Hosted Windows/macOS/FAT/exFAT checks must run on the exact pushed head. Duplicate test declarations from overlapping histories were removed only after their bodies were verified equivalent; every regression remains active once.

The first reconciled-head macOS ExFAT job failed before Rust because direct image formatting was refused by hdiutil. The later reviewed workflow creates an unformatted disposable image, selects only its attached whole device from the returned plist, formats/mounts it and verifies the filesystem. Bash syntax passes. This workflow-only correction leaves the fully tested native/frontend source unchanged; actual filesystem jobs remain hosted gates.

After including #761's new cached-row DTO regression, the complete command-layer suites pass again (226 tests, no failures), along with command-layer all-target Clippy with warnings denied and formatting. The previously verified production source is unchanged; the added test proves compilation metadata survives the display cache.

## Bounded maintainer follow-up

The response to [#761 review 6879975757](https://github.com/tagrex/tagrex/pull/761#issuecomment-6879975757) is included from source `e4b9f93a319a1054612d6ecbdf9a3956ca7aab1d` as `71cf08b`. A capable read block keeps its native key; otherwise compilation metadata falls back to the format's primary write tag. ID3v1-only MP3, RIFF INFO WAV and text-only AIFF now expose TCMP rather than hiding a writable flag. All three core compilation tests and the cached-row DTO regression pass.

The portable uppercase nine-character volume label `TAGREXTEST` is included from workflow correction `5715a8a928909d7f19402dccd08efb2391744e6b` as `3dab762`. This addresses the invalid FAT/exFAT volume label identified in diskutil output. Shell syntax is verified; actual filesystem/platform jobs remain exact-head hosted gates.

Accepted main advanced during this bounded follow-up to `b44bbde2ac266f2946d2b4053a9778650386fb93`. It is merged at `f7427a7`, retaining both the existing semicolon-genre regressions and the new Ogg regressions. Unsupported Ogg codecs are refused instead of being mistaken for AAC and corrupted; supported Ogg codecs remain readable.

Fresh core/commands suites pass **623 tests, 8 ignored**, including the three compilation-key tests, cached-row metadata and both Ogg regressions. Formatting and core/commands all-target Clippy with warnings denied pass. Frontend source is byte-identical to the preceding 403-test checkpoint and was not rerun. Earlier full-workspace/frontend results above describe their recorded source; this native follow-up is covered by the fresh affected native suites. Standalone proposal acceptance/main-to-next integration, fresh hosted checks and experiment gates remain outstanding. No upstream merge is performed.
