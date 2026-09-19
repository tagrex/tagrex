// The field catalogue for the dynamic tag editor (E1), mirroring the Tauri
// `fields.js` + `editor.js` constants so the stand groups a file's real tags the
// same way the web editor does: a Core group of the fields a DJ touches every
// session, a Standard group of the known extended fields plus any custom frame
// we can name, and an Advanced group of raw technical frames with no friendly
// name. Storage keys match the command layer's own vocabulary, so an edit staged
// here names its field exactly as `preview_tag_edits` expects it.

import Foundation

enum EditorFields {
    /// The fields the Core group carries, in the web editor's own order. The
    /// track/total and disc/total pairs render as duo rows; the rest are singles.
    static let coreKeys = [
        "artist", "title", "album", "albumartist",
        "track", "tracktotal", "disc", "disctotal", "year", "genre",
    ]

    /// The known extended fields (key → label), in model order — the Standard
    /// group is these minus the Core keys, plus any promoted custom frame.
    static let extended: [(key: String, label: String)] = [
        ("artist", "Artist"),
        ("title", "Title"),
        ("album", "Album"),
        ("albumartist", "Album Artist"),
        ("track", "Track"),
        ("tracktotal", "Track Total"),
        ("disc", "Disc"),
        ("disctotal", "Disc Total"),
        ("year", "Year"),
        ("genre", "Genre"),
        ("comment", "Comment"),
        ("composer", "Composer"),
        ("publisher", "Publisher"),
        ("catalognumber", "Catalogue #"),
        ("bpm", "BPM"),
        ("isrc", "ISRC"),
        ("key", "Key"),
        ("url", "URL"),
        ("media", "Media"),
    ]

    /// Friendly names for known technical/custom frames (#136), keyed by the raw
    /// custom name upper-cased (no `custom:` prefix). A custom key found here is
    /// promoted into the Standard group; anything else stays a raw Advanced row.
    static let knownCustomLabels: [String: String] = [
        "DISCOGS_RELEASE_ID": "Discogs Release ID",
        "MUSICBRAINZ_ALBUMID": "MusicBrainz Album ID",
        "MUSICBRAINZ_TRACKID": "MusicBrainz Track ID",
        "REPLAYGAIN_TRACK_GAIN": "ReplayGain (track)",
        "REPLAYGAIN_TRACK_PEAK": "ReplayGain peak (track)",
        "REPLAYGAIN_ALBUM_GAIN": "ReplayGain (album)",
        "REPLAYGAIN_ALBUM_PEAK": "ReplayGain peak (album)",
        "WWWAUDIOFILE": "Audio file URL",
        "ORIGARTIST": "Original Artist",
        "ORIGALBUM": "Original Album",
        "ORIGYEAR": "Original Year",
        "ENCODEDBY": "Encoded by",
        "CONDUCTOR": "Conductor",
        "LYRICIST": "Lyricist",
        "GROUPING": "Grouping",
        "SUBTITLE": "Subtitle",
        "COPYRIGHT": "Copyright",
        "MOOD": "Mood",
        "LANGUAGE": "Language",
    ]

    /// Numeric/typed fields: a narrower right-aligned input and inline validation,
    /// mirroring the backend's `is_writable_value` rule so a bad value shows as an
    /// error while typing instead of only being rejected at apply.
    static let numericKeys: Set<String> = ["track", "tracktotal", "disc", "disctotal", "bpm", "year"]

    private static let extendedLabelByKey: [String: String] =
        Dictionary(uniqueKeysWithValues: extended.map { ($0.key, $0.label) })

    /// The display name for a storage key: the extended label, else a promoted
    /// custom's friendly name, else the raw custom name, else the key itself.
    static func label(for key: String) -> String {
        if let label = extendedLabelByKey[key] { return label }
        if key.hasPrefix("custom:") {
            let raw = String(key.dropFirst("custom:".count))
            return knownCustomLabels[raw.uppercased()] ?? raw
        }
        return key
    }

    /// Validate a value the way the Tauri editor does. Returns nil when valid, or
    /// a short hint when not — empty is always valid (clearing a field).
    static func validationHint(for key: String, value: String) -> String? {
        let v = value.trimmingCharacters(in: .whitespaces)
        guard !v.isEmpty else { return nil }
        switch key {
        case "year":
            let year = v.split(separator: "-").first.map(String.init) ?? v
            return year.count == 4 && year.allSatisfy(\.isNumber) ? nil : "4-digit year"
        case "bpm":
            return Double(v) != nil ? nil : "numbers only"
        case _ where numericKeys.contains(key):
            return v.allSatisfy(\.isNumber) ? nil : "numbers only"
        default:
            return nil
        }
    }

    /// A group in the editor, built from the selection's real tags.
    struct Group: Identifiable {
        let id: String
        let title: String
        /// Collapsible groups fold; Core is always open.
        let collapsible: Bool
        let rows: [Row]
    }

    /// One editor row: a single field, or a duo (n / total) pair on one line.
    enum Row: Identifiable {
        case single(key: String)
        case duo(label: String, numberKey: String, totalKey: String)

        var id: String {
            switch self {
            case .single(let key): key
            case .duo(_, let numberKey, _): "duo:\(numberKey)"
            }
        }
    }

    /// Build the three groups from the tag keys present across the selection and
    /// any keys already staged (so a custom field added a moment ago still lists).
    /// Mirrors `renderFieldEditor`: Core in a fixed layout, Standard = known
    /// extended (minus Core) + promoted customs, Advanced = the raw rest.
    static func groups(presentKeys: Set<String>) -> [Group] {
        let core = Group(
            id: "core", title: "Core", collapsible: false,
            rows: [
                .duo(label: "Track", numberKey: "track", totalKey: "tracktotal"),
                .duo(label: "Disc", numberKey: "disc", totalKey: "disctotal"),
                .single(key: "artist"),
                .single(key: "title"),
                .single(key: "album"),
                .single(key: "albumartist"),
                .single(key: "year"),
                .single(key: "genre"),
            ]
        )

        var standardRows: [Row] = extended
            .filter { !coreKeys.contains($0.key) }
            .map { .single(key: $0.key) }

        // Custom keys present anywhere in the selection: those we can name are
        // promoted into Standard (sorted by label), the rest fall into Advanced.
        let customs = presentKeys.filter { $0.hasPrefix("custom:") }.sorted()
        var advancedRows: [Row] = []
        var promoted: [(label: String, key: String)] = []
        for key in customs {
            let raw = String(key.dropFirst("custom:".count))
            if knownCustomLabels[raw.uppercased()] != nil {
                promoted.append((label(for: key), key))
            } else {
                advancedRows.append(.single(key: key))
            }
        }
        promoted.sort { $0.label.localizedCaseInsensitiveCompare($1.label) == .orderedAscending }
        standardRows.append(contentsOf: promoted.map { .single(key: $0.key) })

        var groups = [
            core,
            Group(id: "standard", title: "Standard", collapsible: true, rows: standardRows),
        ]
        if !advancedRows.isEmpty {
            groups.append(Group(id: "advanced", title: "Advanced", collapsible: true, rows: advancedRows))
        }
        return groups
    }
}
