// The model side (#271): scan, stage, apply, undo — all through the core's own
// change-plan path, so a write from here is gated and journaled exactly as a
// write from the app is.

import CTagRex
import Foundation

struct Track: Identifiable, Decodable, Hashable {
    var path: String
    var file: String
    var format: String
    var artist: String
    var title: String
    var album: String
    var albumartist: String
    var year: String
    var genre: String
    var track: String
    var durationSecs: UInt64
    var bitrateKbps: UInt32?

    var id: String { path }

    var duration: String {
        String(format: "%d:%02d", durationSecs / 60, durationSecs % 60)
    }

    /// The value the core stores under `key`, so the UI and the bridge agree
    /// about field names without a second vocabulary.
    func value(for key: Field) -> String {
        switch key {
        case .artist: artist
        case .title: title
        case .album: album
        case .albumartist: albumartist
        case .year: year
        case .genre: genre
        case .track: self.track
        }
    }
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

private struct LibraryPayload: Decodable {
    var root: String
    var rows: [Track]
    var errors: [String]
}

private struct WriteResult: Decodable {
    var applied: Int
    var batch: Int?
    var description: String
    var errors: [String]
}

@Observable
@MainActor
final class Library {
    private(set) var root: URL?
    private(set) var tracks: [Track] = []
    private(set) var errors: [String] = []
    private(set) var isBusy = false
    private(set) var lastMessage = ""

    /// The staged plan: path → field → new value. Nothing is on disk until
    /// Apply, and this is the only thing Discard has to throw away.
    private(set) var staged: [String: [Field: String]] = [:]

    var filter = ""
    var showsOldValues = false

    var rootName: String { root?.lastPathComponent ?? "No folder open" }
    var stagedFileCount: Int { staged.count }
    var hasStagedPlan: Bool { !staged.isEmpty }

    // MARK: - Reading

    var visibleTracks: [Track] {
        let query = filter.trimmingCharacters(in: .whitespaces).lowercased()
        guard !query.isEmpty else { return tracks }

        if let colon = query.firstIndex(of: ":") {
            let name = String(query[query.startIndex..<colon])
            let value = String(query[query.index(after: colon)...])
            if !value.isEmpty, let field = Field(rawValue: name) {
                return tracks.filter { $0.value(for: field).lowercased().contains(value) }
            }
        }

        return tracks.filter { track in
            [track.file, track.artist, track.title, track.album]
                .contains { $0.lowercased().contains(query) }
        }
    }

    func open(_ folder: URL) async {
        root = folder
        staged.removeAll()
        await rescan()
    }

    func rescan() async {
        guard let folder = root else { return }
        isBusy = true
        defer { isBusy = false }

        let path = folder.path
        let payload: LibraryPayload? = await Task.detached(priority: .userInitiated) {
            guard let raw = tagrex_scan_json(path) else { return nil }
            defer { tagrex_string_free(raw) }
            return decode(LibraryPayload.self, from: raw)
        }.value

        tracks = payload?.rows ?? []
        errors = payload?.errors ?? ["the scan returned nothing readable"]
    }

    // MARK: - Staging

    /// Stage one field across a selection. An empty string clears the tag; a
    /// value equal to what the file already holds stages nothing, so typing a
    /// value back to what it was cancels the change instead of recording a
    /// no-op the way the web editor does.
    func stage(_ field: Field, to value: String, for ids: [Track.ID]) {
        for id in ids {
            guard let track = tracks.first(where: { $0.id == id }) else { continue }

            if track.value(for: field) == value {
                staged[id]?.removeValue(forKey: field)
            } else {
                staged[id, default: [:]][field] = value
            }
            if staged[id]?.isEmpty == true {
                staged.removeValue(forKey: id)
            }
        }
    }

    /// The staged value for a cell, or nil when the cell is unchanged.
    func stagedValue(_ field: Field, for id: Track.ID) -> String? {
        staged[id]?[field]
    }

    func discard() {
        staged.removeAll()
        lastMessage = "Discarded"
    }

    // MARK: - Writing

    func apply() async {
        guard let folder = root, !staged.isEmpty else { return }
        isBusy = true
        defer { isBusy = false }

        let edits = staged.map { path, fields in
            ["path": path, "fields": fields.reduce(into: [String: String]()) { out, pair in
                out[pair.key.rawValue] = pair.value
            }] as [String: Any]
        }

        let request: [String: Any] = [
            "root": folder.path,
            "journal": Self.journalPath(for: folder),
            "description": "Edit tags",
            "edits": edits,
        ]

        let result = await call(.apply, request)
        if let result, result.errors.isEmpty {
            lastMessage = "Applied to \(result.applied) file(s)"
            staged.removeAll()
        } else {
            lastMessage = result?.errors.first ?? "the write failed"
        }
        await rescan()
    }

    func undo() async {
        guard let folder = root else { return }
        isBusy = true
        defer { isBusy = false }

        let request: [String: Any] = [
            "root": folder.path,
            "journal": Self.journalPath(for: folder),
        ]

        let result = await call(.undo, request)
        if let result, result.errors.isEmpty {
            lastMessage = result.batch == nil
                ? "Nothing to undo"
                : "Undone: \(result.description)"
        } else {
            lastMessage = result?.errors.first ?? "the undo failed"
        }
        await rescan()
    }

    /// Which write the bridge should perform. An enum rather than a function
    /// pointer: a C function is not Sendable, and crossing it into a detached
    /// task is exactly the kind of thing the concurrency checker is right to
    /// complain about.
    private enum Write {
        case apply, undo
    }

    private func call(_ write: Write, _ request: [String: Any]) async -> WriteResult? {
        guard let body = try? JSONSerialization.data(withJSONObject: request),
              let text = String(data: body, encoding: .utf8)
        else { return nil }

        return await Task.detached(priority: .userInitiated) {
            text.withCString { pointer in
                let raw = switch write {
                case .apply: tagrex_apply_json(pointer)
                case .undo: tagrex_undo_json(pointer)
                }
                guard let raw else { return nil }
                defer { tagrex_string_free(raw) }
                return decode(WriteResult.self, from: raw)
            }
        }.value
    }

    /// The journal lives beside the app's own data, not in the music folder —
    /// a stand should leave nothing behind in a library it was pointed at.
    private static func journalPath(for folder: URL) -> String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first!
            .appendingPathComponent("TagRex Spike", isDirectory: true)
        try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)

        // One journal per library, keyed by the path so two folders never share
        // an undo history.
        let key = String(folder.path.hashValue, radix: 16)
        return support.appendingPathComponent("journal-\(key).sqlite").path
    }
}

private func decode<T: Decodable>(_ type: T.Type, from raw: UnsafeMutablePointer<CChar>) -> T? {
    let json = Data(String(cString: raw).utf8)
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try? decoder.decode(type, from: json)
}
