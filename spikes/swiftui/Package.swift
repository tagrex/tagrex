// swift-tools-version: 5.9
//
// The SwiftUI stand (#271): a read-only window over the same core the app uses.
// Built by build.sh, which compiles the Rust staticlib first and then assembles
// the .app bundle around this executable.

import PackageDescription

let package = Package(
    name: "TagRexSpike",
    // Tahoe, not v14: the toolbar is built out of ToolbarSpacer and the
    // shared-background grouping it implies, both macOS 26. Guarding them
    // with #available would not help — an older SDK cannot compile them at
    // all — and a stand that exists to be compared against the 26 look has
    // no reason to run anywhere else.
    // The string form, not .v26: that enum case wants tools-version 6.2,
    // and raising the tools version would flip the target into Swift 6
    // language mode as a side effect.
    platforms: [.macOS("26.0")],
    targets: [
        .target(name: "CTagRex"),
        .executableTarget(
            name: "TagRexSpike",
            dependencies: ["CTagRex"],
            linkerSettings: [
                // The staticlib by full path, not -ltagrex_ffi: the crate also
                // builds a cdylib into the same directory, the linker prefers a
                // .dylib to a .a, and the result is a bundle that hunts for an
                // absolute path from whatever machine built it.
                .unsafeFlags(["../../target/release/libtagrex_ffi.a"])
            ]
        ),
    ]
)
