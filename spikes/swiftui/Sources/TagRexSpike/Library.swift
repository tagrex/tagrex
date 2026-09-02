// The model side of the stand (#271): one call into the Rust core, decoded.
//
// Read-only by construction — the C ABI it talks to has no write at all, so a
// stand cannot damage a library no matter what the UI does.

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
        let minutes = durationSecs / 60
        let seconds = durationSecs % 60
        return String(format: "%d:%02d", minutes, seconds)
    }
}

private struct LibraryPayload: Decodable {
    var root: String
    var rows: [Track]
    var errors: [String]
}

@Observable
@MainActor
final class Library {
    private(set) var root: URL?
    private(set) var tracks: [Track] = []
    private(set) var errors: [String] = []
    private(set) var isScanning = false

    var filter = ""

    var rootName: String { root?.lastPathComponent ?? "No folder open" }

    /// Substring match over the fields the table shows, plus `field:query`
    /// scoping — the same shorthand the web UI takes (#44).
    var visibleTracks: [Track] {
        let query = filter.trimmingCharacters(in: .whitespaces).lowercased()
        guard !query.isEmpty else { return tracks }

        if let colon = query.firstIndex(of: ":") {
            let field = String(query[query.startIndex..<colon])
            let value = String(query[query.index(after: colon)...])
            if !value.isEmpty, let scope = Self.scopes[field] {
                return tracks.filter { $0[keyPath: scope].lowercased().contains(value) }
            }
        }

        return tracks.filter { track in
            [track.file, track.artist, track.title, track.album]
                .contains { $0.lowercased().contains(query) }
        }
    }

    private static let scopes: [String: KeyPath<Track, String>] = [
        "artist": \Track.artist, "title": \Track.title, "album": \Track.album,
        "year": \Track.year, "genre": \Track.genre, "file": \Track.file,
    ]

    func open(_ folder: URL) async {
        root = folder
        isScanning = true
        defer { isScanning = false }

        let path = folder.path
        let payload: LibraryPayload? = await Task.detached(priority: .userInitiated) {
            guard let raw = tagrex_scan_json(path) else { return nil }
            defer { tagrex_string_free(raw) }

            let json = Data(String(cString: raw).utf8)
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            return try? decoder.decode(LibraryPayload.self, from: json)
        }.value

        tracks = payload?.rows ?? []
        errors = payload?.errors ?? ["the scan returned nothing readable"]
    }
}
