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
    @State private var result: String?
    @State private var error: String?
    @State private var isExporting = false

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

            if format == "report" {
                labeled("Mask") {
                    TextField("%artist% - %title%", text: $mask)
                        .textFieldStyle(.roundedBorder)
                        .font(AppFonts.mono)
                }
            }

            labeled("File name") {
                TextField("name", text: $fileName).textFieldStyle(.roundedBorder)
            }

            Text("Written into the open folder — \(scopeLabel). Your audio files are not modified.")
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
                .disabled(isExporting || fileName.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            if let error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.red)
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
        Task {
            switch await library.export(format: format, fileName: fileName, mask: mask, paths: paths) {
            case .success(let path): result = path
            case .failure(let failure): error = failure.message
            }
            isExporting = false
        }
    }
}
