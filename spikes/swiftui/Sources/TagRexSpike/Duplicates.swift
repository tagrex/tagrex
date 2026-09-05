// The Duplicates panel (#304): a read-only scan of the whole library, grouped by
// a chosen criterion. Nothing here changes files — it is a report.

import SwiftUI

@MainActor
struct DuplicatesPanel: View {
    let library: Library

    /// criterion key → label, in menu order.
    private static let criteria: [(String, String)] = [
        ("artist_title", "Artist + Title"),
        ("album_track", "Album + Track"),
        ("duration", "Duration"),
        ("size", "File size"),
        ("hash", "Identical bytes"),
    ]

    @State private var criterion = "artist_title"
    @State private var groups: [DuplicateGroup] = []
    @State private var error: String?
    @State private var isScanning = false
    @State private var scanned = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            form
            Divider()
            results
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .task(id: criterion) { await scan() }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Text("Duplicates by")
                    .foregroundStyle(.secondary)
                Picker("", selection: $criterion) {
                    ForEach(Self.criteria, id: \.0) { Text($0.1).tag($0.0) }
                }
                .labelsHidden()
                if isScanning { ProgressView().controlSize(.small) }
            }
            Text(summary)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .font(AppFonts.body)
        .padding(12)
    }

    private var summary: String {
        if !scanned { return "Scanning the whole library, not just the selection." }
        let files = groups.reduce(0) { $0 + $1.files.count }
        return groups.isEmpty
            ? "No duplicates under this rule."
            : "\(groups.count) group(s), \(files) files."
    }

    @ViewBuilder
    private var results: some View {
        if let error {
            ContentUnavailableView {
                Label("Scan failed", systemImage: "exclamationmark.triangle")
            } description: {
                Text(error)
            }
        } else if scanned && groups.isEmpty {
            ContentUnavailableView(
                "No duplicates",
                systemImage: "square.on.square.dashed",
                description: Text("Nothing in the library matches under this rule.")
            )
        } else {
            List {
                ForEach(groups) { group in
                    Section {
                        ForEach(group.files) { file in
                            row(file)
                        }
                    } header: {
                        Text(group.key).font(.caption)
                    }
                }
            }
            .listStyle(.inset)
        }
    }

    private func row(_ file: DuplicateFile) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(file.file)
                .font(AppFonts.mono)
                .lineLimit(1)
            HStack(spacing: 6) {
                Text([file.artist, file.title].filter { !$0.isEmpty }.joined(separator: " — "))
                    .lineLimit(1)
                Spacer()
                Text(file.duration).monospacedDigit()
                Text("·")
                Text(file.size).monospacedDigit()
                if let kbps = file.bitrateKbps {
                    Text("· \(kbps)k").monospacedDigit()
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(.vertical, 1)
    }

    private func scan() async {
        isScanning = true
        error = nil
        switch await library.findDuplicates(criterion: criterion) {
        case .success(let found): groups = found
        case .failure(let failure): groups = []; error = failure.message
        }
        isScanning = false
        scanned = true
    }
}
