//! TagRex application entry point.
//!
//! This is a placeholder binary that validates workspace linkage. The Tauri
//! shell (webview window, frontend, IPC commands) is initialized as a
//! separate step on a development machine with `npm` available:
//!
//! ```text
//! cargo install tauri-cli
//! cargo tauri init
//! ```
//!
//! The GUI shell must stay thin: it renders state and forwards user intent
//! to tagrex-core. All logic lives in the core (see docs/architecture.md).

use tagrex_core::transform::TransformChain;

fn main() {
    // Minimal cross-crate call to prove the workspace wiring works.
    let chain = TransformChain::default();
    let name = chain.apply("tagrex");
    println!(
        "{} {}: core linked, GUI shell not wired up yet — see docs/architecture.md",
        name,
        env!("CARGO_PKG_VERSION")
    );
}
