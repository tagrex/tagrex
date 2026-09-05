// The Online panel (#299): search a source, browse the release candidates, and
// look at a release's tracklist. Search and look only — applying a release to
// the selected files (auto-align, then a staged import) is the next step.

import SwiftUI

@MainActor
struct OnlinePanel: View {
    let library: Library
    /// The rows selected in the table — the files an import writes onto.
    let selection: Set<Track.ID>

    @State private var source: Source = .discogs
    /// One free-text query, the way the Tauri panel searches (#97): a preset
    /// fills it from the selection, or it is typed by hand.
    @State private var query = ""

    @State private var candidates: [Candidate] = []
    @State private var error: String?
    @State private var isSearching = false

    /// The release being looked at, and which candidate opened it.
    @State private var openID: Candidate.ID?
    @State private var release: Release?
    @State private var isLoadingRelease = false

    /// The alignment of the open release onto the selected files, once run:
    /// one entry per selected file (in table order), the matched track index.
    @State private var alignment: [Int?]?
    @State private var isAligning = false
    @State private var isStaging = false

    /// Selected file paths in table order — the order the import maps tracks to.
    private var selectedPaths: [String] {
        library.tracks.map(\.id).filter(selection.contains)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            queryForm
            Divider()
            results
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .onChange(of: selection) { _, _ in
            if let release { Task { await align(release) } }
        }
    }

    // MARK: - Query

    private var queryForm: some View {
        VStack(spacing: 8) {
            HStack {
                Text("Source")
                Spacer()
                Picker("Source", selection: $source) {
                    ForEach(Source.allCases) { Text($0.label).tag($0) }
                }
                .labelsHidden()
                .pickerStyle(.menu)
                .fixedSize()
            }

            HStack(spacing: 6) {
                TextField("Search a release…", text: $query).onSubmit(run)
                Menu {
                    presetItems
                } label: {
                    Image(systemName: "sparkles")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help("Build a query from the selection")
            }

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
        !query.trimmingCharacters(in: .whitespaces).isEmpty
    }

    // MARK: - Query presets (#97)

    /// The track a preset draws from: the first selected row, else the first row.
    private var presetTrack: Track? {
        library.tracks.first { selection.contains($0.id) } ?? library.tracks.first
    }

    /// The distinct queries the selection can build, each labelled by where it
    /// came from — two sources that yield the same text collapse into one offer.
    private var presetOffers: [(text: String, labels: String)] {
        guard let track = presetTrack else { return [] }
        let raw: [(String, String)] = [
            ("Folder name", searchable(folderName(track.path))),
            ("File name", searchable(baseName(track.path))),
            ("Album", track.album.trimmingCharacters(in: .whitespaces)),
            ("Artist + Title",
             [track.artist, track.title].filter { !$0.isEmpty }.joined(separator: " ")),
        ]
        var order: [String] = []
        var byText: [String: [String]] = [:]
        for (label, text) in raw where !text.isEmpty {
            if byText[text] == nil { order.append(text) }
            byText[text, default: []].append(label)
        }
        return order.map { (text: $0, labels: byText[$0]!.joined(separator: " · ")) }
    }

    @ViewBuilder
    private var presetItems: some View {
        if presetOffers.isEmpty {
            Text("Select a track to build a query from").disabled(true)
        } else {
            ForEach(presetOffers, id: \.text) { offer in
                Button {
                    query = offer.text
                    run()
                } label: {
                    Text("\(offer.text)  —  \(offer.labels)")
                }
            }
        }
    }

    /// A folder name off a path: the last path component of its parent.
    private func folderName(_ path: String) -> String {
        let parent = (path as NSString).deletingLastPathComponent
        return (parent as NSString).lastPathComponent
    }

    /// A file name without its extension.
    private func baseName(_ path: String) -> String {
        ((path as NSString).lastPathComponent as NSString).deletingPathExtension
    }

    /// A disk name made searchable (#158): underscores are how downloaded music
    /// spells spaces, and a provider asked for `a_b_c` matches nothing. Dots are
    /// left alone — they carry meaning in real titles (`Vol. 2`, `M.I.A.`).
    private func searchable(_ name: String) -> String {
        name.replacingOccurrences(of: "_", with: " ")
            .replacingOccurrences(of: "  ", with: " ")
            .trimmingCharacters(in: .whitespaces)
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
        HStack(spacing: 10) {
            CandidateCover(library: library, source: source, url: candidate.imageURL)
            VStack(alignment: .leading, spacing: 2) {
                Text(candidate.title).fontWeight(.medium)
                Text(candidate.artist).foregroundStyle(.secondary)
                if !candidate.detail.isEmpty {
                    Text(candidate.detail)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
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

            HStack(alignment: .top, spacing: 10) {
                CandidateCover(library: library, source: source, url: release.coverImageURL, size: 64)
                VStack(alignment: .leading, spacing: 2) {
                    Text(release.title).font(.headline)
                    Text([release.artist, release.year.map(String.init), release.country]
                        .compactMap { $0 }
                        .filter { !$0.isEmpty }
                        .joined(separator: " · "))
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 8)

            importBand(release)

            Divider()

            releaseBody(release)
        }
        .overlay {
            if isLoadingRelease { ProgressView() }
        }
    }

    /// The import controls: how the release aligned to the selected files, and
    /// the button that stages it.
    @ViewBuilder
    private func importBand(_ release: Release) -> some View {
        if selectedPaths.isEmpty {
            Text("Select files in the table to import this release onto.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .padding(.bottom, 8)
        } else {
            HStack(spacing: 8) {
                if isAligning {
                    ProgressView().controlSize(.small)
                    Text("Aligning…").font(.caption).foregroundStyle(.secondary)
                } else if let alignment {
                    let matched = alignment.compactMap { $0 }.count
                    Text("Matched \(matched) of \(selectedPaths.count) file(s)")
                        .font(.caption)
                        .foregroundStyle(matched == selectedPaths.count
                                         ? AnyShapeStyle(.secondary)
                                         : AnyShapeStyle(.orange))
                }
                Spacer()
                Button {
                    stageImport(release)
                } label: {
                    if isStaging {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Stage import")
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .disabled(isStaging || !canStage)
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 8)
        }
    }

    // A release-candidate cover: fetched once over the bridge and cached there,
    // a placeholder until it arrives or if the source carries no art.
    private struct CandidateCover: View {
        let library: Library
        let source: Source
        let url: String?
        var size: CGFloat = 40
        @State private var image: NSImage?

        var body: some View {
            ZStack {
                RoundedRectangle(cornerRadius: 4).fill(.quaternary)
                if let image {
                    Image(nsImage: image).resizable().scaledToFill()
                } else {
                    Image(systemName: "opticaldisc")
                        .foregroundStyle(.tertiary)
                        .font(.system(size: size * 0.4))
                }
            }
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: 4))
            .task(id: url) {
                image = nil
                guard let url else { return }
                if let data = await library.fetchImage(source, url: url) {
                    image = NSImage(data: data)
                }
            }
        }
    }

    /// Every selected file matched a track — the only case this first cut stages,
    /// since the import maps tracks to files by position.
    private var canStage: Bool {
        guard let alignment, !selectedPaths.isEmpty else { return false }
        return alignment.count == selectedPaths.count && alignment.allSatisfy { $0 != nil }
    }

    /// The tracklist. When aligned, each track that a file mapped to is ticked,
    /// so the mapping is visible against the list itself.
    @ViewBuilder
    private func releaseBody(_ release: Release) -> some View {
        let matchedTracks = Set((alignment ?? []).compactMap { $0 })
        List(Array(release.tracks.enumerated()), id: \.element.id) { index, track in
            HStack(spacing: 8) {
                Image(systemName: matchedTracks.contains(index) ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(matchedTracks.contains(index) ? AnyShapeStyle(.green) : AnyShapeStyle(.quaternary))
                    .font(.caption)
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

    // MARK: - Actions

    private func run() {
        guard hasQuery, !isSearching else { return }
        isSearching = true
        error = nil
        release = nil
        openID = nil
        Task {
            let result = await library.search(source, query: query)
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
        alignment = nil
        Task {
            let result = await library.fetchRelease(source, id: candidate.id)
            switch result {
            case .success(let fetched):
                release = fetched
                await align(fetched)
            case .failure(let failure):
                error = failure.message
            }
            isLoadingRelease = false
        }
    }

    /// Align the release to the selected files. Run when a release opens and
    /// whenever the selection changes while one is open, so the mapping the
    /// import will use is always current.
    private func align(_ release: Release) async {
        guard !selectedPaths.isEmpty else { alignment = nil; return }
        isAligning = true
        defer { isAligning = false }
        switch await library.alignRelease(paths: selectedPaths, release: release) {
        case .success(let matches): alignment = matches
        case .failure(let failure): error = failure.message
        }
    }

    private func stageImport(_ release: Release) {
        guard let alignment, canStage else { return }
        isStaging = true
        Task {
            let result = await library.stageImport(
                paths: selectedPaths,
                release: release,
                source: source,
                alignment: alignment
            )
            if case .failure(let failure) = result {
                error = failure.message
            }
            isStaging = false
        }
    }
}
