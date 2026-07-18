//! TagRex application entry point.
//!
//! The application logic a GUI drives lives in the [`tagrex`](crate) library
//! crate's command layer ([`App`]). This binary is a placeholder for the Tauri
//! shell (webview window, frontend, IPC commands), which is initialized as a
//! separate step on a machine with a display:
//!
//! ```text
//! cargo install tauri-cli
//! cargo tauri init
//! ```
//!
//! Each Tauri command would hold an `App` in managed state and forward one
//! call into it. The shell must stay thin: it renders state and forwards user
//! intent; all logic lives in tagrex-core (see docs/architecture.md).

fn main() {
    println!(
        "tagrex {}: core + command layer linked (see the `tagrex` lib crate); \
         GUI shell not wired up yet — see docs/architecture.md",
        env!("CARGO_PKG_VERSION")
    );
}
