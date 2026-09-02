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
            resources: [.copy("Fonts")],
            linkerSettings: [
                // The staticlib built by `cargo build -p tagrex-ffi --release`.
                .unsafeFlags(["-L../../target/release", "-ltagrex_ffi"])
            ]
        ),
    ]
)
