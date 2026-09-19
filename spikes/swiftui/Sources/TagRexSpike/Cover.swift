// The cover well at the top of the editor (#56), mirroring the Tauri cover.js:
// it shows the artwork the selection carries — the shared cover when they all
// hold one, a fan of the distinct fronts when they differ, a placeholder when
// there is none — and edits it. Replace picks an image file, Remove strips every
// image, and when a sibling cover file sits beside the tracks on disk it offers
// to embed that. Every action stages a plan the change bar applies, like the rest
// of the editor.

import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct CoverWell: View {
    let library: Library
    let tracks: [Track]

    @State private var summary: Library.CoverSummary?
    @State private var external: Library.CoverArt?
    @State private var loading = false
    @State private var busy = false
    @State private var picking = false

    private var paths: [String] { tracks.map(\.id) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Cover")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)

            HStack(alignment: .top, spacing: 12) {
                coverArtwork
                    .frame(width: 96, height: 96)
                VStack(alignment: .leading, spacing: 6) {
                    Text(statusText)
                        .font(AppFonts.sans(11))
                        .foregroundStyle(.secondary)
                    FlowRow(spacing: 6) {
                        Button("Replace…") { picking = true }
                            .controlSize(.small)
                            .disabled(busy || tracks.isEmpty)
                            .help("Embed an image from disk as the cover")
                        if (summary?.withCover ?? 0) > 0 {
                            Button("Remove") { removeCover() }
                                .controlSize(.small)
                                .disabled(busy)
                                .help("Strip every embedded image from the selection")
                        }
                        if let external {
                            Button("From folder") { setCover(external) }
                                .controlSize(.small)
                                .disabled(busy)
                                .help("Embed the cover file sitting beside these tracks")
                        }
                    }
                }
                Spacer(minLength: 0)
            }
        }
        .task(id: paths) { await load() }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.image]) { result in
            if case .success(let url) = result { pickImage(url) }
        }
    }

    // MARK: - Artwork

    @ViewBuilder
    private var coverArtwork: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 6).fill(.quaternary)
            if loading {
                ProgressView().controlSize(.small)
            } else if let shared = summary?.sharedSet.first, let data = shared.data,
                      let image = NSImage(data: data) {
                Image(nsImage: image).resizable().scaledToFit()
            } else if summary?.distinct == true, let sample = summary?.samples.first,
                      let data = sample.data, let image = NSImage(data: data) {
                // Mixed: show one front with a hint that the selection differs.
                Image(nsImage: image).resizable().scaledToFit().opacity(0.85)
            } else {
                Image(systemName: "photo")
                    .font(.system(size: 26))
                    .foregroundStyle(.tertiary)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.cardBorder, lineWidth: 1))
    }

    private var statusText: String {
        guard let summary else { return "—" }
        if summary.total == 0 { return "No file selected." }
        if summary.withCover == 0 { return "No cover on the selection." }
        if summary.distinct {
            return "\(summary.withCover) of \(summary.total) have covers — they differ."
        }
        let count = summary.sharedSet.count
        let images = count == 1 ? "1 image" : "\(count) images"
        return summary.withCover == summary.total
            ? "All carry the same cover (\(images))."
            : "\(summary.withCover) of \(summary.total) carry the same cover (\(images))."
    }

    // MARK: - Actions

    private func load() async {
        guard !paths.isEmpty else { summary = nil; external = nil; return }
        loading = true
        summary = await library.coverSummary(paths: paths)
        external = await library.externalCover(paths: paths)
        loading = false
    }

    private func pickImage(_ url: URL) {
        busy = true
        Task {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let read = await library.readCoverImage(path: url.path)
            switch read {
            case .success(let cover): await stage(cover)
            case .failure(let failure): library.note(failure.message)
            }
            busy = false
        }
    }

    private func setCover(_ cover: Library.CoverArt) {
        busy = true
        Task { await stage(cover); busy = false }
    }

    private func stage(_ cover: Library.CoverArt) async {
        _ = await library.stageCoverSet(paths: paths, covers: [cover])
    }

    private func removeCover() {
        busy = true
        Task {
            _ = await library.stageCoverRemove(paths: paths)
            busy = false
        }
    }
}
