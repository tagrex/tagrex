// The Export panel (#305): write a playlist, CUE, CSV, HTML, XML or report of
// the chosen files into the library folder. Read-only for the audio — it only
// adds an export file.

import SwiftUI

@MainActor
struct ExportPanel: View {
    let library: Library
    let selection: Set<Track.ID>

    /// format key → (label, default file name).
    private static let formats: [(String, String, String)] = [
        ("playlist", "Playlist", "playlist.m3u8"),
        ("cue", "CUE", "playlist.cue"),
        ("csv", "CSV", "tracks.csv"),
        ("html", "HTML", "tracks.html"),
        ("xml", "XML", "tracks.xml"),
        ("report", "Report", "report.txt"),
    ]

    @State private var format = "playlist"
    @State private var fileName = "playlist.m3u8"
    @State private var mask = "%artist% - %title%"
    /// Split a playlist export into one file per group (#46): "" = one playlist,
    /// else "folder" or "album". Only meaningful for the playlist format.
    @State private var split = ""
    /// The mask each split playlist is named from.
    @State private var splitMask = "%albumartist% - %album%"
    @State private var result: String?
    @State private var splitCount: Int?
    @State private var error: String?
    @State private var isExporting = false

    private var splitting: Bool { format == "playlist" && !split.isEmpty }

    private var paths: [String] {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? library.visibleTracks.map(\.id) : selected
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            labeled("Format") {
                Picker("", selection: $format) {
                    ForEach(Self.formats, id: \.0) { Text($0.1).tag($0.0) }
                }
                .labelsHidden()
                .onChange(of: format) { _, new in
                    fileName = Self.formats.first { $0.0 == new }?.2 ?? fileName
                    result = nil
                }
            }

            if format == "playlist" {
                labeled("Split") {
                    Picker("", selection: $split) {
                        Text("One playlist").tag("")
                        Text("By folder").tag("folder")
                        Text("By album").tag("album")
                    }
                    .labelsHidden()
                    .onChange(of: split) { _, _ in result = nil; splitCount = nil }
                }
            }

            if format == "report" {
                labeled("Mask") {
                    TextField("%artist% - %title%", text: $mask)
                        .textFieldStyle(.roundedBorder)
                        .font(AppFonts.mono)
                }
            }

            if splitting {
                labeled("Name mask") {
                    TextField("%albumartist% - %album%", text: $splitMask)
                        .textFieldStyle(.roundedBorder)
                        .font(AppFonts.mono)
                }
            } else {
                labeled("File name") {
                    TextField("name", text: $fileName).textFieldStyle(.roundedBorder)
                }
            }

            Text(splitting
                 ? "One playlist per \(split) from \(scopeLabel), named by the mask. Your audio files are not modified."
                 : "Written into the open folder — \(scopeLabel). Your audio files are not modified.")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack {
                Spacer()
                Button {
                    run()
                } label: {
                    if isExporting {
                        ProgressView().controlSize(.small)
                    } else {
                        Label("Export", systemImage: "square.and.arrow.up")
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(isExporting || (splitting
                    ? splitMask.trimmingCharacters(in: .whitespaces).isEmpty
                    : fileName.trimmingCharacters(in: .whitespaces).isEmpty))
            }

            if let error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.red)
            } else if let splitCount {
                Label("Wrote \(splitCount) playlist(s)", systemImage: "checkmark.circle")
                    .font(.caption)
                    .foregroundStyle(.green)
            } else if let result {
                Label("Wrote \((result as NSString).lastPathComponent)", systemImage: "checkmark.circle")
                    .font(.caption)
                    .foregroundStyle(.green)
            }

            Spacer()
        }
        .font(AppFonts.body)
        .padding(14)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    private var scopeLabel: String {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? "all \(paths.count) file(s)" : "\(paths.count) selected"
    }

    private func labeled<Content: View>(_ label: String, @ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: 10) {
            Text(label).frame(width: 76, alignment: .leading).foregroundStyle(.secondary)
            content()
        }
    }

    private func run() {
        isExporting = true
        error = nil
        result = nil
        splitCount = nil
        Task {
            if splitting {
                switch await library.exportPlaylists(grouping: split, nameMask: splitMask, paths: paths) {
                case .success(let written): splitCount = written.count
                case .failure(let failure): error = failure.message
                }
            } else {
                switch await library.export(format: format, fileName: fileName, mask: mask, paths: paths) {
                case .success(let path): result = path
                case .failure(let failure): error = failure.message
                }
            }
            isExporting = false
        }
    }
}
