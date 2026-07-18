# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/) once a
first release ships.

## [Unreleased]

### Added

- Workspace skeleton: `tagrex-core` (tag model, mask engine, transform
  pipeline, transaction pipeline, undo journal — all module signatures in
  place, `TagEngine` I/O still `todo!()`), `tagrex-providers-discogs`
  (provider trait implementation, HTTP client still `todo!()`), and a
  placeholder `app` binary proving the workspace links.
- CI on GitHub Actions: `cargo fmt --check`, `clippy -D warnings`, `build`,
  `test`.
- Tracking issues for the remaining implementation order from
  `docs/architecture.md` (#1-#7).
