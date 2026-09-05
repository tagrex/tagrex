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

    enum CodingKeys: String, CodingKey {
        case id, artist, title, year, score, country, label, format
        case catalogNumber = "catalog_number"
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

    /// The value the import writes to the genre tag: the styles joined, else the
    /// genres.
    var importGenre: String? {
        let chosen = (styles?.isEmpty == false ? styles : genres) ?? []
        let joined = chosen.joined(separator: "/")
        return joined.isEmpty ? nil : joined
    }
}

/// A search or fetch failure, carrying the message to show in the panel.
/// `Result`'s failure type must be an `Error`, and a bare `String` is not one.
struct SearchFailure: Error {
    let message: String
}

/// One line of a rename preview: the file's current name and what the mask
/// renames it to.
struct RenamePair: Identifiable, Hashable {
    var old: String
    var new: String
    var id: String { old }
}

/// One transform rule, as the backend's TransformRuleDto. snake_case keys are
/// the property names, since the ABI does not convert them.
struct TransformRule: Encodable {
    var kind: String
    var from = ""
    var to = ""
    var regex = false
    var whole_word = false
    var case_sensitive = false
    var style = ""
    var enabled = true
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

    /// The staged edit map: path → field → new value. Nothing is on disk until
    /// Apply. It drives the table diff for both a hand edit and a staged import.
    private(set) var staged: [String: [Field: String]] = [:]

    /// A whole staged plan from an online import (#300). When set, Apply writes
    /// this plan rather than rebuilding one from `staged` — the plan carries more
    /// than the table's seven columns (isrc, bpm, catalogue, …), and `staged`
    /// only mirrors the visible part of it for the diff.
    private var stagedPlan: JSONValue?
    private var stagedPlanCount = 0

    /// path → the new file name a staged rename gives it, so the File column can
    /// show the rename as a diff the way a tag change shows in its column.
    private(set) var stagedRenames: [String: String] = [:]

    var filter = ""
    var showsOldValues = false

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
        // A hand edit supersedes a pending import or rename: the staging sources
        // must not mix, and Apply follows whichever is current.
        stagedPlan = nil
        stagedPlanCount = 0
        stagedRenames.removeAll()
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
    func search(
        _ source: Source,
        artist: String,
        album: String,
        catalog: String
    ) async -> Result<[Candidate], SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        let box = SessionHandle(raw: session)
        let query = SearchArgs.Query(
            artist: blankToNil(artist),
            album: blankToNil(album),
            catalog_number: blankToNil(catalog)
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

    /// Fetch a release's full tracklist.
    func fetchRelease(_ source: Source, id: String) async -> Result<Release, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }
        let box = SessionHandle(raw: session)
        return await Task.detached(priority: .userInitiated) {
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
        alignment: [Int?]
    ) async -> Result<Int, SearchFailure> {
        guard let session else { return .failure(SearchFailure(message: "No library open")) }

        var ordered: [ImportTrack] = []
        for match in alignment {
            guard let index = match, release.tracks.indices.contains(index) else {
                return .failure(SearchFailure(message: "every file must be matched to a track"))
            }
            ordered.append(importTrack(from: release.tracks[index], albumArtist: release.artist))
        }

        let selection = ImportSelection(
            album: blankToNil(release.title),
            album_artist: blankToNil(release.artist),
            year: release.year.map(String.init),
            genre: release.importGenre,
            tracks: ordered,
            release_id: blankToNil(release.id),
            source: source.rawValue
        )
        let box = SessionHandle(raw: session)
        let result: Result<(JSONValue, [String: [Field: String]], Int), SearchFailure> =
            await Task.detached(priority: .userInitiated) {
                let args = ImportArgs(paths: paths, selection: selection, vinyl_sides_to_disc: false)
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
                var diffs: [String: [Field: String]] = [:]
                for change in parsed.changes {
                    for tagChange in change.tag_changes where Field(rawValue: tagChange.field) != nil {
                        diffs[change.path, default: [:]][Field(rawValue: tagChange.field)!] =
                            tagChange.new ?? ""
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
        let result: Result<(JSONValue, [String: [Field: String]], [String: String], Int), SearchFailure> =
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
                var diffs: [String: [Field: String]] = [:]
                var renames: [String: String] = [:]
                for change in parsed.changes {
                    if let to = change.rename_to { renames[change.path] = baseName(to) }
                    for tag in change.tag_changes where Field(rawValue: tag.field) != nil {
                        diffs[change.path, default: [:]][Field(rawValue: tag.field)!] = tag.new ?? ""
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

    // MARK: - Player

    /// The last status read from the player, or nil when nothing is loaded.
    private(set) var playerStatus: PlayerStatus?

    /// The paths playback walks for gapless advance (the visible rows at the
    /// moment Play was pressed), and the track we have already queued a next for.
    private var playQueue: [String] = []
    private var fedNextFor: String?
    private var polling: Task<Void, Never>?

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
        playerStatus = status

        // Gapless: when the player asks for a next track and one hasn't been fed
        // for the current track yet, queue the following row.
        if status.wantsNext, let current = status.path, fedNextFor != current {
            fedNextFor = current
            if let index = playQueue.firstIndex(of: current), index + 1 < playQueue.count {
                fire(session, "player_set_next", encodeArgs(PathArg(path: playQueue[index + 1])))
            }
        }
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
/// stand having to model the whole `PlanDto`. `@unchecked Sendable`: it holds
/// immutable JSON data (dictionaries, arrays and scalars decoded once), so it is
/// safe to hand a staged plan back from a detached task to the main actor.
private struct JSONValue: Codable, @unchecked Sendable {
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
    }
}

private struct FetchArgs: Encodable {
    let source: String
    let token: String
    let release_id: String
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

/// Decode a plan (as a JSONValue) into the parts the stand reflects — visible
/// tag changes and renames. Re-encodes the opaque value, then reads the shape.
private func decodePlan(_ plan: JSONValue) -> StagedPlanShape? {
    guard let data = try? JSONEncoder().encode(plan) else { return nil }
    return try? JSONDecoder().decode(StagedPlanShape.self, from: data)
}

private func baseName(_ path: String) -> String {
    (path as NSString).lastPathComponent
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
    }

    struct FieldChange: Decodable {
        let field: String
        let old: String?
        let new: String?
    }

    let changes: [FileChange]
}
