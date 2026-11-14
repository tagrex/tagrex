# Interface contributions

Follow [the experiment's ground rules](next.md). Keep interface PRs aimed at
`next` and independently useful native changes aimed at `main`.

```sh
gh pr create --repo tagrex/tagrex --base next
```

For a standalone backend fix, use `--base main`. Explicitly name any dependency
on a native PR until it has merged and reached `next` through the shared base.

Use small, feature-focused changes and commit messages. Include Linux runtime
evidence and list the Windows/macOS testing still needed. Performance comparisons
use the same files and machines on both branches, with feature differences
recorded separately. Keep private working notes and real-library data out of
the published branch.
