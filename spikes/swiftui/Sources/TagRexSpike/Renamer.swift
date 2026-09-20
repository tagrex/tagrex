// The Renamer panel (#302): rename files in place from a mask, or reorganise
// them into folders under a destination (#37). Both build a plan and flow
// through the same change-plan bar as everything else — staged, shown as a diff,
// written only on Apply.

import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct RenamerPanel: View {
    let library: Library
    let selection: Set<Track.ID>

    private enum RenamerMode: String, CaseIterable, Identifiable {
        case rename, move
        var id: Self { self }
        var title: String { self == .rename ? "Rename" : "Move into folders" }
    }

    @State private var renamerMode: RenamerMode = .rename

    @AppStorage("renamer.mask") private var mask = "%artist% - %title%"
    // A folder pattern: separators become folders under the destination.
    @AppStorage("renamer.moveMask") private var moveMask = "%albumartist%/%album%/%track% - %title%"
    @State private var destination: String?
    @State private var copy = false
    @State private var prune = false
    @State private var choosingDestination = false

    @State private var pairs: [RenamePair] = []
    @State private var error: String?
    @State private var isStaging = false

    /// The files the action runs over: the selection, or every visible row when
    /// nothing is selected.
    private var paths: [String] {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? library.visibleTracks.map(\.id) : selected
    }

    /// Re-preview when the active mask, mode, destination or target set changes.
    private var refreshKey: String {
        let activeMask = renamerMode == .rename ? mask : moveMask
        return [
            renamerMode.rawValue, activeMask, destination ?? "", String(copy),
            String(paths.count), paths.first ?? "", paths.last ?? "",
        ].joined(separator: "|")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Picker("", selection: $renamerMode) {
                ForEach(RenamerMode.allCases) { Text($0.title.uppercased()).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(12)
            Divider()
            form
            Divider()
            preview
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .task(id: refreshKey) { await refresh() }
        .fileImporter(isPresented: $choosingDestination, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { destination = url.path }
        }
    }

    @ViewBuilder
    private var form: some View {
        if renamerMode == .rename { renameForm } else { moveForm }
    }

    // MARK: - Rename in place

    private var renameForm: some View {
        VStack(alignment: .leading, spacing: 8) {
            fieldLabel("Rename mask")
            // The UI font, not monospace (#351) — it read as out of place
            // against the rest of the interface, and the placeholder reference
            // makes tokens discoverable without needing column alignment.
            HStack(spacing: 6) {
                TextField("%artist% - %title%", text: $mask)
                    .textFieldStyle(.roundedBorder)
                    .font(AppFonts.body)
                MaskPresetButton(mask: $mask)
            }
            Text("Placeholders like %artist%, %title%, %track% — plus $upper(), $pad().")
                .font(.caption)
                .foregroundStyle(.tertiary)
            actionRow(label: scopeLabel, title: "Stage rename") { stageRename() }
        }
        .padding(12)
    }

    // MARK: - Move into folders

    private var moveForm: some View {
        VStack(alignment: .leading, spacing: 8) {
            fieldLabel("Folder pattern")
            HStack(spacing: 6) {
                TextField("%albumartist%/%album%/%track% - %title%", text: $moveMask)
                    .textFieldStyle(.roundedBorder)
                    .font(AppFonts.body)
                MaskPresetButton(mask: $moveMask)
            }
            Text("Slashes become folders under the destination.")
                .font(.caption)
                .foregroundStyle(.tertiary)

            HStack(spacing: 6) {
                Text("Into")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(destination.map { ($0 as NSString).lastPathComponent } ?? "the library folder")
                    .font(AppFonts.sans(11))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Button("Choose…") { choosingDestination = true }
                    .controlSize(.small)
                if destination != nil {
                    Button {
                        destination = nil
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                    }
                    .buttonStyle(.borderless)
                    .help("Reset to the library folder")
                }
            }

            HStack(spacing: 10) {
                Picker("", selection: $copy) {
                    Text("Move").tag(false)
                    Text("Copy").tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                Toggle("Prune empty folders", isOn: $prune)
                    .toggleStyle(.checkbox)
                    .controlSize(.small)
                    // A copy empties nothing, so pruning is inert for it.
                    .disabled(copy)
                Spacer()
            }

            actionRow(label: scopeLabel, title: copy ? "Stage copy" : "Stage move") { stageMove() }
        }
        .padding(12)
    }

    // MARK: - Shared form pieces

    private func fieldLabel(_ text: String) -> some View {
        Text(text).font(.caption).fontWeight(.semibold).foregroundStyle(.secondary)
    }

    private func actionRow(label: String, title: String, action: @escaping () -> Void) -> some View {
        HStack {
            Text(label).font(.caption).foregroundStyle(.secondary)
            Spacer()
            Button(action: action) {
                if isStaging { ProgressView().controlSize(.small) } else { Text(title) }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.small)
            .disabled(isStaging || pairs.isEmpty)
        }
    }

    private var scopeLabel: String {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        let base = selected.isEmpty ? "all \(paths.count) file(s)" : "\(paths.count) selected"
        return pairs.isEmpty ? base : "\(pairs.count) of \(base) change"
    }

    // MARK: - Preview

    @ViewBuilder
    private var preview: some View {
        if let error {
            ContentUnavailableView {
                Label(renamerMode == .rename ? "Rename failed" : "Move failed",
                      systemImage: "exclamationmark.triangle")
            } description: {
                Text(error)
            }
        } else if pairs.isEmpty {
            ContentUnavailableView(
                renamerMode == .rename ? "Nothing to rename" : "Nothing to move",
                systemImage: renamerMode == .rename ? "textformat" : "folder",
                description: Text(renamerMode == .rename
                                  ? "This mask leaves every file's name unchanged."
                                  : "This pattern leaves every file where it is.")
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

    // MARK: - Actions

    private func refresh() async {
        // A short delay debounces per-keystroke typing: .task(id:) cancels this
        // before the sleep returns when the mask changes again.
        try? await Task.sleep(for: .milliseconds(250))
        if Task.isCancelled { return }
        error = nil
        let result = renamerMode == .rename
            ? await library.renamePreview(mask: mask, paths: paths)
            : await library.movePreview(mask: moveMask, paths: paths, destination: destination, copy: copy)
        switch result {
        case .success(let found): pairs = found
        case .failure(let failure): pairs = []; error = failure.message
        }
    }

    private func stageRename() {
        isStaging = true
        Task {
            if case .failure(let failure) = await library.stageRename(mask: mask, paths: paths) {
                error = failure.message
            }
            isStaging = false
        }
    }

    private func stageMove() {
        isStaging = true
        Task {
            let result = await library.stageMove(
                mask: moveMask, paths: paths, destination: destination, copy: copy, prune: prune)
            if case .failure(let failure) = result { error = failure.message }
            isStaging = false
        }
    }
}
