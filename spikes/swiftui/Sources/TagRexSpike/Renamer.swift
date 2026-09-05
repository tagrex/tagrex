// The Renamer panel (#302): a mask, a live old → new preview, and a staged
// rename that flows through the same change-plan bar as everything else. Renames
// files in place for now; reorganizing into folders comes later.

import SwiftUI

@MainActor
struct RenamerPanel: View {
    let library: Library
    let selection: Set<Track.ID>

    @State private var mask = "%artist% - %title%"
    @State private var pairs: [RenamePair] = []
    @State private var error: String?
    @State private var isStaging = false

    /// The files the rename runs over: the selection, or every visible row when
    /// nothing is selected.
    private var paths: [String] {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? library.visibleTracks.map(\.id) : selected
    }

    /// Re-preview when the mask or the target set changes.
    private var refreshKey: String {
        "\(mask)|\(paths.count)|\(paths.first ?? "")|\(paths.last ?? "")"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            form
            Divider()
            preview
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .task(id: refreshKey) { await refresh() }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Rename mask")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)
            TextField("%artist% - %title%", text: $mask)
                .textFieldStyle(.roundedBorder)
                .font(AppFonts.mono)
            Text("Placeholders like %artist%, %title%, %track% — plus $upper(), $pad().")
                .font(.caption)
                .foregroundStyle(.tertiary)
            HStack {
                Text(scopeLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button {
                    stage()
                } label: {
                    if isStaging {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Stage rename")
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .disabled(isStaging || pairs.isEmpty)
            }
        }
        .padding(12)
    }

    private var scopeLabel: String {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        let base = selected.isEmpty ? "all \(paths.count) file(s)" : "\(paths.count) selected"
        return pairs.isEmpty ? base : "\(pairs.count) of \(base) change"
    }

    @ViewBuilder
    private var preview: some View {
        if let error {
            ContentUnavailableView {
                Label("Rename failed", systemImage: "exclamationmark.triangle")
            } description: {
                Text(error)
            }
        } else if pairs.isEmpty {
            ContentUnavailableView(
                "Nothing to rename",
                systemImage: "textformat",
                description: Text("This mask leaves every file's name unchanged.")
            )
        } else {
            List(pairs) { pair in
                VStack(alignment: .leading, spacing: 2) {
                    Text(pair.old)
                        .font(AppFonts.mono)
                        .foregroundStyle(.tertiary)
                        .strikethrough()
                    Text(pair.new)
                        .font(AppFonts.mono)
                        .foregroundStyle(.green)
                }
                .lineLimit(1)
                .padding(.vertical, 1)
            }
            .listStyle(.inset)
        }
    }

    private func refresh() async {
        // A short delay debounces per-keystroke typing: .task(id:) cancels this
        // before the sleep returns when the mask changes again.
        try? await Task.sleep(for: .milliseconds(250))
        if Task.isCancelled { return }
        error = nil
        switch await library.renamePreview(mask: mask, paths: paths) {
        case .success(let found): pairs = found
        case .failure(let failure): pairs = []; error = failure.message
        }
    }

    private func stage() {
        isStaging = true
        Task {
            if case .failure(let failure) = await library.stageRename(mask: mask, paths: paths) {
                error = failure.message
            }
            isStaging = false
        }
    }
}
