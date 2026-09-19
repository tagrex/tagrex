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
    @State private var pendingTrash: TrashRequest?

    /// A trash held for confirmation: which files, and how to word the prompt.
    private struct TrashRequest: Identifiable {
        let id = UUID()
        let paths: [String]
        let message: String
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            form
            Divider()
            results
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .task(id: criterion) { await scan() }
        .alert("Move to Trash?", isPresented: trashPresented, presenting: pendingTrash) { request in
            Button("Move to Trash", role: .destructive) { trash(request.paths) }
            Button("Cancel", role: .cancel) {}
        } message: { request in
            Text(request.message)
        }
    }

    private var trashPresented: Binding<Bool> {
        Binding(get: { pendingTrash != nil }, set: { if !$0 { pendingTrash = nil } })
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
        // When there are no groups the empty-state view below says so — here the
        // line just states the scope, so the verdict isn't printed twice (D2).
        if !scanned || groups.isEmpty {
            return "Scanned the whole library, not just the selection."
        }
        let files = groups.reduce(0) { $0 + $1.files.count }
        return "\(groups.count) group(s), \(files) files."
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
                        ForEach(Array(group.files.enumerated()), id: \.element.id) { index, file in
                            row(file, isFirst: index == 0)
                        }
                    } header: {
                        HStack {
                            Text(group.key).font(.caption)
                            Spacer()
                            if group.files.count > 1 {
                                Button("Trash extras") { confirmTrashExtras(group) }
                                    .buttonStyle(.borderless)
                                    .controlSize(.small)
                                    .help("Move every file but the first in this group to the Trash")
                            }
                        }
                    }
                }
            }
            .listStyle(.inset)
        }
    }

    private func row(_ file: DuplicateFile, isFirst: Bool) -> some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(file.file)
                        .font(AppFonts.mono)
                        .lineLimit(1)
                    if isFirst {
                        Text("keep")
                            .font(.caption2)
                            .foregroundStyle(.tint)
                    }
                }
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
            Button {
                pendingTrash = TrashRequest(paths: [file.path], message: "Move \(file.file) to the Trash?")
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .controlSize(.small)
            .help("Move this file to the Trash")
        }
        .padding(.vertical, 1)
    }

    private func confirmTrashExtras(_ group: DuplicateGroup) {
        let extras = Array(group.files.dropFirst()).map(\.path)
        guard !extras.isEmpty else { return }
        pendingTrash = TrashRequest(
            paths: extras,
            message: "Move \(extras.count) file(s) to the Trash, keeping \(group.files[0].file)?")
    }

    private func trash(_ paths: [String]) {
        Task {
            if case .success = await library.trashFiles(paths) { await scan() }
        }
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
