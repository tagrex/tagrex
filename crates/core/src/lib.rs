//! TagRex core.
//!
//! Module boundaries follow `docs/architecture.md` in the repository root.
//! The one invariant everything else follows from: **no module writes tags or
//! renames files directly** — every operation is compiled into a
//! [`plan::ChangePlan`], previewed, applied through [`plan::Executor`], and
//! journaled for rollback.

pub mod export;
pub mod journal;
pub mod mask;
pub mod matching;
pub mod model;
pub mod plan;
pub mod provider;
pub mod scanner;
pub mod transform;
