// The app's own typefaces, in the one format native toolkits can read.
//
// app/ui/assets ships woff2, which is a web format: CoreText will not load it.
// build.sh fetches the upstream TTFs of the same families — IBM Plex Sans and
// JetBrains Mono, both open-licensed — into Sources/TagRexSpike/Fonts, and this
// registers whatever landed there. With the folder empty the stand still runs,
// on the system faces, so a missing download degrades the comparison rather
// than breaking the build.

import AppKit
import SwiftUI

enum AppFonts {
    private(set) static var hasBundledFaces = false

    static func register() {
        // Bundle.main, not Bundle.module: the generated accessor resolves
        // against the build tree it was compiled in, so a bundle built by CI
        // looked for its resources under /Users/runner and died on launch.
        guard let urls = Bundle.main.urls(forResourcesWithExtension: "ttf", subdirectory: "Fonts"),
              !urls.isEmpty
        else { return }

        for url in urls {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
        hasBundledFaces = true
    }

    // The family names from each TTF's name table — NOT "IBMPlexSans", which is
    // no font's PostScript name (the real one is IBMPlexSans-Regular), so
    // `.custom` was silently falling back to the system face. A family name also
    // lets `.weight` pick a weight off the variable font.
    private static let sansFamily = "IBM Plex Sans"
    private static let monoFamily = "JetBrains Mono"

    static var body: Font {
        hasBundledFaces ? .custom(sansFamily, size: 12, relativeTo: .body) : .body
    }

    static var mono: Font {
        hasBundledFaces
            ? .custom(monoFamily, size: 11.5, relativeTo: .body)
            : .system(.body, design: .monospaced)
    }

    /// IBM Plex Sans at an explicit size and weight, falling back to the system
    /// face — for the release cards, which mix a few sizes.
    static func sans(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        (hasBundledFaces ? Font.custom(sansFamily, size: size) : .system(size: size))
            .weight(weight)
    }

    /// JetBrains Mono at an explicit size.
    static func monoSized(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        (hasBundledFaces ? Font.custom(monoFamily, size: size) : .system(size: size, design: .monospaced))
            .weight(weight)
    }
}
