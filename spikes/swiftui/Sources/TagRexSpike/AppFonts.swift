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

    static var body: Font {
        hasBundledFaces ? .custom("IBMPlexSans", size: 12, relativeTo: .body) : .body
    }

    static var mono: Font {
        hasBundledFaces
            ? .custom("JetBrainsMono-Regular", size: 11.5, relativeTo: .body)
            : .system(.body, design: .monospaced)
    }
}
