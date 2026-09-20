// The model side (#271, #293): open a session, list, stage, apply, undo — all
// through the command layer's own path, so a write from here is gated and
// journaled exactly as a write from the app is. The bridge is the session ABI
// in crates/ffi: `tagrex_open` once, then `tagrex_invoke` by command name.

import CTagRex
import CryptoKit
import Foundation

struct Track: Identifiable, Decodable, Hashable {
    var path: String
    var format: String
    /// Storage-key -> value, the way the command layer reports a track. The UI
    /// reads fields out of here, so it shares one vocabulary with the backend.
    var tags: [String: String]
    var unreadable: Bool
    var durationSecs: UInt64?
    /// The tag blocks the file carries, in the order it carries them (#47) — what
    /// the editor's block bar strips and converts.
    var tagBlocks: [TagBlock]

    var id: String { path }

    var file: String { (path as NSString).lastPathComponent }

    var artist: String { value(for: .artist) }
    var title: String { value(for: .title) }
    var album: String { value(for: .album) }
    var albumartist: String { value(for: .albumartist) }
    var year: String { value(for: .year) }
    var genre: String { value(for: .genre) }
    var track: String { value(for: .track) }
    /// The catalogue number tag — an extended field, so read by raw key.
    var catalognumber: String { value(forKey: "catalognumber") }
    // The rest of EXTENDED_FIELDS (`app/ui/js/fields.js`) not already modeled
    // above — plain tag lookups, same as catalognumber, so each can back a
    // sortable table column.
    var tracktotal: String { value(forKey: "tracktotal") }
    var disc: String { value(forKey: "disc") }
    var comment: String { value(forKey: "comment") }
    var composer: String { value(forKey: "composer") }
    var publisher: String { value(forKey: "publisher") }
    var bpm: String { value(forKey: "bpm") }
    var isrc: String { value(forKey: "isrc") }
    var key: String { value(forKey: "key") }
    var url: String { value(forKey: "url") }
    var media: String { value(forKey: "media") }
    /// A sort key for the Length column: playing time in whole seconds.
    var durationSort: Int { Int(durationSecs ?? 0) }

    /// The "ID3v2.4 + ID3v1" summary of which tag blocks a file carries — mirrors
    /// Tauri's `tagBlockSummary`: the block being read from listed first.
    var tagtypes: String {
        let read = tagBlocks.filter(\.readFrom).map(\.label)
        let rest = tagBlocks.filter { !$0.readFrom }.map(\.label)
        return (read + rest).joined(separator: " + ")
    }

    var duration: String {
        guard let secs = durationSecs else { return "" }
        return String(format: "%d:%02d", secs / 60, secs % 60)
    }

    /// The value the core stores under `key`, so the UI and the bridge agree
    /// about field names without a second vocabulary.
    func value(for key: Field) -> String {
        tags[key.rawValue] ?? ""
    }

    /// The value under an arbitrary storage key — what the dynamic tag editor
    /// reads, since it edits the file's real tags, not just the modeled columns.
    func value(forKey key: String) -> String {
        tags[key] ?? ""
    }

    /// The grouping-bucket value under `groupBy` — mirrors Tauri's `groupKeyOf`
    /// (`app/ui/js/grouping.js`): the two fixed groupings read off the path or a
    /// provider id, everything else is a plain tag lookup, so any modeled field
    /// groups the table the same way the built-in ones do.
    func groupKey(by groupBy: String) -> String {
        switch groupBy {
        case "folder":
            return (path as NSString).deletingLastPathComponent
        case "release":
            return tags["custom:MUSICBRAINZ_ALBUMID"] ?? tags["custom:DISCOGS_RELEASE_ID"] ?? ""
        default:
            return tags[groupBy] ?? ""
        }
    }

    // Mapped by hand rather than through a snake-case decoding strategy: that
    // strategy also rewrites dictionary keys, which would mangle the `tags` map
    // and any plan the bridge round-trips back into a later call.
    enum CodingKeys: String, CodingKey {
        case path, format, tags, unreadable
        case durationSecs = "duration_secs"
        case tagBlocks = "tag_blocks"
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        path = try c.decode(String.self, forKey: .path)
        format = try c.decode(String.self, forKey: .format)
        tags = try c.decode([String: String].self, forKey: .tags)
        unreadable = try c.decodeIfPresent(Bool.self, forKey: .unreadable) ?? false
        durationSecs = try c.decodeIfPresent(UInt64.self, forKey: .durationSecs)
        tagBlocks = try c.decodeIfPresent([TagBlock].self, forKey: .tagBlocks) ?? []
    }
}

/// One tag block a file carries (#47): a display label, the storage key of its
/// kind (`id3v1`, `id3v2`, `vorbis`, …), and whether it is the block the app
/// reads from and writes to.
struct TagBlock: Decodable, Hashable {
    var label: String
    var kind: String
    var readFrom: Bool

    enum CodingKeys: String, CodingKey {
        case label, kind
        case readFrom = "read_from"
    }
}

// MARK: - Online search models

/// An online source the panel can search.
enum Source: String, CaseIterable, Identifiable {
    case discogs, musicbrainz, beatport

    var id: String { rawValue }

    var label: String {
        switch self {
        case .discogs: "Discogs"
        case .musicbrainz: "MusicBrainz"
        case .beatport: "Beatport"
        }
    }
}

/// One release candidate from `provider_search`.
struct Candidate: Identifiable, Decodable, Hashable {
    var id: String
    var artist: String
    var title: String
    var year: Int?
    var score: Double
    var country: String?
    var label: String?
    var format: String?
    var catalogNumber: String?
    var thumbURL: String?
    var coverURL: String?

    enum CodingKeys: String, CodingKey {
        case id, artist, title, year, score, country, label, format
        case catalogNumber = "catalog_number"
        case thumbURL = "thumb_url"
        case coverURL = "cover_url"
    }

    /// The image the card shows: the thumbnail if the source gave one, else the
    /// full cover URL. Empty when the source carries no art.
    var imageURL: String? {
        [thumbURL, coverURL].compactMap { $0 }.first { !$0.isEmpty }
    }

    /// "label · CAT 123 · Belgium", the parts that are present.
    var detail: String {
        [label, catalogNumber, country]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }
}

/// One track of a fetched release.
struct ReleaseTrack: Identifiable, Decodable, Hashable {
    var position: String
    var disc: Int?
    var artist: String?
    var title: String
    var durationSecs: Int?
    var isrc: String?
    var bpm: Int?
    var key: String?

    var id: String { "\(disc ?? 0)-\(position)-\(title)" }

    enum CodingKeys: String, CodingKey {
        case position, disc, artist, title, isrc, bpm, key
        case durationSecs = "duration_secs"
    }

    var length: String {
        guard let secs = durationSecs else { return "" }
        return String(format: "%d:%02d", secs / 60, secs % 60)
    }
}

/// A fetched release. Only the parts the panel shows or imports are decoded.
struct Release: Decodable {
    var id: String
    var artist: String
    var title: String
    var year: Int?
    var tracks: [ReleaseTrack]
    var country: String?
    /// Broad genres and specific styles; the import writes the styles to the
    /// genre tag by preference, falling back to the genres.
    var genres: [String]?
    var styles: [String]?
    /// Label / catalogue-number pairs the release lists (#90). A release can carry
    /// several — even from the same label — so the caller picks which one to write
    /// (label → Publisher, catno → CatalogNumber). Listing order; first is primary.
    var labels: [ReleaseLabel]?
    /// Physical/source format descriptor, e.g. `Vinyl, 12", 33 ⅓ RPM` or `CD`
    /// (#106). Drives the media-type tag and the media badge.
    var format: String?
    /// Public webpage for the release (the provider's release page), if any.
    var url: String?
    /// The release cover. From Cover Art Archive for MusicBrainz (public, no
    /// token) and from the provider for Discogs/Beatport (needs their auth).
    var coverImageURL: String?
    /// The number of discs, and the release's images — for the card's counts.
    var discTotal: Int?
    /// Every image the release carries, primary first (#102): each one's URL and
    /// its dimensions when the provider states them (0 = unknown). Used for the
    /// cover resolution/count readout and to save the images to disk.
    var images: [ReleaseImage]?

    enum CodingKeys: String, CodingKey {
        case id, artist, title, year, tracks, country, genres, styles, labels, format, images, url
        case coverImageURL = "cover_image_url"
        case discTotal = "disc_total"
    }

    var trackCount: Int { tracks.count }
    var discCount: Int { max(1, discTotal ?? 1) }
    var imageCount: Int { images?.count ?? 0 }
}

/// One label imprint of a release, with its catalogue number when stated (#90).
struct ReleaseLabel: Decodable, Hashable {
    var name: String
    var catalogNumber: String?

    enum CodingKeys: String, CodingKey {
        case name
        case catalogNumber = "catalog_number"
    }

    /// "Antler-Subway — AS 5606", the parts present, for the picker.
    var label: String {
        catalogNumber.map { "\(name) — \($0)" } ?? name
    }
}

/// One image of a release: a download handle plus its pixel dimensions when the
/// provider states them (0 = unknown). Ordered with the primary first (#102).
struct ReleaseImage: Decodable, Hashable {
    var url: String
    var width: Int
    var height: Int
}

/// The counts a release card shows once its release is prefetched.
struct ReleaseCounts: Equatable {
    let tracks: Int
    let discs: Int
    let images: Int
}

extension Release {

    /// The value the import writes to the genre tag: the styles joined, else the
    /// genres.
    var importGenre: String? {
        let chosen = (styles?.isEmpty == false ? styles : genres) ?? []
        let joined = chosen.joined(separator: "/")
        return joined.isEmpty ? nil : joined
    }

    /// The value written to the media tag on import (#106), mirroring the Tauri
    /// `mediaTagValue`: a clean normalized label for a kind we recognise, else the
    /// provider's own format text, else nothing.
    var mediaTagValue: String? {
        let f = (format ?? "").lowercased()
        func has(_ keys: String...) -> Bool { keys.contains { f.contains($0) } }
        let label: String?
        if has("cassette", "tape") {
            label = "Cassette"
        } else if has("vinyl", "lp", "ep", "7\"", "10\"", "12\"", "shellac") {
            label = "Vinyl"
        } else if has("sacd", "hdcd", "cdr", "compact disc", "cd") {
            label = "CD"
        } else if has("file", "flac", "mp3", "wav", "aac", "digital", "download", "streaming") {
            label = "File"
        } else {
            label = nil
        }
        return label ?? blankToNil(format ?? "")
    }
}

/// A search or fetch failure, carrying the message to show in the panel.
/// `Result`'s failure type must be an `Error`, and a bare `String` is not one.
struct SearchFailure: Error {
    let message: String
}

/// The settings the stand exposes: the online credentials/throttle and the ID3
/// write revision. A subset of the backend `SettingsDto`.
struct OnlineSettings: Equatable {
    var discogsToken = ""
    var proxy = ""
    var rateLimitPerMin = 0
    var id3v23 = false
}

/// One line of a rename preview: the file's current name and what the mask
/// renames it to.
struct RenamePair: Identifiable, Hashable {
    var old: String
    var new: String
    var id: String { old }
}

/// One file in a duplicate group.
struct DuplicateFile: Decodable, Hashable, Identifiable {
    var path: String
    var artist: String
    var title: String
    var album: String
    var durationSecs: UInt64
    var sizeBytes: UInt64
    var bitrateKbps: UInt32?

    var id: String { path }
    var file: String { (path as NSString).lastPathComponent }

    enum CodingKeys: String, CodingKey {
        case path, artist, title, album
        case durationSecs = "duration_secs"
        case sizeBytes = "size_bytes"
        case bitrateKbps = "bitrate_kbps"
    }

    var duration: String {
        String(format: "%d:%02d", durationSecs / 60, durationSecs % 60)
    }

    var size: String {
        let mb = Double(sizeBytes) / 1_048_576
        return String(format: "%.1f MB", mb)
    }
}

/// A set of files judged duplicates of each other under the chosen criterion.
struct DuplicateGroup: Decodable, Identifiable {
    var key: String
    var files: [DuplicateFile]
    var id: String { key + (files.first?.path ?? "") }
}

/// A live probe of one file's name through a mask: the string matched, the
/// fields captured, and whether the mask matched at all.
struct NameProbe: Decodable {
    var subject: String
    /// (field key, captured value) pairs, serialized as 2-element arrays.
    var fields: [[String]]
    var matched: Bool

    var pairs: [(field: String, value: String)] {
        fields.compactMap { $0.count == 2 ? ($0[0], $0[1]) : nil }
    }
}

/// One transform rule, as the backend's TransformRuleDto. snake_case keys are
/// the property names, since the ABI does not convert them.
struct TransformRule: Codable {
    var kind: String
    var from = ""
    var to = ""
    var regex = false
    var whole_word = false
    var case_sensitive = false
    var style = ""
    var enabled = true
    /// What this step acts on, overriding the group's scope (#250): a field key,
    /// `tags`, `filename`/`fileext`, or nil to follow the group.
    var scope: String?

    init(kind: String, from: String = "", to: String = "", regex: Bool = false,
         whole_word: Bool = false, case_sensitive: Bool = false, style: String = "",
         enabled: Bool = true, scope: String? = nil) {
        self.kind = kind
        self.from = from
        self.to = to
        self.regex = regex
        self.whole_word = whole_word
        self.case_sensitive = case_sensitive
        self.style = style
        self.enabled = enabled
        self.scope = scope
    }

    // Manual decode so a builtin group missing an optional field still loads
    // (synthesized Decodable ignores the property defaults above).
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = try c.decode(String.self, forKey: .kind)
        from = try c.decodeIfPresent(String.self, forKey: .from) ?? ""
        to = try c.decodeIfPresent(String.self, forKey: .to) ?? ""
        regex = try c.decodeIfPresent(Bool.self, forKey: .regex) ?? false
        whole_word = try c.decodeIfPresent(Bool.self, forKey: .whole_word) ?? false
        case_sensitive = try c.decodeIfPresent(Bool.self, forKey: .case_sensitive) ?? false
        style = try c.decodeIfPresent(String.self, forKey: .style) ?? ""
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
        scope = try c.decodeIfPresent(String.self, forKey: .scope)
    }
}

/// A named chain of transform steps run as one previewable batch (#57): the
/// group's default scope, its rules (each optionally overriding that scope), and
/// a one-line note the shipped presets carry.
struct ActionGroup: Codable, Identifiable {
    var name: String
    var scope: String
    var rules: [TransformRule]
    var note: String = ""

    var id: String { name }

    init(name: String, scope: String, rules: [TransformRule], note: String = "") {
        self.name = name
        self.scope = scope
        self.rules = rules
        self.note = note
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        scope = try c.decodeIfPresent(String.self, forKey: .scope) ?? "tags"
        rules = try c.decode([TransformRule].self, forKey: .rules)
        note = try c.decodeIfPresent(String.self, forKey: .note) ?? ""
    }
}

/// One line of a transform preview: what changed (a field name or "file"), its
/// old value and the new one.
struct TransformPair: Identifiable, Hashable {
    var label: String
    var old: String
    var new: String
    var id: String { "\(label)|\(old)|\(new)" }
}

/// The editable fields, named with the core's storage keys.
enum Field: String, CaseIterable, Identifiable {
    case artist, title, album, albumartist, year, genre, track

    var id: String { rawValue }

    var label: String {
        switch self {
        case .artist: "Artist"
        case .title: "Title"
        case .album: "Album"
        case .albumartist: "Album artist"
        case .year: "Year"
        case .genre: "Genre"
        case .track: "Track"
        }
    }
}

/// The `{"ok":…}` / `{"error":…}` envelope every ABI call answers with.
private struct Reply<T: Decodable>: Decodable {
    var ok: T?
    var error: ErrorReply?
}

private struct ErrorReply: Decodable {
    var text: String
}

/// A batch as `history` and `apply_plan` report it — only the id is needed here,
/// to undo it and to word the message.
private struct Batch: Decodable {
    var id: Int
    var description: String
}

/// The session pointer, boxed so it can cross into a detached task. Access is
/// serialized by `isBusy` and the `await` on each call — one call at a time —
/// which is the contract the ABI asks for.
private struct SessionHandle: @unchecked Sendable {
    let raw: OpaquePointer
}

@Observable
@MainActor
final class Library {
    private(set) var root: URL?
    private(set) var tracks: [Track] = []
    private(set) var errors: [String] = []
    private(set) var isBusy = false
    private(set) var lastMessage = ""

    /// Post a status-bar note from a panel — the outcome of an action that stages
    /// nothing (a saved image, say), where `lastMessage` is otherwise `private`.
    func note(_ message: String) { lastMessage = message }

    /// Release-cover bytes already fetched, keyed by image URL, so re-rendering a
    /// results row never re-hits the provider. Mirrors `imageCache` in the Tauri
    /// online.js.
    private var imageCache: [String: Data] = [:]
    /// Full releases already fetched, keyed by "source/id", so the card-count
    /// prefetch and opening a release share one fetch.
    private var releaseCache: [String: Release] = [:]

    /// The staged edit map: path → storage-key → new value. Nothing is on disk
    /// until Apply. It drives the table diff for both a hand edit and a staged
    /// import. Keyed by the raw storage key (not the modeled `Field` enum) so the
    /// dynamic editor can stage any tag the file carries, not only the columns.
    private(set) var staged: [String: [String: String]] = [:]

    /// A whole staged plan from an online import (#300). When set, Apply writes
    /// this plan rather than rebuilding one from `staged` — the plan carries more
    /// than the table's seven columns (isrc, bpm, catalogue, …), and `staged`
    /// only mirrors the visible part of it for the diff.
    private var stagedPlan: JSONValue?
    private var stagedPlanCount = 0

    /// path → the new file name a staged rename gives it, so the File column can
    /// show the rename as a diff the way a tag change shows in its column.
    private(set) var stagedRenames: [String: String] = [:]

    /// Storage keys the user has locked (#63): every plan the backend builds
    /// skips them, so a locked field is protected from imports, transforms and
    /// hand edits alike. Session-wide, pushed with `set_locked_fields`.
    private(set) var lockedFields: Set<String> = []

    var filter = ""
    /// Filter flags (#44): match the query as a regular expression, and/or match
    /// case-sensitively. Off = a case-insensitive substring, as before.
    var filterRegex = false
    var filterCaseSensitive = false
    var showsOldValues = false

    /// Whether the current regex filter fails to compile — the field reddens and
    /// the filter is not applied (the table shows everything), so a half-typed
    /// pattern like `(` isn't destructive.
    var filterInvalid: Bool {
        guard filterRegex else { return false }
        let query = filterParts().query
        guard !query.isEmpty else { return false }
        return regex(for: query) == nil
    }

    var rootName: String { root?.lastPathComponent ?? "No folder open" }
    var stagedFileCount: Int { stagedPlan != nil ? stagedPlanCount : staged.count }
    var hasStagedPlan: Bool { stagedPlan != nil || !staged.isEmpty }

    /// The live session. Held across calls, closed when another folder opens.
    /// Not closed on deinit — a `Library` lives for the window's lifetime, and
    /// deinit cannot reach a main-actor property to close it; the process exit
    /// reclaims the last one.
    private var session: OpaquePointer?

    // MARK: - Reading

    var visibleTracks: [Track] {
        let parts = filterParts()
        guard !parts.query.isEmpty else { return tracks }
        // A bad regex isn't applied — show everything and let the field redden,
        // so typing a partial pattern doesn't wipe the table.
        guard let matches = matcher(for: parts.query) else { return tracks }

        return tracks.filter { track in
            if let field = parts.field {
                return matches(track.value(for: field))
            }
            return [track.file, track.artist, track.title, track.album].contains(where: matches)
        }
    }

    /// Split the filter into an optional `field:` scope and the query text — from
    /// the raw filter (not lower-cased), so a case-sensitive match sees it whole.
    private func filterParts() -> (field: Field?, query: String) {
        let raw = filter.trimmingCharacters(in: .whitespaces)
        if let colon = raw.firstIndex(of: ":") {
            let name = String(raw[raw.startIndex..<colon]).lowercased()
            let value = String(raw[raw.index(after: colon)...])
            if !value.isEmpty, let field = Field(rawValue: name) {
                return (field, value)
            }
        }
        return (nil, raw)
    }

    /// A compiled regex for `query` under the current case flag, or nil if it
    /// doesn't compile.
    private func regex(for query: String) -> NSRegularExpression? {
        try? NSRegularExpression(
            pattern: query, options: filterCaseSensitive ? [] : [.caseInsensitive])
    }

    /// The match predicate for the current flags: a regex search, or a substring
    /// (case-sensitive or not). Nil only when a regex query fails to compile.
    private func matcher(for query: String) -> ((String) -> Bool)? {
        if filterRegex {
            guard let re = regex(for: query) else { return nil }
            return { text in
                re.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) != nil
            }
        }
        if filterCaseSensitive {
            return { $0.contains(query) }
        }
        let lowered = query.lowercased()
        return { $0.lowercased().contains(lowered) }
    }

    func open(_ folder: URL) async {
        if let session { tagrex_close(session) }
        session = nil
        root = folder
        staged.removeAll()

        var handle: OpaquePointer?
        let opened = folder.path.withCString { rootPtr -> Bool in
            Self.configDir(for: folder).withCString { cfgPtr -> Bool in
                guard let raw = tagrex_open(rootPtr, cfgPtr, &handle) else { return false }
                defer { tagrex_string_free(raw) }
                let reply: Reply<EmptyOk>? = decode(Reply<EmptyOk>.self, from: raw)
                return reply?.error == nil
            }
        }

        if opened, handle != nil {
            session = handle
            lockedFields = []
            await rescan()
            await loadLockedFields()
            _ = await loadOnlineSettings()
        } else {
            tracks = []
            errors = ["the library could not be opened"]
        }
    }

    func rescan() async {
        guard let session else { return }
        isBusy = true
        defer { isBusy = false }

        let box = SessionHandle(raw: session)
        let reply: Reply<[Track]>? = await Task.detached(priority: .userInitiated) {
            invoke(box, "list_tracks", "{}")
        }.value

        tracks = reply?.ok ?? []
        errors = reply?.error.map { [$0.text] } ?? []
    }

    // MARK: - Staging

    /// Stage one field across a selection. An empty string clears the tag; a
    /// value equal to what the file already holds stages nothing, so typing a
    /// value back to what it was cancels the change instead of recording a
    /// no-op the way the web editor does.
    func stage(_ key: String, to value: String, for ids: [Track.ID]) {
        // A hand edit supersedes a pending import or rename: the staging sources
        // must not mix, and Apply follows whichever is current.
        stagedPlan = nil
        stagedPlanCount = 0
        stagedRenames.removeAll()
        for id in ids {
            guard let track = tracks.first(where: { $0.id == id }) else { continue }

            if track.value(forKey: key) == value {
                staged[id]?.removeValue(forKey: key)
            } else {
                staged[id, default: [:]][key] = value
            }
            if staged[id]?.isEmpty == true {
                staged.removeValue(forKey: id)
            }
        }
    }

    /// The staged value for a cell, or nil when the cell is unchanged.
    func stagedValue(_ key: String, for id: Track.ID) -> String? {
        staged[id]?[key]
    }

    // MARK: - Field locks (#63)

    func isLocked(_ key: String) -> Bool { lockedFields.contains(key) }

    /// Read back what the session holds — called after opening a library.
    func loadLockedFields() async {
        guard let session else { lockedFields = []; return }
        let box = SessionHandle(raw: session)
        let fields: [String] = await Task.detached(priority: .userInitiated) {
            let reply: Reply<[String]>? = invoke(box, "locked_fields", "{}")
            return reply?.ok ?? []
        }.value
        lockedFields = Set(fields)
    }

    /// Toggle a set of keys as a unit (a duo like track/tracktotal locks whole),
    /// then push the new set to the session so its plan gate honours it.
    func toggleLock(_ keys: [String]) async {
        guard let session else { return }
        let turningOn = !keys.allSatisfy(lockedFields.contains)
        if turningOn { keys.forEach { lockedFields.insert($0) } }
        else { keys.forEach { lockedFields.remove($0) } }
        let box = SessionHandle(raw: session)
        let fields = Array(lockedFields)
        await Task.detached(priority: .userInitiated) {
            _ = invoke(box, "set_locked_fields", encodeArgs(LockedFieldsArg(fields: fields))) as Reply<JSONValue>?
        }.value
    }

    func discard() {
        staged.removeAll()
        stagedPlan = nil
        stagedPlanCount = 0
        stagedRenames.removeAll()
        lastMessage = "Discarded"
    }

    // MARK: - Writing

    /// Apply the staged plan. A staged import already has one; a hand edit is
    /// turned into one first (preview_tag_edits). Either way the write goes
    /// through apply_plan — one journaled, undoable batch.
    func apply() async {
        guard let session, hasStagedPlan else { return }
        if let plan = stagedPlan {
            await applyStagedPlan(plan, count: stagedPlanCount)
            return
        }
        isBusy = true
        defer { isBusy = false }

        let edits: [[String: String]] = staged.flatMap { path, fields in
            fields.map { field, value in
                ["path": path, "field": field, "value": value]
            }
        }
        let count = staged.count

        let box = SessionHandle(raw: session)
        let message: String = await Task.detached(priority: .userInitiated) {
            let planReply: Reply<JSONValue>? =
                invoke(box, "preview_tag_edits", encodeArgs(EditsArg(edits: edits)))
            guard let plan = planReply?.ok else {
                return planReply?.error?.text ?? "the write could not be prepared"
            }

            let applied: Reply<Batch>? =
                invoke(box, "apply_plan", encodeArgs(PlanArg(plan: plan)))
            if applied?.ok != nil {
                return "Applied to \(count) file(s)"
            }
            return applied?.error?.text ?? "the write failed"
        }.value

        lastMessage = message
        if message.hasPrefix("Applied") { staged.removeAll() }
        await rescan()
    }

    /// Apply a whole staged plan (from an online import) — one journaled batch,
    /// undoable like any other.
    private func applyStagedPlan(_ plan: JSONValue, count: Int) async {
        guard let session else { return }
        isBusy = true
        defer { isBusy = false }

        let box = SessionHandle(raw: session)
        let message: String = await Task.detached(priority: .userInitiated) {
            let applied: Reply<Batch>? = invoke(box, "apply_plan", encodeArgs(PlanArg(plan: plan)))
            if applied?.ok != nil { return "Applied to \(count) file(s)" }
            return applied?.error?.text ?? "the write failed"
        }.value

        lastMessage = message
        if message.hasPrefix("Applied") {
            staged.removeAll()
            stagedPlan = nil
            stagedPlanCount = 0
            stagedRenames.removeAll()
        }
        await rescan()
    }

    func undo() async {
        guard let session else { return }
        isBusy = true
        defer { isBusy = false }

        let box = SessionHandle(raw: session)
        let message: String = await Task.detached(priority: .userInitiated) {
            let history: Reply<[Batch]>? = invoke(box, "history", "{}")
            guard let newest = history?.ok?.first else {
                return history?.error?.text ?? "Nothing to undo"
            }

            let undone: Reply<EmptyOk>? =
                invoke(box, "undo", encodeArgs(UndoArg(batchId: newest.id)))
            if undone?.error == nil {
                return "Undone: \(newest.description)"
            }
            return undone?.error?.text ?? "the undo failed"
        }.value

        lastMessage = message
        await rescan()
    }

    // MARK: - Online search

    /// Search a source. Returns the candidates, or a message to show in place of
    /// them — a provider's own error (a missing Discogs token, no Beatport
    /// sign-in) rather than a silent empty list.
    /// One free-text query, sent as the album term the way the Tauri panel does
    /// (`query: { album }` in online.js) — the provider treats it as the general
    /// search string. `format` filters by media (empty = all); `page`/`perPage`
    /// page the results.
    func search(
        _ source: Source,
        query text: String,
        format: String,
        page: Int,
        perPage: Int
    ) async -> Result<[Candidate], SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        let box = SessionHandle(raw: session)
        let query = SearchArgs.Query(
            artist: nil,
            album: blankToNil(text),
            catalog_number: nil,
            format: blankToNil(format),
            page: page,
            per_page: perPage
        )
        return await Task.detached(priority: .userInitiated) {
            let token: String
            switch resolveToken(box, source) {
            case .success(let resolved): token = resolved
            case .failure(let failure): return .failure(failure)
            }
            let args = SearchArgs(source: source.rawValue, token: token, query: query)
            let reply: Reply<[Candidate]>? = invoke(box, "provider_search", encodeArgs(args))
            if let candidates = reply?.ok { return .success(candidates) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "the search failed"))
        }.value
    }

    /// Fetch a release's full tracklist, cached by "source/id".
    func fetchRelease(_ source: Source, id: String) async -> Result<Release, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        if let cached = releaseCache["\(source.rawValue)/\(id)"] { return .success(cached) }
        let box = SessionHandle(raw: session)
        let result = await Task.detached(priority: .userInitiated) { () -> Result<Release, SearchFailure> in
            let token: String
            switch resolveToken(box, source) {
            case .success(let resolved): token = resolved
            case .failure(let failure): return .failure(failure)
            }
            let args = FetchArgs(source: source.rawValue, token: token, release_id: id)
            let reply: Reply<Release>? = invoke(box, "provider_fetch_release", encodeArgs(args))
            if let release = reply?.ok { return .success(release) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "could not load the release"))
        }.value
        if case .success(let release) = result { releaseCache["\(source.rawValue)/\(id)"] = release }
        return result
    }

    /// The card counts for a candidate — track / disc / image — fetched (and
    /// cached) through `fetchRelease`. Nil on any failure, leaving the card
    /// without counts, the way the Tauri prefetch does.
    func releaseCounts(_ source: Source, id: String) async -> ReleaseCounts? {
        if case .success(let release) = await fetchRelease(source, id: id) {
            return ReleaseCounts(
                tracks: release.trackCount,
                discs: release.discCount,
                images: release.imageCount
            )
        }
        return nil
    }

    /// Fetch a release cover's bytes over `provider_fetch_image`, cached by URL so
    /// a re-render never re-hits the provider. Returns nil — leaving the row's
    /// placeholder — on any failure, the way the Tauri card does.
    func fetchImage(_ source: Source, url: String) async -> Data? {
        guard session != nil, !url.isEmpty else { return nil }
        if let cached = imageCache[url] { return cached }
        let box = SessionHandle(raw: session!)
        let data = await Task.detached(priority: .utility) { () -> Data? in
            let token: String
            switch resolveToken(box, source) {
            case .success(let resolved): token = resolved
            case .failure: return nil
            }
            let args = FetchImageArgs(source: source.rawValue, token: token, url: url)
            let reply: Reply<ProviderImage>? = invoke(box, "provider_fetch_image", encodeArgs(args))
            guard let image = reply?.ok else { return nil }
            return Data(base64Encoded: image.data_base64)
        }.value
        if let data { imageCache[url] = data }
        return data
    }

    // MARK: - Settings

    /// The exposed settings last loaded/saved, cached so a save that only changes
    /// the saved groups still writes the current proxy/rate/revision back.
    private var currentOnline = OnlineSettings()
    /// The user's saved rule chains (#57), persisted in the stand's settings.json.
    private(set) var savedActionGroups: [ActionGroup] = []

    /// Load the settings the stand exposes plus the saved Discogs token.
    func loadOnlineSettings() async -> OnlineSettings {
        guard let session else { return OnlineSettings() }
        let box = SessionHandle(raw: session)
        let result: (OnlineSettings, [ActionGroup]) = await Task.detached(priority: .userInitiated) {
            var settings = OnlineSettings()
            var groups: [ActionGroup] = []
            let loaded: Reply<LoadedSettings>? = invoke(box, "load_settings", "{}")
            if let ok = loaded?.ok {
                settings.proxy = ok.proxy ?? ""
                settings.rateLimitPerMin = ok.rate_limit_per_min ?? 0
                settings.id3v23 = ok.id3_v23 ?? false
                groups = ok.action_groups ?? []
            }
            let token: Reply<String>? = invoke(box, "saved_discogs_token", "{}")
            settings.discogsToken = token?.ok ?? ""
            return (settings, groups)
        }.value
        currentOnline = result.0
        savedActionGroups = result.1
        return result.0
    }

    /// Save the exposed settings and the Discogs token. Applied live in the
    /// session, so a new token or proxy takes effect on the next search without
    /// reopening. The saved groups ride along so a settings save never drops them.
    func saveOnlineSettings(_ settings: OnlineSettings) async {
        currentOnline = settings
        await persistSettings()
        guard let session else { return }
        let box = SessionHandle(raw: session)
        await Task.detached(priority: .userInitiated) {
            let token = SaveTokenArgs(token: settings.discogsToken.trimmingCharacters(in: .whitespaces))
            _ = invoke(box, "save_discogs_token", encodeArgs(token)) as Reply<EmptyOk>?
        }.value
    }

    /// Save the current chain as a named group, replacing one of the same name.
    func saveActionGroup(_ group: ActionGroup) async {
        savedActionGroups.removeAll { $0.name == group.name }
        savedActionGroups.append(group)
        await persistSettings()
    }

    func deleteActionGroup(named name: String) async {
        savedActionGroups.removeAll { $0.name == name }
        await persistSettings()
    }

    /// Write the exposed settings + saved groups back as the whole SettingsDto.
    private func persistSettings() async {
        guard let session else { return }
        let box = SessionHandle(raw: session)
        let payload = SaveSettingsArgs(settings: .init(
            proxy: currentOnline.proxy.trimmingCharacters(in: .whitespaces),
            rate_limit_per_min: currentOnline.rateLimitPerMin,
            id3_v23: currentOnline.id3v23,
            action_groups: savedActionGroups
        ))
        await Task.detached(priority: .userInitiated) {
            _ = invoke(box, "save_settings", encodeArgs(payload)) as Reply<EmptyOk>?
        }.value
    }

    /// Align a release's tracks to `paths`. Returns, per file in order, the index
    /// of the release track it matched — or nil when nothing matched.
    func alignRelease(paths: [String], release: Release) async -> Result<[Int?], SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        let box = SessionHandle(raw: session)
        let tracks = release.tracks.map { importTrack(from: $0, albumArtist: release.artist) }
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<[AlignMatch?]>? =
                invoke(box, "auto_align", encodeArgs(AlignArgs(paths: paths, tracks: tracks)))
            if let matches = reply?.ok { return .success(matches.map { $0?.track }) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "alignment failed"))
        }.value
    }

    /// Build the import plan for `paths` from `release`, aligned track per file,
    /// and stage it — the table shows the visible changes and the change-plan bar
    /// takes over, so Apply writes it exactly as a hand edit is written. Every
    /// file must have a matched track; the caller enables this only then.
    func stageImport(
        paths: [String],
        release: Release,
        source: Source,
        alignment: [Int?],
        labelIndex: Int = 0,
        vinylSidesToDisc: Bool = false
    ) async -> Result<Int, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }

        var ordered: [ImportTrack] = []
        for match in alignment {
            guard let index = match, release.tracks.indices.contains(index) else {
                return .failure(SearchFailure(message: "every file must be matched to a track"))
            }
            ordered.append(importTrack(from: release.tracks[index], albumArtist: release.artist))
        }

        // The chosen label / catalogue-number pair (#90): the picker's selection,
        // or the first pair when there is no picker (0 or 1 label).
        let labels = release.labels ?? []
        let chosen = labels.indices.contains(labelIndex) ? labels[labelIndex] : labels.first
        let selection = ImportSelection(
            album: blankToNil(release.title),
            album_artist: blankToNil(release.artist),
            year: release.year.map(String.init),
            genre: release.importGenre,
            tracks: ordered,
            release_id: blankToNil(release.id),
            source: source.rawValue,
            label: chosen.flatMap { blankToNil($0.name) },
            catalog_number: chosen?.catalogNumber.flatMap(blankToNil),
            country: release.country.flatMap(blankToNil),
            track_total: release.tracks.isEmpty ? nil : String(release.tracks.count),
            disc_total: release.discTotal.map(String.init),
            url: release.url.flatMap(blankToNil),
            media_type: release.mediaTagValue
        )
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: [String: String]], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let args = ImportArgs(
                    paths: paths, selection: selection, vinyl_sides_to_disc: vinylSidesToDisc)
                let reply: Reply<JSONValue>? = invoke(box, "preview_import", encodeArgs(args))
                guard let plan = reply?.ok else {
                    return .failure(SearchFailure(
                        message: reply?.error?.text ?? "the import could not be prepared"))
                }
                guard let data = try? JSONEncoder().encode(plan),
                      let parsed = try? JSONDecoder().decode(StagedPlanShape.self, from: data)
                else {
                    return .failure(SearchFailure(message: "could not read the import plan"))
                }
                var diffs: [String: [String: String]] = [:]
                for change in parsed.changes {
                    for tagChange in change.tag_changes {
                        diffs[change.path, default: [:]][tagChange.field] = tagChange.new ?? ""
                    }
                }
                return .success((plan, diffs, parsed.changes.count))
            }.value

        switch result {
        case .success(let (plan, diffs, count)):
            staged = diffs
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged an import of \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            return .failure(failure)
        }
    }

    /// Fetch the release's full-resolution cover and stage a plan that embeds it
    /// into `paths` (`preview_cover_embed`, the Tauri "Embed cover" button, #207).
    /// The cover has no table column, so nothing per-cell is shown — the plan is
    /// staged and the change-plan bar's Apply writes it, one journaled batch. The
    /// count is the files the cover actually changes (already-matching files are
    /// left out by the backend).
    func embedCover(paths: [String], coverURL: String?, source: Source) async
        -> Result<Int, SearchFailure>
    {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        guard let coverURL, !coverURL.isEmpty else {
            return .failure(SearchFailure(message: "This release carries no cover to embed"))
        }
        guard !paths.isEmpty else {
            return .failure(SearchFailure(message: "Select files in the table to embed the cover into"))
        }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let token: String
                switch resolveToken(box, source) {
                case .success(let resolved): token = resolved
                case .failure(let failure): return .failure(failure)
                }
                let fetchArgs = FetchImageArgs(source: source.rawValue, token: token, url: coverURL)
                let image: Reply<ProviderImage>? = invoke(box, "provider_fetch_image", encodeArgs(fetchArgs))
                guard let cover = image?.ok else {
                    return .failure(SearchFailure(
                        message: image?.error?.text ?? "could not fetch the cover"))
                }
                let args = CoverEmbedArgs(
                    paths: paths,
                    cover: CoverArtArg(mime: cover.mime, data_base64: cover.data_base64)
                )
                let reply: Reply<JSONValue>? = invoke(box, "preview_cover_embed", encodeArgs(args))
                guard let plan = reply?.ok else {
                    return .failure(SearchFailure(
                        message: reply?.error?.text ?? "the cover embed could not be prepared"))
                }
                guard let parsed = decodePlan(plan) else {
                    return .failure(SearchFailure(message: "could not read the cover plan"))
                }
                return .success((plan, parsed.changes.count))
            }.value

        switch result {
        case .success(let (plan, count)):
            guard count > 0 else {
                lastMessage = "Selected files already have this cover"
                return .success(0)
            }
            staged.removeAll()
            stagedRenames.removeAll()
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged a cover embed for \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            lastMessage = failure.message
            return .failure(failure)
        }
    }

    /// Save a release's image(s) to disk next to `path` (#102). `all` saves every
    /// image (primary → folder.jpg, then cover.jpg, cover-1.jpg…); otherwise just
    /// the primary. Reports the files it would overwrite in `conflicts` when
    /// `overwrite` is false, so the caller can confirm before a second call.
    func saveReleaseImages(
        source: Source,
        path: String,
        urls: [String],
        overwrite: Bool
    ) async -> Result<SaveImagesResult, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        guard !urls.isEmpty else {
            return .failure(SearchFailure(message: "This release carries no images to save"))
        }
        let box = SessionHandle(raw: session)
        let result: Result<SaveImagesResult, SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let token: String
                switch resolveToken(box, source) {
                case .success(let resolved): token = resolved
                case .failure(let failure): return .failure(failure)
                }
                let args = SaveImagesArgs(
                    source: source.rawValue, token: token, path: path, urls: urls, overwrite: overwrite
                )
                let reply: Reply<SaveImagesResult>? = invoke(box, "save_release_images", encodeArgs(args))
                if let result = reply?.ok { return .success(result) }
                return .failure(SearchFailure(message: reply?.error?.text ?? "could not save the images"))
            }.value
        if case .failure(let failure) = result { lastMessage = failure.message }
        return result
    }

    // MARK: - Renamer

    /// Preview a rename mask over `paths`: old file name → new file name, for the
    /// files the mask actually changes. Read-only; nothing is staged.
    func renamePreview(mask: String, paths: [String]) async -> Result<[RenamePair], SearchFailure> {
        guard let session, !mask.isEmpty, !paths.isEmpty else { return .success([]) }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<JSONValue>? =
                invoke(box, "preview_rename", encodeArgs(MaskPathsArg(mask: mask, paths: paths)))
            guard let plan = reply?.ok else {
                return .failure(SearchFailure(
                    message: reply?.error?.text ?? "the rename could not be previewed"))
            }
            guard let parsed = decodePlan(plan) else {
                return .failure(SearchFailure(message: "could not read the rename plan"))
            }
            let pairs = parsed.changes.compactMap { change -> RenamePair? in
                guard let to = change.rename_to else { return nil }
                return RenamePair(old: baseName(change.path), new: baseName(to))
            }
            return .success(pairs)
        }.value
    }

    /// Build the rename plan and stage it: the File column shows each new name,
    /// the change-plan bar takes over, and Apply writes it — one journaled batch.
    func stageRename(mask: String, paths: [String]) async -> Result<Int, SearchFailure> {
        guard let session, !mask.isEmpty, !paths.isEmpty else { return .success(0) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: String], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let reply: Reply<JSONValue>? =
                    invoke(box, "preview_rename", encodeArgs(MaskPathsArg(mask: mask, paths: paths)))
                guard let plan = reply?.ok else {
                    return .failure(SearchFailure(
                        message: reply?.error?.text ?? "the rename could not be prepared"))
                }
                guard let parsed = decodePlan(plan) else {
                    return .failure(SearchFailure(message: "could not read the rename plan"))
                }
                var renames: [String: String] = [:]
                for change in parsed.changes {
                    if let to = change.rename_to { renames[change.path] = baseName(to) }
                }
                return .success((plan, renames, renames.count))
            }.value

        switch result {
        case .success(let (plan, renames, count)):
            staged.removeAll()
            stagedRenames = renames
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged a rename of \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            return .failure(failure)
        }
    }

    /// Preview reorganising files into folders (#37): the mask is a folder
    /// pattern (`%albumartist%/%album%/%track% - %title%`), each file's new home
    /// shown relative to the destination (or the library root). Read-only.
    func movePreview(
        mask: String, paths: [String], destination: String?, copy: Bool
    ) async -> Result<[RenamePair], SearchFailure> {
        guard let session, !mask.isEmpty, !paths.isEmpty else { return .success([]) }
        let box = SessionHandle(raw: session)
        let base = destination ?? root?.path
        return await Task.detached(priority: .userInitiated) {
            let args = MoveArg(mask: mask, paths: paths, destination: destination,
                               copy: copy, prune_empty_dirs: false)
            let reply: Reply<JSONValue>? = invoke(box, "preview_move", encodeArgs(args))
            guard let plan = reply?.ok else {
                return .failure(SearchFailure(message: reply?.error?.text ?? "the move could not be previewed"))
            }
            guard let parsed = decodePlan(plan) else {
                return .failure(SearchFailure(message: "could not read the move plan"))
            }
            let pairs = parsed.changes.compactMap { change -> RenamePair? in
                guard let to = change.rename_to else { return nil }
                return RenamePair(old: baseName(change.path), new: relativePath(to, under: base))
            }
            return .success(pairs)
        }.value
    }

    /// Build the move plan and stage it: the File column shows each new name, the
    /// change bar applies it — one journaled batch, undoable like any other.
    func stageMove(
        mask: String, paths: [String], destination: String?, copy: Bool, prune: Bool
    ) async -> Result<Int, SearchFailure> {
        guard let session, !mask.isEmpty, !paths.isEmpty else { return .success(0) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: String], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let args = MoveArg(mask: mask, paths: paths, destination: destination,
                                   copy: copy, prune_empty_dirs: copy ? false : prune)
                let reply: Reply<JSONValue>? = invoke(box, "preview_move", encodeArgs(args))
                guard let plan = reply?.ok else {
                    return .failure(SearchFailure(message: reply?.error?.text ?? "the move could not be prepared"))
                }
                guard let parsed = decodePlan(plan) else {
                    return .failure(SearchFailure(message: "could not read the move plan"))
                }
                var renames: [String: String] = [:]
                for change in parsed.changes {
                    if let to = change.rename_to { renames[change.path] = baseName(to) }
                }
                return .success((plan, renames, renames.count))
            }.value

        switch result {
        case .success(let (plan, renames, count)):
            staged.removeAll()
            stagedRenames = renames
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged a \(copy ? "copy" : "move") of \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            return .failure(failure)
        }
    }

    // MARK: - Tags from name

    /// Probe one file's name through a mask: what the mask captures. Read-only.
    func probeFromName(mask: String, path: String) async -> Result<NameProbe, SearchFailure> {
        guard let session, !mask.isEmpty, !path.isEmpty else {
            return .failure(SearchFailure(message: "Pick a file and a mask"))
        }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<NameProbe>? =
                invoke(box, "probe_tags_from_name", encodeArgs(ProbeArg(mask: mask, path: path)))
            if let probe = reply?.ok { return .success(probe) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "the probe failed"))
        }.value
    }

    /// Build the tags-from-name plan and stage it: captured values fill the table
    /// diff, the change-plan bar takes over, Apply writes one journaled batch. When
    /// `groups` is non-empty the captured values run through that chain before
    /// staging (#F1, `preview_transform_over_plan`) — e.g. title-casing a name.
    func stageFromName(
        mask: String, paths: [String], groups: [ActionGroup] = []
    ) async -> Result<Int, SearchFailure> {
        guard let session, !mask.isEmpty, !paths.isEmpty else { return .success(0) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: [String: String]], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let reply: Reply<JSONValue>? = invoke(
                    box, "preview_tags_from_name",
                    encodeArgs(MaskPathsArg(mask: mask, paths: paths)))
                guard var plan = reply?.ok else {
                    return .failure(SearchFailure(
                        message: reply?.error?.text ?? "the tags could not be prepared"))
                }
                // Run the captured values through the chain before staging.
                if !groups.isEmpty {
                    let over: Reply<JSONValue>? = invoke(
                        box, "preview_transform_over_plan",
                        encodeArgs(TransformOverPlanArg(plan: plan, groups: groups)))
                    guard let transformed = over?.ok else {
                        return .failure(SearchFailure(
                            message: over?.error?.text ?? "the clean-up chain could not be applied"))
                    }
                    plan = transformed
                }
                guard let parsed = decodePlan(plan) else {
                    return .failure(SearchFailure(message: "could not read the plan"))
                }
                var diffs: [String: [String: String]] = [:]
                for change in parsed.changes {
                    for tag in change.tag_changes {
                        diffs[change.path, default: [:]][tag.field] = tag.new ?? ""
                    }
                }
                return .success((plan, diffs, parsed.changes.count))
            }.value

        switch result {
        case .success(let (plan, diffs, count)):
            staged = diffs
            stagedRenames.removeAll()
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged tags from name for \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            return .failure(failure)
        }
    }

    // MARK: - Generator

    /// Preview a transform chain over a scope ("tags", a field key, "filename"
    /// or "fileext"): what each file's value changes from and to. Read-only.
    func transformPreview(
        rules: [TransformRule],
        scope: String,
        paths: [String]
    ) async -> Result<[TransformPair], SearchFailure> {
        guard let session, !rules.isEmpty, !paths.isEmpty else { return .success([]) }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<JSONValue>? = invoke(
                box, "preview_transform",
                encodeArgs(TransformArgs(paths: paths, rules: rules, scope: scope)))
            guard let plan = reply?.ok else {
                return .failure(SearchFailure(
                    message: reply?.error?.text ?? "the transform could not be previewed"))
            }
            guard let parsed = decodePlan(plan) else {
                return .failure(SearchFailure(message: "could not read the transform plan"))
            }
            var pairs: [TransformPair] = []
            for change in parsed.changes {
                if let to = change.rename_to {
                    pairs.append(TransformPair(
                        label: "file", old: baseName(change.path), new: baseName(to)))
                }
                for tag in change.tag_changes {
                    pairs.append(TransformPair(
                        label: tag.field, old: tag.old ?? "", new: tag.new ?? ""))
                }
            }
            return .success(pairs)
        }.value
    }

    /// Build the transform plan and stage it: tag changes fill the table diff,
    /// a filename change fills the File column, the change-plan bar takes over.
    func stageTransform(
        rules: [TransformRule],
        scope: String,
        paths: [String]
    ) async -> Result<Int, SearchFailure> {
        guard let session, !rules.isEmpty, !paths.isEmpty else { return .success(0) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: [String: String]], [String: String], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let reply: Reply<JSONValue>? = invoke(
                    box, "preview_transform",
                    encodeArgs(TransformArgs(paths: paths, rules: rules, scope: scope)))
                guard let plan = reply?.ok else {
                    return .failure(SearchFailure(
                        message: reply?.error?.text ?? "the transform could not be prepared"))
                }
                guard let parsed = decodePlan(plan) else {
                    return .failure(SearchFailure(message: "could not read the transform plan"))
                }
                var diffs: [String: [String: String]] = [:]
                var renames: [String: String] = [:]
                for change in parsed.changes {
                    if let to = change.rename_to { renames[change.path] = baseName(to) }
                    for tag in change.tag_changes {
                        diffs[change.path, default: [:]][tag.field] = tag.new ?? ""
                    }
                }
                return .success((plan, diffs, renames, parsed.changes.count))
            }.value

        switch result {
        case .success(let (plan, diffs, renames, count)):
            staged = diffs
            stagedRenames = renames
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged a transform of \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            return .failure(failure)
        }
    }

    /// Stage a distinct set of edits per file — the mechanism behind numbering
    /// and the vinyl split, where each file gets its own value. The table diff
    /// and Apply both read `staged`, so writing it directly is a hand edit by
    /// another name; no-ops (a value already on disk) are dropped.
    private func stagePerFileEdits(_ edits: [String: [String: String]], message: String) {
        stagedPlan = nil
        stagedPlanCount = 0
        stagedRenames.removeAll()
        var next: [String: [String: String]] = [:]
        for (path, fields) in edits {
            guard let track = tracks.first(where: { $0.id == path }) else { continue }
            for (key, value) in fields where track.value(forKey: key) != value {
                next[path, default: [:]][key] = value
            }
        }
        staged = next
        lastMessage = message
    }

    /// Number the selected files in order from `start` (#G-2): each gets a track
    /// number, optionally the shared total, optionally a disc number.
    func numberTracks(paths: [String], start: Int, writeTotal: Bool, disc: String?) {
        guard !paths.isEmpty else { return }
        var edits: [String: [String: String]] = [:]
        for (offset, path) in paths.enumerated() {
            edits[path, default: [:]]["track"] = String(start + offset)
            if let disc, !disc.isEmpty { edits[path]?["disc"] = disc }
            if writeTotal { edits[path]?["tracktotal"] = String(paths.count) }
        }
        stagePerFileEdits(edits, message: "Numbered \(paths.count) track(s)")
    }

    /// Split a vinyl-side position (A1, B2) in each file's track tag into a disc
    /// and a track number (#G-3). Returns how many files carried one.
    func splitVinylSides(paths: [String]) -> Int {
        var edits: [String: [String: String]] = [:]
        for path in paths {
            guard let track = tracks.first(where: { $0.id == path }),
                  let parsed = parseVinylPosition(track.value(forKey: "track")) else { continue }
            edits[path] = ["track": parsed.track ?? "1", "disc": parsed.disc]
        }
        let count = edits.count
        if count > 0 { stagePerFileEdits(edits, message: "Split \(count) vinyl position(s)") }
        else { lastMessage = "No vinyl-side values (A1, B2) in the selection" }
        return count
    }

    /// The shipped rule chains (`builtin_action_groups`), to load into the editor.
    func builtinActionGroups() async -> [ActionGroup] {
        guard let session else { return [] }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<[ActionGroup]>? = invoke(box, "builtin_action_groups", "{}")
            return reply?.ok ?? []
        }.value
    }

    /// Preview an action-group chain over the selection (`preview_transform_groups`):
    /// every rule in order, each on its own scope or the group's. Read-only.
    func transformGroupsPreview(
        groups: [ActionGroup], paths: [String]
    ) async -> Result<[TransformPair], SearchFailure> {
        guard let session, !groups.isEmpty, !paths.isEmpty else { return .success([]) }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<JSONValue>? = invoke(
                box, "preview_transform_groups", encodeArgs(TransformGroupsArg(paths: paths, groups: groups)))
            guard let plan = reply?.ok else {
                return .failure(SearchFailure(message: reply?.error?.text ?? "the chain could not be previewed"))
            }
            guard let parsed = decodePlan(plan) else {
                return .failure(SearchFailure(message: "could not read the chain plan"))
            }
            var pairs: [TransformPair] = []
            for change in parsed.changes {
                if let to = change.rename_to {
                    pairs.append(TransformPair(label: "file", old: baseName(change.path), new: baseName(to)))
                }
                for tag in change.tag_changes {
                    pairs.append(TransformPair(label: tag.field, old: tag.old ?? "", new: tag.new ?? ""))
                }
            }
            return .success(pairs)
        }.value
    }

    /// Build the action-group plan and stage it (`preview_transform_groups`), the
    /// change bar applies it — one journaled batch.
    func stageTransformGroups(groups: [ActionGroup], paths: [String]) async -> Result<Int, SearchFailure> {
        guard let session, !groups.isEmpty, !paths.isEmpty else { return .success(0) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: [String: String]], [String: String], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let reply: Reply<JSONValue>? = invoke(
                    box, "preview_transform_groups", encodeArgs(TransformGroupsArg(paths: paths, groups: groups)))
                guard let plan = reply?.ok else {
                    return .failure(SearchFailure(message: reply?.error?.text ?? "the chain could not be prepared"))
                }
                guard let parsed = decodePlan(plan) else {
                    return .failure(SearchFailure(message: "could not read the chain plan"))
                }
                var diffs: [String: [String: String]] = [:]
                var renames: [String: String] = [:]
                for change in parsed.changes {
                    if let to = change.rename_to { renames[change.path] = baseName(to) }
                    for tag in change.tag_changes {
                        diffs[change.path, default: [:]][tag.field] = tag.new ?? ""
                    }
                }
                return .success((plan, diffs, renames, parsed.changes.count))
            }.value

        switch result {
        case .success(let (plan, diffs, renames, count)):
            staged = diffs
            stagedRenames = renames
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged a transform of \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            return .failure(failure)
        }
    }

    /// Run a chain over the already-staged plan (#G-4, `preview_transform_over_plan`):
    /// the transform layers on top of a staged import or from-name, so its values
    /// can be cleaned up before Apply. Replaces the staged plan with the result.
    func transformOverStagedPlan(groups: [ActionGroup]) async -> Result<Int, SearchFailure> {
        guard let session, let plan = stagedPlan, !groups.isEmpty else { return .success(0) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: [String: String]], [String: String], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let reply: Reply<JSONValue>? = invoke(
                    box, "preview_transform_over_plan",
                    encodeArgs(TransformOverPlanArg(plan: plan, groups: groups)))
                guard let newPlan = reply?.ok else {
                    return .failure(SearchFailure(message: reply?.error?.text ?? "the transform could not be applied"))
                }
                guard let parsed = decodePlan(newPlan) else {
                    return .failure(SearchFailure(message: "could not read the transformed plan"))
                }
                var diffs: [String: [String: String]] = [:]
                var renames: [String: String] = [:]
                for change in parsed.changes {
                    if let to = change.rename_to { renames[change.path] = baseName(to) }
                    for tag in change.tag_changes {
                        diffs[change.path, default: [:]][tag.field] = tag.new ?? ""
                    }
                }
                return .success((newPlan, diffs, renames, parsed.changes.count))
            }.value

        switch result {
        case .success(let (newPlan, diffs, renames, count)):
            staged = diffs
            stagedRenames = renames
            stagedPlan = newPlan
            stagedPlanCount = count
            lastMessage = "Applied the chain to \(count) staged change(s)"
            return .success(count)
        case .failure(let failure):
            lastMessage = failure.message
            return .failure(failure)
        }
    }

    // MARK: - Tag blocks (#47, #205)

    /// What the selection can be converted to: the block kinds every selected
    /// file can be given, and the ID3v2 revisions the app writes.
    struct BlockTargets {
        let kinds: [BlockOption]
        let revisions: [BlockOption]
    }

    struct BlockOption: Decodable, Hashable, Identifiable {
        let kind: String
        let label: String
        var id: String { kind }
    }

    private struct BlockTargetsReply: Decodable {
        let kinds: [BlockOption]
        let revisions: [BlockOption]
    }

    /// A previewed block plan held for the View to confirm and commit: the plan,
    /// the file count, whether undo would be lossy (`inexact`), and what a lossy
    /// change would drop — so the confirmation names it before anything stages.
    struct BlockPreview {
        let plan: JSONValue
        let count: Int
        let inexact: Bool
        let lostFields: [String]
        let lostPictures: Bool
    }

    /// The block kinds and revisions the selection can convert to (`tag_block_targets`).
    func tagBlockTargets(paths: [String]) async -> BlockTargets? {
        guard let session, !paths.isEmpty else { return nil }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<BlockTargetsReply>? =
                invoke(box, "tag_block_targets", encodeArgs(PathsArg(paths: paths)))
            guard let ok = reply?.ok else { return nil }
            return BlockTargets(kinds: ok.kinds, revisions: ok.revisions)
        }.value
    }

    /// Preview stripping one tag block from the selection (`preview_remove_tag_block`).
    /// The plan is not staged yet — the View confirms a lossy removal first.
    func previewRemoveTagBlock(kind: String, paths: [String]) async -> Result<BlockPreview, SearchFailure> {
        await previewBlockPlan(command: "preview_remove_tag_block",
                               args: encodeArgs(RemoveBlockArg(paths: paths, kind: kind)))
    }

    /// Preview converting the selection's read block into another kind or ID3v2
    /// revision (`preview_convert_tag_block`).
    func previewConvertTagBlock(
        from: String, to: String, revision: String?, paths: [String]
    ) async -> Result<BlockPreview, SearchFailure> {
        await previewBlockPlan(
            command: "preview_convert_tag_block",
            args: encodeArgs(ConvertBlockArg(paths: paths, from: from, to: to, revision: revision)))
    }

    private func previewBlockPlan(command: String, args: String) async -> Result<BlockPreview, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "no library open")) }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<JSONValue>? = invoke(box, command, args)
            guard let plan = reply?.ok else {
                return .failure(SearchFailure(message: reply?.error?.text ?? "the change could not be prepared"))
            }
            guard let parsed = decodePlan(plan) else {
                return .failure(SearchFailure(message: "could not read the plan"))
            }
            var inexact = false
            var lostFields = Set<String>()
            var lostPictures = false
            for change in parsed.changes {
                for block in change.block_changes ?? [] {
                    if block.exact == false { inexact = true }
                    (block.lost_fields ?? []).forEach { lostFields.insert($0) }
                    if block.lost_pictures == true { lostPictures = true }
                }
            }
            return .success(BlockPreview(
                plan: plan, count: parsed.changes.count, inexact: inexact,
                lostFields: lostFields.sorted(), lostPictures: lostPictures))
        }.value
    }

    /// Stage a previewed block plan — one journaled batch, applied by the change
    /// bar like any other. The tag diff (if the change moves values) fills the
    /// table; the block change itself rides in the plan.
    func commitBlockPreview(_ preview: BlockPreview) {
        guard let parsed = decodePlan(preview.plan) else { return }
        var diffs: [String: [String: String]] = [:]
        for change in parsed.changes {
            for tag in change.tag_changes {
                diffs[change.path, default: [:]][tag.field] = tag.new ?? ""
            }
        }
        staged = diffs
        stagedRenames.removeAll()
        stagedPlan = preview.plan
        stagedPlanCount = preview.count
        lastMessage = "Staged a tag-block change for \(preview.count) file(s)"
    }

    // MARK: - Cover editing (#56)

    /// One embedded image: its bytes as base64, its media type, and which cover
    /// it depicts. Codable both ways — decoded from a read, encoded into a set.
    struct CoverArt: Codable, Hashable {
        var mime: String
        var dataBase64: String
        var kind: String = ""
        var description: String = ""

        enum CodingKeys: String, CodingKey {
            case mime, kind, description
            case dataBase64 = "data_base64"
        }

        /// The bytes, for the cover well to render (decoded in the view layer,
        /// which owns AppKit).
        var data: Data? { Data(base64Encoded: dataBase64) }
    }

    /// Cover state across the selection, for the editor's cover well: how many
    /// files carry any image, whether they differ, the distinct fronts to fan out
    /// when mixed, and the shared set when they all carry the same one.
    struct CoverSummary: Decodable {
        var total: Int
        var withCover: Int
        var distinct: Bool
        var samples: [CoverArt]
        var sharedSet: [CoverArt]

        enum CodingKeys: String, CodingKey {
            case total, distinct, samples
            case withCover = "with_cover"
            case sharedSet = "shared_set"
        }
    }

    /// Read the selection's cover state (`read_cover_summary`).
    func coverSummary(paths: [String]) async -> CoverSummary? {
        guard let session, !paths.isEmpty else { return nil }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<CoverSummary>? =
                invoke(box, "read_cover_summary", encodeArgs(PathsArg(paths: paths)))
            return reply?.ok
        }.value
    }

    /// A cover found beside the selection on disk (`read_external_cover`) — a
    /// sibling `cover.jpg`/`folder.jpg` the app can embed. Nil when there is none.
    func externalCover(paths: [String]) async -> CoverArt? {
        guard let session, !paths.isEmpty else { return nil }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<CoverArt?>? =
                invoke(box, "read_external_cover", encodeArgs(PathsArg(paths: paths)))
            return reply?.ok ?? nil
        }.value
    }

    /// Read and validate an image file, returning it as a cover (`read_cover_image`).
    func readCoverImage(path: String) async -> Result<CoverArt, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "no library open")) }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<CoverArt>? =
                invoke(box, "read_cover_image", encodeArgs(ReadCoverImageArg(path: path)))
            if let cover = reply?.ok { return .success(cover) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "could not read the image"))
        }.value
    }

    /// Stage setting the selection's whole image set to `covers` (`preview_cover_set`).
    func stageCoverSet(paths: [String], covers: [CoverArt]) async -> Result<Int, SearchFailure> {
        await stageCoverPlan(command: "preview_cover_set",
                             args: encodeArgs(CoverSetArg(paths: paths, covers: covers)),
                             verb: "cover set")
    }

    /// Stage stripping every embedded image from the selection (`preview_cover_remove`).
    func stageCoverRemove(paths: [String]) async -> Result<Int, SearchFailure> {
        await stageCoverPlan(command: "preview_cover_remove",
                             args: encodeArgs(PathsArg(paths: paths)),
                             verb: "cover removal")
    }

    private func stageCoverPlan(command: String, args: String, verb: String) async -> Result<Int, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "no library open")) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, Int), SearchFailure> = await Task.detached(priority: .userInitiated) {
            let reply: Reply<JSONValue>? = invoke(box, command, args)
            guard let plan = reply?.ok else {
                return .failure(SearchFailure(message: reply?.error?.text ?? "the change could not be prepared"))
            }
            guard let parsed = decodePlan(plan) else {
                return .failure(SearchFailure(message: "could not read the plan"))
            }
            return .success((plan, parsed.changes.count))
        }.value

        switch result {
        case .success(let (plan, count)):
            guard count > 0 else {
                lastMessage = "No file needed that \(verb)"
                return .success(0)
            }
            staged.removeAll()
            stagedRenames.removeAll()
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged a \(verb) for \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            lastMessage = failure.message
            return .failure(failure)
        }
    }

    /// Stage clearing the text tags on the selection (`preview_clear_tags`): cover
    /// art and DJ cue points are kept. The cleared values fill the table diff, and
    /// the change bar's Apply writes it — one journaled, undoable batch.
    func stageClearTags(paths: [String]) async -> Result<Int, SearchFailure> {
        guard let session, !paths.isEmpty else { return .success(0) }
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: [String: String]], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let reply: Reply<JSONValue>? =
                    invoke(box, "preview_clear_tags", encodeArgs(PathsArg(paths: paths)))
                guard let plan = reply?.ok else {
                    return .failure(SearchFailure(message: reply?.error?.text ?? "clearing could not be prepared"))
                }
                guard let parsed = decodePlan(plan) else {
                    return .failure(SearchFailure(message: "could not read the plan"))
                }
                var diffs: [String: [String: String]] = [:]
                for change in parsed.changes {
                    for tag in change.tag_changes {
                        diffs[change.path, default: [:]][tag.field] = tag.new ?? ""
                    }
                }
                return .success((plan, diffs, parsed.changes.count))
            }.value

        switch result {
        case .success(let (plan, diffs, count)):
            guard count > 0 else { lastMessage = "Nothing to clear"; return .success(0) }
            staged = diffs
            stagedRenames.removeAll()
            stagedPlan = plan
            stagedPlanCount = count
            lastMessage = "Staged clearing tags on \(count) file(s)"
            return .success(count)
        case .failure(let failure):
            lastMessage = failure.message
            return .failure(failure)
        }
    }

    // MARK: - Export

    /// Write an export of `paths` into the library folder and return the path
    /// written. Read-only for the audio files — it only adds an export file.
    /// `format` is one of playlist / cue / csv / html / xml / report; `mask` is
    /// used only by report.
    func export(
        format: String,
        fileName: String,
        mask: String,
        paths: [String]
    ) async -> Result<String, SearchFailure> {
        guard let session, !paths.isEmpty else {
            return .failure(SearchFailure(message: "Nothing to export"))
        }
        let command: String
        let args: String
        switch format {
        case "report":
            command = "export_report"
            args = encodeArgs(ReportArg(paths: paths, mask: mask, file_name: fileName))
        default:
            command = "export_\(format)"
            args = encodeArgs(ExportArg(paths: paths, file_name: fileName))
        }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<String>? = invoke(box, command, args)
            if let path = reply?.ok { return .success(path) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "the export failed"))
        }.value
    }

    /// Write one playlist per group (`export_playlists`, #46): `grouping` is
    /// "folder" or "album", `nameMask` names each playlist from that group's
    /// tags. Returns the files written.
    func exportPlaylists(
        grouping: String, nameMask: String, paths: [String]
    ) async -> Result<[String], SearchFailure> {
        guard let session, !paths.isEmpty else {
            return .failure(SearchFailure(message: "Nothing to export"))
        }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<[String]>? = invoke(
                box, "export_playlists",
                encodeArgs(ExportPlaylistsArg(paths: paths, grouping: grouping, name_mask: nameMask)))
            if let written = reply?.ok { return .success(written) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "the export failed"))
        }.value
    }

    // MARK: - Duplicates

    /// Scan the whole open library for likely duplicates under `criterion`
    /// ("artist_title", "album_track", "duration", "size", "hash"). Read-only.
    func findDuplicates(criterion: String) async -> Result<[DuplicateGroup], SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<[DuplicateGroup]>? =
                invoke(box, "find_duplicates", encodeArgs(CriterionArg(criterion: criterion)))
            if let groups = reply?.ok { return .success(groups) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "the scan failed"))
        }.value
    }

    /// Render a mask as a computed column (`render_column`, T2 custom column):
    /// path → the mask rendered against that file's tags. Read-only.
    func renderColumn(pattern: String, paths: [String]) async -> [String: String] {
        guard let session, !pattern.isEmpty, !paths.isEmpty else { return [:] }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) { () -> [String: String] in
            let reply: Reply<[String]>? = invoke(
                box, "render_column", encodeArgs(RenderColumnArg(pattern: pattern, paths: paths)))
            guard let values = reply?.ok, values.count == paths.count else { return [:] }
            return Dictionary(uniqueKeysWithValues: zip(paths, values))
        }.value
    }

    /// Move `paths` to the Trash (`trash_files`) — recoverable, not a delete.
    /// Returns the paths actually trashed; the library is re-read after so the
    /// table drops them.
    func trashFiles(_ paths: [String]) async -> Result<[String], SearchFailure> {
        guard let session, !paths.isEmpty else { return .success([]) }
        isBusy = true
        defer { isBusy = false }
        let box = SessionHandle(raw: session)
        let result: Result<[String], SearchFailure> = await Task.detached(priority: .userInitiated) {
            let reply: Reply<[String]>? = invoke(box, "trash_files", encodeArgs(PathsArg(paths: paths)))
            if let trashed = reply?.ok { return .success(trashed) }
            return .failure(SearchFailure(message: reply?.error?.text ?? "the files could not be trashed"))
        }.value
        if case .success(let trashed) = result {
            lastMessage = "Moved \(trashed.count) file(s) to the Trash"
            await rescan()
        }
        return result
    }

    // MARK: - Player

    /// The last status read from the player, or nil when nothing is loaded.
    private(set) var playerStatus: PlayerStatus?

    /// The paths playback walks for gapless advance (the visible rows at the
    /// moment Play was pressed), and the track we have already queued a next for.
    private var playQueue: [String] = []
    private var fedNextFor: String?
    private var polling: Task<Void, Never>?
    /// Last polled position, to notice a loop (position wraps while the path
    /// stays) so repeat-one and repeat-all can re-queue the next track.
    private var lastPosition: Double = 0

    /// How playback advances at the end of a track (#player repeat).
    enum RepeatMode: String { case off, all, one }
    var repeatMode: RepeatMode = .off

    func cycleRepeat() {
        repeatMode = switch repeatMode {
        case .off: .all
        case .all: .one
        case .one: .off
        }
        // Re-decide what follows the current track under the new mode.
        fedNextFor = nil
    }

    /// The track to queue after `current`, under the repeat mode: itself for
    /// "one", the next row (wrapping for "all"), or the next row / nothing for
    /// "off". Mirrors the Tauri `nextPath`.
    private func nextPath(after current: String) -> String? {
        guard let index = playQueue.firstIndex(of: current) else {
            return repeatMode == .one ? current : nil
        }
        switch repeatMode {
        case .one: return current
        case .all: return index + 1 < playQueue.count ? playQueue[index + 1] : playQueue.first
        case .off: return index + 1 < playQueue.count ? playQueue[index + 1] : nil
        }
    }

    var isPlaying: Bool {
        guard let status = playerStatus else { return false }
        return status.path != nil && !status.isPaused
    }

    /// The loaded track, matched back to a row so the bar can name it.
    var nowPlaying: Track? {
        guard let path = playerStatus?.path else { return nil }
        return tracks.first { $0.id == path }
    }

    func play(_ path: String, queue: [String]) {
        guard let session else { return }
        playQueue = queue
        fedNextFor = nil
        fire(session, "player_play", encodeArgs(PathArg(path: path)))
        startPolling()
    }

    func togglePause() {
        guard let session, let status = playerStatus, status.path != nil else { return }
        fire(session, status.isPaused ? "player_resume" : "player_pause", "{}")
    }

    func stopPlayback() {
        guard let session else { return }
        fire(session, "player_stop", "{}")
        polling?.cancel()
        polling = nil
        playerStatus = nil
    }

    func seek(to secs: Double) {
        guard let session else { return }
        fire(session, "player_seek", encodeArgs(SecsArg(secs: secs)))
    }

    func setVolume(_ level: Double) {
        guard let session else { return }
        fire(session, "player_set_volume", encodeArgs(LevelArg(level: level)))
    }

    /// Send a fire-and-forget player command off the main actor.
    private func fire(_ session: OpaquePointer, _ cmd: String, _ args: String) {
        let box = SessionHandle(raw: session)
        Task.detached(priority: .userInitiated) {
            _ = invoke(box, cmd, args) as Reply<EmptyOk>?
        }
    }

    private func startPolling() {
        polling?.cancel()
        polling = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refreshStatus()
                try? await Task.sleep(for: .milliseconds(300))
            }
        }
    }

    private func refreshStatus() async {
        guard let session else { return }
        let box = SessionHandle(raw: session)
        let reply: Reply<PlayerStatus>? =
            await Task.detached(priority: .userInitiated) { invoke(box, "player_status", "{}") }.value
        guard let status = reply?.ok else { return }

        // A wrapped position on the same track means it looped or was restarted:
        // clear the fed-next latch so repeat can queue the follow-up again.
        if let current = status.path, current == playerStatus?.path,
           status.positionSecs + 2 < lastPosition {
            fedNextFor = nil
        }
        lastPosition = status.positionSecs
        playerStatus = status

        // Gapless: when the player asks for a next track and one hasn't been fed
        // for the current track yet, queue the follow-up the repeat mode picks.
        if status.wantsNext, let current = status.path, fedNextFor != current {
            fedNextFor = current
            if let next = nextPath(after: current) {
                fire(session, "player_set_next", encodeArgs(PathArg(path: next)))
            }
        }
    }

    /// The amplitude envelope of a track (`waveform`): 1000 buckets, each 0…255.
    func waveform(path: String) async -> [UInt8]? {
        guard let session else { return nil }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
            let reply: Reply<[UInt8]>? = invoke(box, "waveform", encodeArgs(PathArg(path: path)))
            return reply?.ok
        }.value
    }

    /// The config dir (and so the journal) lives beside the app's own data, not
    /// in the music folder — a stand should leave nothing behind in a library it
    /// was pointed at. One dir per library, keyed by path, so two folders never
    /// share an undo history.
    private static func configDir(for folder: URL) -> String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first!
            .appendingPathComponent("TagRex Spike", isDirectory: true)
        // A stable digest of the path, not `String.hashValue` — Swift seeds its
        // hasher randomly per process (SE-0206), so hashValue gave a fresh
        // directory every launch and the journal (and any saved token) never
        // persisted (#311).
        let digest = SHA256.hash(data: Data(folder.path.utf8))
        let key = digest.prefix(8).map { String(format: "%02x", $0) }.joined()
        return support.appendingPathComponent("lib-\(key)", isDirectory: true).path
    }
}

/// The `ok` payload for a call that returns nothing but success.
private struct EmptyOk: Decodable {}

/// An opaque JSON value, to carry a plan back into the next call without the
/// stand having to model the whole `PlanDto`. `@unchecked Sendable`: it holds
/// immutable JSON data (dictionaries, arrays and scalars decoded once), so it is
/// safe to hand a staged plan back from a detached task to the main actor.
// Internal (not private): the tag-block preview holds a plan of this type and is
// read from `TagBlocks.swift`, so the type has to be visible across the module.
struct JSONValue: Codable, @unchecked Sendable {
    let value: Any

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        value = try JSONValue.decode(container)
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try JSONValue.encode(value, into: &container)
    }

    private static func decode(_ c: SingleValueDecodingContainer) throws -> Any {
        if let v = try? c.decode([String: JSONValue].self) { return v.mapValues(\.value) }
        if let v = try? c.decode([JSONValue].self) { return v.map(\.value) }
        if let v = try? c.decode(Bool.self) { return v }
        if let v = try? c.decode(Int64.self) { return v }
        if let v = try? c.decode(Double.self) { return v }
        if let v = try? c.decode(String.self) { return v }
        return NSNull()
    }

    private static func encode(_ value: Any, into c: inout SingleValueEncodingContainer) throws {
        switch value {
        case let v as [String: Any]: try c.encode(v.mapValues(JSONValue.init(wrapping:)))
        case let v as [Any]: try c.encode(v.map(JSONValue.init(wrapping:)))
        case let v as Bool: try c.encode(v)
        case let v as Int64: try c.encode(v)
        case let v as Int: try c.encode(Int64(v))
        case let v as Double: try c.encode(v)
        case let v as String: try c.encode(v)
        default: try c.encodeNil()
        }
    }

    private init(wrapping value: Any) { self.value = value }
}

// MARK: - Bridge plumbing

/// Serializes every `tagrex_invoke`. The backend session is single-threaded —
/// concurrent calls (the card-count prefetch's pool, cover fetches and the
/// player poll all run off the main actor) panic across the C ABI, which cannot
/// unwind and aborts the app. One lock around the call makes the FFI sequential
/// regardless of how many callers overlap.
private let ffiLock = NSLock()

/// Invoke a command and decode its envelope. Runs off the main actor; the
/// pointer is boxed Sendable and the call is serialized by `ffiLock`.
private func invoke<T: Decodable>(_ session: SessionHandle, _ cmd: String, _ args: String) -> Reply<T>? {
    cmd.withCString { cmdPtr in
        args.withCString { argsPtr in
            ffiLock.lock()
            let raw = tagrex_invoke(session.raw, cmdPtr, argsPtr)
            ffiLock.unlock()
            guard let raw else { return nil }
            defer { tagrex_string_free(raw) }
            return decode(Reply<T>.self, from: raw)
        }
    }
}

/// An `invoke` argument object. Each command's shape is its own Encodable, so
/// the keys are exactly what the command names its parameters — no snake-case
/// strategy, which would also rewrite the plan's own keys when it round-trips.
private struct EditsArg: Encodable {
    let edits: [[String: String]]
}

private struct PlanArg: Encodable {
    let plan: JSONValue
}

private struct PathsArg: Encodable {
    let paths: [String]
}

private struct RemoveBlockArg: Encodable {
    let paths: [String]
    let kind: String
}

private struct ConvertBlockArg: Encodable {
    let paths: [String]
    let from: String
    let to: String
    let revision: String?
}

private struct ReadCoverImageArg: Encodable {
    let path: String
}

private struct LockedFieldsArg: Encodable {
    let fields: [String]
}

private struct RenderColumnArg: Encodable {
    let pattern: String
    let paths: [String]
}

private struct MoveArg: Encodable {
    let mask: String
    let paths: [String]
    let destination: String?
    let copy: Bool
    let prune_empty_dirs: Bool
}

private struct CoverSetArg: Encodable {
    let paths: [String]
    let covers: [Library.CoverArt]
}

private struct UndoArg: Encodable {
    let batchId: Int

    enum CodingKeys: String, CodingKey {
        case batchId = "batch_id"
    }
}

private func encodeArgs<T: Encodable>(_ value: T) -> String {
    guard let data = try? JSONEncoder().encode(value),
          let text = String(data: data, encoding: .utf8)
    else { return "{}" }
    return text
}

private func decode<T: Decodable>(_ type: T.Type, from raw: UnsafeMutablePointer<CChar>) -> T? {
    let json = Data(String(cString: raw).utf8)
    return try? JSONDecoder().decode(type, from: json)
}

/// The token a source needs, resolved off the main actor (it is itself an
/// invoke). MusicBrainz needs none; Discogs reads the saved token (empty is
/// fine, the provider says so); Beatport asks for a fresh access token and
/// surfaces "not signed in" as a failure rather than searching with none.
private func resolveToken(_ box: SessionHandle, _ source: Source) -> Result<String, SearchFailure> {
    switch source {
    case .musicbrainz:
        return .success("")
    case .discogs:
        let reply: Reply<String>? = invoke(box, "saved_discogs_token", "{}")
        return .success(reply?.ok ?? "")
    case .beatport:
        let reply: Reply<String>? = invoke(box, "beatport_token", "{}")
        if let token = reply?.ok { return .success(token) }
        return .failure(SearchFailure(message: reply?.error?.text ?? "Not signed in to Beatport"))
    }
}

private func blankToNil(_ text: String) -> String? {
    let trimmed = text.trimmingCharacters(in: .whitespaces)
    return trimmed.isEmpty ? nil : trimmed
}

private struct SearchArgs: Encodable {
    let source: String
    let token: String
    let query: Query

    /// Optional fields the synthesized encoder omits when nil, which is what the
    /// backend's `SearchQueryDto` expects for an absent term.
    struct Query: Encodable {
        let artist: String?
        let album: String?
        let catalog_number: String?
        let format: String?
        let page: Int
        let per_page: Int
    }
}

private struct FetchArgs: Encodable {
    let source: String
    let token: String
    let release_id: String
}

private struct FetchImageArgs: Encodable {
    let source: String
    let token: String
    let url: String
}

/// The subset of `SettingsDto` the stand reads back; unknown keys are ignored.
private struct LoadedSettings: Decodable {
    let proxy: String?
    let rate_limit_per_min: Int?
    let id3_v23: Bool?
    let action_groups: [ActionGroup]?
}

private struct SaveSettingsArgs: Encodable {
    struct Payload: Encodable {
        let proxy: String
        let rate_limit_per_min: Int
        let id3_v23: Bool
        // The stand owns its own settings.json, so round-tripping the saved
        // groups keeps a settings save from wiping them (save replaces the whole
        // SettingsDto).
        let action_groups: [ActionGroup]
    }
    let settings: Payload
}

private struct SaveTokenArgs: Encodable {
    let token: String
}

/// The reply from `provider_fetch_image`: the raw bytes as base64 and their mime.
private struct ProviderImage: Decodable {
    let mime: String
    let data_base64: String
}

/// A cover crossing to `preview_cover_embed` as the backend `CoverArtDto`; the
/// `kind`/`description` fields default on the backend, so only these two are sent.
private struct CoverArtArg: Encodable {
    let mime: String
    let data_base64: String
}

private struct CoverEmbedArgs: Encodable {
    let paths: [String]
    let cover: CoverArtArg
}

private struct SaveImagesArgs: Encodable {
    let source: String
    let token: String
    let path: String
    let urls: [String]
    let overwrite: Bool
}

/// The reply from `save_release_images`: the files written, and the ones that
/// already exist (only reported when `overwrite` was false).
struct SaveImagesResult: Decodable {
    let written: [String]
    let conflicts: [String]
}

// One release track as the backend's ImportTrackDto; snake_case keys are the
// property names, since the ABI does not convert them.
private struct ImportTrack: Encodable {
    let position: String
    let disc: Int?
    let artist: String
    let title: String
    let duration_secs: Int?
    let isrc: String?
    let bpm: Int?
    let key: String?
}

private func importTrack(from track: ReleaseTrack, albumArtist: String) -> ImportTrack {
    let artist = (track.artist?.isEmpty == false) ? track.artist! : albumArtist
    return ImportTrack(
        position: track.position,
        disc: track.disc,
        artist: artist,
        title: track.title,
        duration_secs: track.durationSecs,
        isrc: track.isrc,
        bpm: track.bpm,
        key: track.key
    )
}

private struct ImportSelection: Encodable {
    let album: String?
    let album_artist: String?
    let year: String?
    let genre: String?
    let tracks: [ImportTrack]
    let release_id: String?
    let source: String?
    // The chosen label imprint (#90) and the rest of the album-level fields the
    // Tauri import writes; omitted by the encoder when nil.
    let label: String?
    let catalog_number: String?
    let country: String?
    let track_total: String?
    let disc_total: String?
    let url: String?
    let media_type: String?
}

private struct AlignArgs: Encodable {
    let paths: [String]
    let tracks: [ImportTrack]
}

private struct MaskPathsArg: Encodable {
    let mask: String
    let paths: [String]
}

private struct TransformArgs: Encodable {
    let paths: [String]
    let rules: [TransformRule]
    let scope: String
}

private struct TransformGroupsArg: Encodable {
    let paths: [String]
    let groups: [ActionGroup]
}

private struct TransformOverPlanArg: Encodable {
    let plan: JSONValue
    let groups: [ActionGroup]
}

private struct CriterionArg: Encodable {
    let criterion: String
}

private struct ProbeArg: Encodable {
    let mask: String
    let path: String
}

private struct ExportArg: Encodable {
    let paths: [String]
    let file_name: String
}

private struct ReportArg: Encodable {
    let paths: [String]
    let mask: String
    let file_name: String
}

private struct ExportPlaylistsArg: Encodable {
    let paths: [String]
    let grouping: String
    let name_mask: String
}

/// Decode a plan (as a JSONValue) into the parts the stand reflects — visible
/// tag changes and renames. Re-encodes the opaque value, then reads the shape.
private func decodePlan(_ plan: JSONValue) -> StagedPlanShape? {
    guard let data = try? JSONEncoder().encode(plan) else { return nil }
    return try? JSONDecoder().decode(StagedPlanShape.self, from: data)
}

private func baseName(_ path: String) -> String {
    (path as NSString).lastPathComponent
}

/// Parse a vinyl-side position ("A1", "B", reverse "1A") into a disc (the side
/// letter, A→1) and a track number, mirroring the Tauri `parseVinylPosition`.
private func parseVinylPosition(_ value: String) -> (disc: String, track: String?)? {
    let v = value.trimmingCharacters(in: .whitespaces)
    guard let first = v.first else { return nil }
    let side: Character
    let num: Substring
    if first.isLetter, v.dropFirst().allSatisfy(\.isNumber) {
        side = first
        num = v.dropFirst()
    } else if v.count >= 2, let last = v.last, last.isLetter, v.dropLast().allSatisfy(\.isNumber) {
        side = last
        num = v.dropLast()
    } else {
        return nil
    }
    guard let scalar = String(side).uppercased().unicodeScalars.first else { return nil }
    let disc = Int(scalar.value) - 64
    guard disc >= 1, disc <= 26 else { return nil }
    return (String(disc), num.isEmpty ? nil : String(Int(num) ?? 1))
}

/// A path shown relative to `base` (the move destination or library root), so a
/// reorganise preview reads as the folder tree it builds rather than a long
/// absolute path. Falls back to the last two components when `base` isn't a
/// prefix (a move outside the root).
private func relativePath(_ path: String, under base: String?) -> String {
    if var base, !base.isEmpty {
        while base.count > 1, base.hasSuffix("/") { base.removeLast() }
        if path.hasPrefix(base + "/") { return String(path.dropFirst(base.count + 1)) }
    }
    let parts = (path as NSString).pathComponents
    return parts.suffix(2).joined(separator: "/")
}

private struct PathArg: Encodable {
    let path: String
}

private struct SecsArg: Encodable {
    let secs: Double
}

private struct LevelArg: Encodable {
    let level: Double
}

private struct ImportArgs: Encodable {
    let paths: [String]
    let selection: ImportSelection
    let vinyl_sides_to_disc: Bool
}

/// One `auto_align` result. Only the matched track index is needed here.
private struct AlignMatch: Decodable {
    let track: Int
}

/// The parts of a `PlanDto` the stand reflects in the table diff.
private struct StagedPlanShape: Decodable {
    struct FileChange: Decodable {
        let path: String
        let rename_to: String?
        let tag_changes: [FieldChange]
        /// Whole-block changes (#47, #205). Present on a strip/convert plan; the
        /// `exact` flag says whether undo can restore the block byte-for-byte.
        let block_changes: [BlockChange]?
    }

    struct FieldChange: Decodable {
        let field: String
        let old: String?
        let new: String?
    }

    struct BlockChange: Decodable {
        let label: String?
        let exact: Bool?
        let lost_fields: [String]?
        let lost_pictures: Bool?
    }

    let changes: [FileChange]
}
