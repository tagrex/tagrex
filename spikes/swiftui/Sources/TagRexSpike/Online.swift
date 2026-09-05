// The Online panel (#299): search a source, browse the release candidates, and
// look at a release's tracklist. Search and look only — applying a release to
// the selected files (auto-align, then a staged import) is the next step.

import SwiftUI

@MainActor
struct OnlinePanel: View {
    let library: Library

    @State private var source: Source = .musicbrainz
    @State private var artist = ""
    @State private var album = ""
    @State private var catalog = ""

    @State private var candidates: [Candidate] = []
    @State private var error: String?
    @State private var isSearching = false

    /// The release being looked at, and which candidate opened it.
    @State private var openID: Candidate.ID?
    @State private var release: Release?
    @State private var isLoadingRelease = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            queryForm
            Divider()
            results
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    // MARK: - Query

    private var queryForm: some View {
        VStack(spacing: 8) {
            Picker("Source", selection: $source) {
                ForEach(Source.allCases) { Text($0.label).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            TextField("Artist", text: $artist).onSubmit(run)
            TextField("Album", text: $album).onSubmit(run)
            TextField("Catalogue no.", text: $catalog).onSubmit(run)

            HStack {
                Spacer()
                Button {
                    run()
                } label: {
                    if isSearching {
                        ProgressView().controlSize(.small)
                    } else {
                        Label("Search", systemImage: "magnifyingglass")
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(isSearching || !hasQuery)
            }
        }
        .textFieldStyle(.roundedBorder)
        .font(AppFonts.body)
        .padding(12)
    }

    private var hasQuery: Bool {
        [artist, album, catalog].contains { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
    }

    // MARK: - Results

    @ViewBuilder
    private var results: some View {
        if let error {
            ContentUnavailableView {
                Label("Search failed", systemImage: "exclamationmark.triangle")
            } description: {
                Text(error)
            }
        } else if let release {
            releaseView(release)
        } else if candidates.isEmpty {
            ContentUnavailableView(
                "Nothing found yet",
                systemImage: "magnifyingglass",
                description: Text("Search a source to see its releases.")
            )
        } else {
            List(candidates, selection: $openID) { candidate in
                candidateRow(candidate)
                    .contentShape(Rectangle())
                    .onTapGesture { open(candidate) }
            }
            .listStyle(.inset)
        }
    }

    private func candidateRow(_ candidate: Candidate) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(candidate.title).fontWeight(.medium)
            Text(candidate.artist).foregroundStyle(.secondary)
            if !candidate.detail.isEmpty {
                Text(candidate.detail)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .badge(candidate.year.map(String.init) ?? "")
        .padding(.vertical, 2)
    }

    // MARK: - Release

    @ViewBuilder
    private func releaseView(_ release: Release) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Button {
                    self.release = nil
                    openID = nil
                } label: {
                    Label("Results", systemImage: "chevron.left")
                }
                .buttonStyle(.borderless)
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            VStack(alignment: .leading, spacing: 2) {
                Text(release.title).font(.headline)
                Text([release.artist, release.year.map(String.init), release.country]
                    .compactMap { $0 }
                    .filter { !$0.isEmpty }
                    .joined(separator: " · "))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 8)

            Divider()

            List(release.tracks) { track in
                HStack(spacing: 8) {
                    Text(track.position)
                        .font(AppFonts.mono)
                        .foregroundStyle(.secondary)
                        .frame(minWidth: 34, alignment: .leading)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(track.title)
                        if let artist = track.artist, !artist.isEmpty, artist != release.artist {
                            Text(artist).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Spacer()
                    Text(track.length)
                        .font(AppFonts.mono)
                        .foregroundStyle(.tertiary)
                }
            }
            .listStyle(.inset)
        }
        .overlay {
            if isLoadingRelease { ProgressView() }
        }
    }

    // MARK: - Actions

    private func run() {
        guard hasQuery, !isSearching else { return }
        isSearching = true
        error = nil
        release = nil
        openID = nil
        Task {
            let result = await library.search(source, artist: artist, album: album, catalog: catalog)
            switch result {
            case .success(let found):
                candidates = found
                if found.isEmpty { error = "No releases matched." }
            case .failure(let failure):
                candidates = []
                error = failure.message
            }
            isSearching = false
        }
    }

    private func open(_ candidate: Candidate) {
        openID = candidate.id
        isLoadingRelease = true
        Task {
            let result = await library.fetchRelease(source, id: candidate.id)
            switch result {
            case .success(let fetched):
                release = fetched
            case .failure(let failure):
                error = failure.message
            }
            isLoadingRelease = false
        }
    }
}
