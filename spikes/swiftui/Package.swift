// swift-tools-version: 5.9
//
// The SwiftUI stand (#271): a read-only window over the same core the app uses.
// Built by build.sh, which compiles the Rust staticlib first and then assembles
// the .app bundle around this executable.

import PackageDescription

let package = Package(
    name: "TagRexSpike",
    platforms: [.macOS(.v14)],
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
