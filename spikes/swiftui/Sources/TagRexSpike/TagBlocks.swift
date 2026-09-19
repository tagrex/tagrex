// The tag-block bar at the top of the editor (#47, #205), mirroring the Tauri
// block line: it says which block the values are read from, offers a "Remove
// <block>" for each spare block the selection carries, and a "Convert…" that
// rewrites the read block as a different kind or ID3v2 revision. Every action
// goes through the normal preview/apply/undo path — nothing reaches disk until
// the change bar's Apply — and a change undo can't fully reverse asks first.

import SwiftUI

@MainActor
struct TagBlocksBar: View {
    let library: Library
    /// The selected tracks — the files the block actions read and write.
    let tracks: [Track]

    @State private var convertOpen = false
    @State private var targets: Library.BlockTargets?
    @State private var convertKind = ""
    @State private var convertRevision = "id3v24"
    @State private var busy = false
    @State private var pendingConfirm: BlockConfirm?

    /// A lossy change held for confirmation: undo can't fully reverse it, so the
    /// user sees what it would drop before it stages.
    private struct BlockConfirm: Identifiable {
        let id = UUID()
        let verb: String
        let label: String
        let message: String
        let preview: Library.BlockPreview
    }

    private var paths: [String] { tracks.map(\.id) }

    var body: some View {
        if readKinds.isEmpty {
            EmptyView()
        } else {
            VStack(alignment: .leading, spacing: 8) {
                Text("Tag blocks")
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.secondary)

                Text(statusText)
                    .font(AppFonts.sans(11))
                    .foregroundStyle(.secondary)

                FlowRow(spacing: 6) {
                    ForEach(strippable, id: \.kind) { spare in
                        Button("Remove \(spare.label)") { removeBlock(kind: spare.kind, label: spare.label) }
                            .buttonStyle(.borderless)
                            .controlSize(.small)
                            .disabled(busy)
                            .help("Strip the \(spare.label) block from the selection")
                    }
                    Button("Convert…") { openConvert() }
                        .buttonStyle(.borderless)
                        .controlSize(.small)
                        .disabled(busy || readKinds.count > 1)
                        .help(readKinds.count > 1
                              ? "The selected files read from different tag blocks — convert them separately"
                              : "Write these tags as a different kind of tag block")
                }

                if convertOpen { convertPicker }
            }
            .alert("Can't be fully undone", isPresented: confirmPresented, presenting: pendingConfirm) { confirm in
                Button(confirm.verb, role: .destructive) { library.commitBlockPreview(confirm.preview) }
                Button("Cancel", role: .cancel) {}
            } message: { confirm in
                Text(confirm.message)
            }
        }
    }

    // MARK: - Convert picker

    @ViewBuilder
    private var convertPicker: some View {
        if let targets {
            HStack(spacing: 6) {
                Text("Write as")
                    .font(AppFonts.sans(11))
                    .foregroundStyle(.secondary)
                Picker("Write as", selection: $convertKind) {
                    ForEach(targets.kinds) { Text($0.label).tag($0.kind) }
                }
                .labelsHidden()
                .pickerStyle(.menu)
                .controlSize(.small)
                .fixedSize()
                if convertKind == "id3v2" {
                    Picker("Revision", selection: $convertRevision) {
                        ForEach(targets.revisions) { Text("ID3v\($0.label)").tag($0.kind) }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .controlSize(.small)
                    .fixedSize()
                }
                Button("Preview") { convertBlock() }
                    .controlSize(.small)
                    .disabled(busy)
                Button("Cancel") { convertOpen = false }
                    .buttonStyle(.borderless)
                    .controlSize(.small)
            }
        }
    }

    private var confirmPresented: Binding<Bool> {
        Binding(get: { pendingConfirm != nil }, set: { if !$0 { pendingConfirm = nil } })
    }

    // MARK: - Derived block state (mirrors the Tauri block line)

    /// The kinds the values are read from — the first `read_from` block per file,
    /// else its first block. Convert needs a single one across the selection.
    private var readKinds: Set<String> {
        var kinds = Set<String>()
        for track in tracks {
            guard let read = readBlock(track) else { continue }
            kinds.insert(read.kind)
        }
        return kinds
    }

    private func readBlock(_ track: Track) -> TagBlock? {
        track.tagBlocks.first { $0.readFrom } ?? track.tagBlocks.first
    }

    /// The label of the read block when the whole selection reads one kind.
    private var readLabel: String? {
        guard readKinds.count == 1 else { return nil }
        return tracks.compactMap(readBlock).first?.label
    }

    /// The spare blocks the selection carries — every block that isn't the one
    /// being read — keyed by kind so two files sharing a spare offer one button.
    private var strippable: [(kind: String, label: String, files: Int)] {
        var byKind: [String: (label: String, files: Int)] = [:]
        var order: [String] = []
        for track in tracks {
            guard let read = readBlock(track), track.tagBlocks.count > 1 else { continue }
            for block in track.tagBlocks where block.kind != read.kind {
                if byKind[block.kind] == nil { order.append(block.kind) }
                let existing = byKind[block.kind]
                byKind[block.kind] = (block.label, (existing?.files ?? 0) + 1)
            }
        }
        return order.map { (kind: $0, label: byKind[$0]!.label, files: byKind[$0]!.files) }
    }

    private var statusText: String {
        if readKinds.count > 1 { return "The selected files read from different tag blocks." }
        let read = readLabel ?? "—"
        return strippable.isEmpty ? "Carrying \(read)." : "Reading \(read); also carrying spares."
    }

    // MARK: - Actions

    private func removeBlock(kind: String, label: String) {
        busy = true
        Task {
            let result = await library.previewRemoveTagBlock(kind: kind, paths: paths)
            busy = false
            handle(result, verb: "Remove", label: label)
        }
    }

    private func openConvert() {
        guard let from = readKinds.first else { return }
        busy = true
        Task {
            let fetched = await library.tagBlockTargets(paths: paths)
            busy = false
            guard let fetched, !fetched.kinds.isEmpty else {
                library.note("The selection has no tag-block kind in common")
                return
            }
            targets = fetched
            convertKind = from
            convertOpen = true
        }
    }

    private func convertBlock() {
        guard let from = readKinds.first else { return }
        let revision = convertKind == "id3v2" ? convertRevision : nil
        busy = true
        Task {
            let result = await library.previewConvertTagBlock(
                from: from, to: convertKind, revision: revision, paths: paths)
            busy = false
            if case .success(let preview) = result, preview.count > 0 {
                convertOpen = false
            }
            handle(result, verb: "Convert", label: convertLabel)
        }
    }

    private var convertLabel: String {
        targets?.kinds.first { $0.kind == convertKind }?.label ?? convertKind
    }

    /// Common outcome handling: nothing carries → a note; a lossy change → a
    /// confirmation naming what it drops; otherwise stage it straight away.
    private func handle(_ result: Result<Library.BlockPreview, SearchFailure>, verb: String, label: String) {
        switch result {
        case .success(let preview):
            if preview.count == 0 {
                library.note("No selected file carries that block")
                return
            }
            if preview.inexact {
                pendingConfirm = BlockConfirm(
                    verb: verb, label: label, message: lossMessage(verb: verb, label: label, preview: preview),
                    preview: preview)
            } else {
                library.commitBlockPreview(preview)
            }
        case .failure(let failure):
            library.note(failure.message)
        }
    }

    /// What a lossy change would drop, spelled out before it stages: undo rebuilds
    /// a block from what the model can express, so a frame it can't — a cue point,
    /// a rating — would not come back.
    private func lossMessage(verb: String, label: String, preview: Library.BlockPreview) -> String {
        var parts: [String] = []
        if !preview.lostFields.isEmpty {
            parts.append(preview.lostFields.map { EditorFields.label(for: $0) }.joined(separator: ", "))
        }
        if preview.lostPictures { parts.append("artwork") }
        let files = "\(preview.count) file\(preview.count == 1 ? "" : "s")"
        let base = "\(verb) \(label) on \(files)? Undo can't fully restore it"
        return parts.isEmpty ? "\(base)." : "\(base) — it would drop: \(parts.joined(separator: ", "))."
    }
}

/// A minimal wrapping row: lays its children left to right and wraps to the next
/// line when the width runs out — for the strip/convert buttons, which vary in
/// number and length. SwiftUI has no built-in flow layout before the `Layout`
/// API, and this keeps the buttons from clipping in the 432pt panel.
struct FlowRow: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var rowWidth: CGFloat = 0
        var rowHeight: CGFloat = 0
        var totalHeight: CGFloat = 0
        var totalWidth: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if rowWidth > 0, rowWidth + spacing + size.width > maxWidth {
                totalHeight += rowHeight + spacing
                totalWidth = max(totalWidth, rowWidth)
                rowWidth = 0
                rowHeight = 0
            }
            rowWidth += (rowWidth > 0 ? spacing : 0) + size.width
            rowHeight = max(rowHeight, size.height)
        }
        totalHeight += rowHeight
        totalWidth = max(totalWidth, rowWidth)
        return CGSize(width: min(totalWidth, maxWidth), height: totalHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > bounds.minX, x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), anchor: .topLeading, proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}
