// The model side (#271, #293): open a session, list, stage, apply, undo — all
// through the command layer's own path, so a write from here is gated and
// journaled exactly as a write from the app is. The bridge is the session ABI
// in crates/ffi: `tagrex_open` once, then `tagrex_invoke` by command name.

import CTagRex
import Foundation

struct Track: Identifiable, Decodable, Hashable {
    var path: String
    var format: String
    /// Storage-key -> value, the way the command layer reports a track. The UI
    /// reads fields out of here, so it shares one vocabulary with the backend.
    var tags: [String: String]
    var unreadable: Bool
    var durationSecs: UInt64?

    var id: String { path }

    var file: String { (path as NSString).lastPathComponent }

    var artist: String { value(for: .artist) }
    var title: String { value(for: .title) }
    var album: String { value(for: .album) }
    var albumartist: String { value(for: .albumartist) }
    var year: String { value(for: .year) }
    var genre: String { value(for: .genre) }
    var track: String { value(for: .track) }

    var duration: String {
        guard let secs = durationSecs else { return "" }
        return String(format: "%d:%02d", secs / 60, secs % 60)
    }

    /// The value the core stores under `key`, so the UI and the bridge agree
    /// about field names without a second vocabulary.
    func value(for key: Field) -> String {
        tags[key.rawValue] ?? ""
    }

    // Mapped by hand rather than through a snake-case decoding strategy: that
    // strategy also rewrites dictionary keys, which would mangle the `tags` map
    // and any plan the bridge round-trips back into a later call.
    enum CodingKeys: String, CodingKey {
        case path, format, tags, unreadable
        case durationSecs = "duration_secs"
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

    /// The staged plan: path → field → new value. Nothing is on disk until
    /// Apply, and this is the only thing Discard has to throw away.
    private(set) var staged: [String: [Field: String]] = [:]

    var filter = ""
    var showsOldValues = false

    var rootName: String { root?.lastPathComponent ?? "No folder open" }
    var stagedFileCount: Int { staged.count }
    var hasStagedPlan: Bool { !staged.isEmpty }

    /// The live session. Held across calls, closed when another folder opens.
    /// Not closed on deinit — a `Library` lives for the window's lifetime, and
    /// deinit cannot reach a main-actor property to close it; the process exit
    /// reclaims the last one.
    private var session: OpaquePointer?

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
            await rescan()
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

    /// Stage the edits into a plan, then apply that plan — the same two steps the
    /// interface takes, so the write goes through the gate rather than around it.
    func apply() async {
        guard let session, !staged.isEmpty else { return }
        isBusy = true
        defer { isBusy = false }

        let edits: [[String: String]] = staged.flatMap { path, fields in
            fields.map { field, value in
                ["path": path, "field": field.rawValue, "value": value]
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

    /// The config dir (and so the journal) lives beside the app's own data, not
    /// in the music folder — a stand should leave nothing behind in a library it
    /// was pointed at. One dir per library, keyed by path, so two folders never
    /// share an undo history.
    private static func configDir(for folder: URL) -> String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first!
            .appendingPathComponent("TagRex Spike", isDirectory: true)
        let key = String(folder.path.hashValue, radix: 16)
        return support.appendingPathComponent("lib-\(key)", isDirectory: true).path
    }
}

/// The `ok` payload for a call that returns nothing but success.
private struct EmptyOk: Decodable {}

/// An opaque JSON value, to carry a plan back into the next call without the
/// stand having to model the whole `PlanDto`.
private struct JSONValue: Codable {
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

/// Invoke a command and decode its envelope. Runs off the main actor; the
/// pointer is boxed Sendable and access is serialized by the caller.
private func invoke<T: Decodable>(_ session: SessionHandle, _ cmd: String, _ args: String) -> Reply<T>? {
    cmd.withCString { cmdPtr in
        args.withCString { argsPtr in
            guard let raw = tagrex_invoke(session.raw, cmdPtr, argsPtr) else { return nil }
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
