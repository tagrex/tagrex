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
    /// Media filter (empty = all), and how many results a page fetches.
    @State private var mediaFilter = ""
    @State private var perPage = 5

    @State private var candidates: [Candidate] = []
    @State private var page = 0
    @State private var hasMore = false
    @State private var error: String?
    @State private var isSearching = false
    /// Per-candidate track/disc/image counts, filled in the background after a
    /// search (the Tauri prefetch, #98). Absent until the release is fetched.
    @State private var counts: [String: ReleaseCounts] = [:]

    /// The media types the filter offers, mirroring the Tauri select.
    private let mediaOptions: [(value: String, label: String)] = [
        ("", "All media"), ("CD", "CD"), ("Vinyl", "Vinyl"), ("LP", "LP"),
        ("Cassette", "Cassette"), ("File", "File"),
    ]

    /// The release being looked at, and which candidate opened it.
    @State private var openID: Candidate.ID?
    @State private var release: Release?
    @State private var isLoadingRelease = false

    /// The alignment of the open release onto the selected files, once run:
    /// one entry per selected file (in table order), the matched track index.
    @State private var alignment: [Int?]?
    @State private var isAligning = false
    @State private var isStaging = false

    /// The chosen label / catalogue-number pair for the open release (#90); the
    /// index into `release.labels`. Reset to the primary when a release opens.
    @State private var labelIndex = 0
    @State private var isEmbedding = false
    @State private var isSavingImages = false
    /// A pending "save images" call the user must confirm because it would
    /// overwrite files already in the folder (#102).
    @State private var saveConflict: SaveConflict?

    /// A save-images call held for confirmation: the urls it would write and the
    /// existing files it would overwrite.
    private struct SaveConflict: Identifiable {
        let id = UUID()
        let path: String
        let urls: [String]
        let names: [String]
    }

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
        .alert("Overwrite artwork?", isPresented: overwritePresented, presenting: saveConflict) { conflict in
            Button("Overwrite", role: .destructive) { confirmSave(conflict) }
            Button("Cancel", role: .cancel) { saveConflict = nil }
        } message: { conflict in
            Text("\(conflict.names.joined(separator: ", ")) already "
                + (conflict.names.count == 1 ? "exists" : "exist")
                + " in that folder.")
        }
    }

    private var overwritePresented: Binding<Bool> {
        Binding(get: { saveConflict != nil }, set: { if !$0 { saveConflict = nil } })
    }

    // MARK: - Query

    private var queryForm: some View {
        VStack(spacing: 8) {
            HStack {
                Text("Source")
                Picker("Source", selection: $source) {
                    ForEach(Source.allCases) { Text($0.label).tag($0) }
                }
                .labelsHidden()
                .pickerStyle(.menu)
                .fixedSize()
                Spacer()
                Picker("Media", selection: $mediaFilter) {
                    ForEach(mediaOptions, id: \.value) { Text($0.label).tag($0.value) }
                }
                .labelsHidden()
                .pickerStyle(.menu)
                .fixedSize()
                .help("Filter results by media type")
            }

            HStack(spacing: 6) {
                TextField("Search a release…", text: $query).onSubmit { run() }
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
        } else if candidates.isEmpty {
            ContentUnavailableView(
                "Nothing found yet",
                systemImage: "magnifyingglass",
                description: Text("Search a source to see its releases.")
            )
        } else {
            VStack(spacing: 0) {
                resultsHeader
                Divider()
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(candidates) { candidate in
                            candidateCard(candidate)
                        }
                        if hasMore {
                            Button {
                                run(reset: false)
                            } label: {
                                if isSearching {
                                    ProgressView().controlSize(.small)
                                } else {
                                    Text("Load more results").font(AppFonts.sans(12))
                                }
                            }
                            .buttonStyle(.borderless)
                            .disabled(isSearching)
                            .padding(.vertical, 6)
                            .frame(maxWidth: .infinity)
                        }
                    }
                    .padding(12)
                }
            }
        }
    }

    /// One release-candidate card: the row in a rounded, outlined container the
    /// way the Tauri `.release-card` is, so cards read as distinct and hold a
    /// stable height regardless of how many are loaded.
    private func candidateCard(_ candidate: Candidate) -> some View {
        let isOpen = openID == candidate.id
        return VStack(spacing: 0) {
            candidateRow(candidate, isOpen: isOpen)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture { toggle(candidate) }
            if isOpen {
                expandedContent(candidate)
            }
        }
        .background(
            RoundedRectangle(cornerRadius: 8)
                .fill(Color(nsColor: .controlBackgroundColor))
        )
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(
            RoundedRectangle(cornerRadius: 8).strokeBorder(Color.cardBorder, lineWidth: 1)
        )
    }

    private var resultsHeader: some View {
        HStack {
            Text("Found \(candidates.count) \(candidates.count == 1 ? "entry" : "entries")")
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
            Text("Show")
                .font(.caption)
                .foregroundStyle(.secondary)
            Picker("Show", selection: $perPage) {
                ForEach([5, 10, 15], id: \.self) { Text("\($0)").tag($0) }
            }
            .labelsHidden()
            .pickerStyle(.menu)
            .fixedSize()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }

    /// A release candidate card, organised the way the Tauri card is
    /// (`cardMarkup`, online.js): a cover spanning the card height, then four
    /// lines — the catalogue number, the artist, the title, and the
    /// country · year · format meta — with a caret.
    private func candidateRow(_ candidate: Candidate, isOpen: Bool) -> some View {
        HStack(alignment: .center, spacing: 10) {
            CandidateCover(library: library, source: source, url: candidate.imageURL)
                .frame(width: 72, height: 72)
                .overlay(alignment: .bottomLeading) {
                    mediaBadge(candidate)
                }
            VStack(alignment: .leading, spacing: 2) {
                releaseBadge(candidate)
                Text(candidate.artist)
                    .font(AppFonts.sans(12, .semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Text(candidate.title)
                    .font(AppFonts.sans(12, .semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                let meta = candidateMeta(candidate)
                if !meta.isEmpty {
                    Text(meta)
                        .font(AppFonts.sans(11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
            // Fill the row so every line measures against the same width — with
            // the column sized to content, a card whose longest line was the meta
            // truncated a *shorter* meta on another card.
            .frame(maxWidth: .infinity, alignment: .leading)
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.secondary)
                .rotationEffect(.degrees(isOpen ? 90 : 0))
        }
    }

    /// The catalogue-number + track-count badge, the way the Tauri card wears it
    /// (#124): the catalogue in an accent-tinted pill, the count beside it.
    @ViewBuilder
    private func releaseBadge(_ candidate: Candidate) -> some View {
        let catalog = candidate.catalogNumber ?? ""
        let count = countLabel(candidate.id)
        // The Tauri `.rel-badge` (#124): one unified pill with a *neutral*
        // border, the catalogue segment on a faint accent fill, the count
        // segment neutral, the two split by that same border colour.
        HStack(spacing: 0) {
            if !catalog.isEmpty {
                Text(catalog)
                    .font(AppFonts.monoSized(11, .semibold))
                    .foregroundStyle(.tint)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 2)
                    .background(.tint.opacity(0.12))
            }
            if let count {
                if !catalog.isEmpty {
                    Rectangle().fill(Color.cardBorder).frame(width: 1)
                }
                Text(count)
                    .font(AppFonts.monoSized(11))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 2)
            }
        }
        .fixedSize()
        .clipShape(RoundedRectangle(cornerRadius: 5))
        .overlay(
            RoundedRectangle(cornerRadius: 5).strokeBorder(Color.cardBorder, lineWidth: 1)
        )
    }

    /// The media-type glyph on the cover — a disc for CD/vinyl, a waveform for a
    /// file — with the disc count when a release spans more than one.
    @ViewBuilder
    private func mediaBadge(_ candidate: Candidate) -> some View {
        let discs = counts[candidate.id]?.discs ?? 1
        HStack(spacing: 2) {
            MediaGlyph(kind: mediaKind(candidate.format), size: 18)
            if discs > 1 {
                Text("×\(discs)")
                    .font(.system(size: 12, weight: .semibold))
                    .lineLimit(1)
                    .fixedSize()
            }
        }
        .fixedSize()
        .padding(.horizontal, 4)
        .padding(.vertical, 2)
        .foregroundStyle(.white)
        .background(.black.opacity(0.45), in: Capsule())
        .padding(4)
    }

    /// The media kind, classified the way the Tauri `mediaKind` does.
    private func mediaKind(_ format: String?) -> MediaKind {
        let f = (format ?? "").lowercased()
        func has(_ keys: String...) -> Bool { keys.contains { f.contains($0) } }
        if has("cassette", "tape") { return .cassette }
        if has("vinyl", "lp", "ep", "7\"", "10\"", "12\"", "shellac") { return .vinyl }
        if has("sacd", "hdcd", "cdr", "compact disc", "cd") { return .cd }
        if has("file", "flac", "mp3", "wav", "aac", "digital", "download", "streaming") { return .digital }
        return .generic
    }

    /// The prefetched track/disc count for the card's first line, once known:
    /// "15 tracks", or "30 tracks · 2 discs" for a multi-disc release.
    private func countLabel(_ id: String) -> String? {
        guard let count = counts[id] else { return nil }
        let tracks = "\(count.tracks) track\(count.tracks == 1 ? "" : "s")"
        return count.discs > 1 ? "\(tracks) · \(count.discs) discs" : tracks
    }

    /// The candidate's meta line: country · year · format, the parts present.
    private func candidateMeta(_ candidate: Candidate) -> String {
        [candidate.country, candidate.year.map(String.init), candidate.format]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }

    // MARK: - Release (inline expansion)

    /// Open the tapped card, or collapse it when it is already open — an
    /// accordion, one release at a time, the way the Tauri card expands in place.
    private func toggle(_ candidate: Candidate) {
        if openID == candidate.id {
            openID = nil
            release = nil
            alignment = nil
        } else {
            open(candidate)
        }
    }

    @ViewBuilder
    private func expandedContent(_ candidate: Candidate) -> some View {
        Divider()
        if let release {
            importControls(release)
            if (release.labels?.count ?? 0) > 1 {
                labelPicker(release)
            }
            Divider()
            tracklist(release)
        } else {
            HStack {
                Spacer()
                ProgressView().controlSize(.small).padding(12)
                Spacer()
            }
        }
    }

    /// The import row: Auto-match to re-align the release onto the selection,
    /// Embed cover and Save artwork (#102, #207), the match summary, and Stage
    /// import.
    @ViewBuilder
    private func importControls(_ release: Release) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Button("Auto-match") { Task { await align(release) } }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .disabled(selectedPaths.isEmpty || isAligning)
                    .help("Reorder the selected files to line up with this tracklist")
                embedButton(release)
                saveImagesControl(release)
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
            importStatus
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    /// "Embed cover" — stage a plan that writes this release's cover into the
    /// selected files (#207). Disabled without a cover or a selection.
    @ViewBuilder
    private func embedButton(_ release: Release) -> some View {
        Button {
            embedCover(release)
        } label: {
            if isEmbedding {
                ProgressView().controlSize(.small)
            } else {
                Text("Embed cover")
            }
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .disabled(isEmbedding || selectedPaths.isEmpty || (release.coverImageURL ?? "").isEmpty)
        .help("Embed this release's cover into the selected files")
    }

    /// Save this release's artwork to disk next to the selected tracks (#102): a
    /// plain button for a single image, a menu offering "all N" when there are
    /// more. The primary image's resolution rides in the tooltip.
    @ViewBuilder
    private func saveImagesControl(_ release: Release) -> some View {
        let images = release.images ?? []
        let title = saveArtTooltip(release)
        if images.count <= 1 {
            Button {
                saveImages(release, all: false)
            } label: {
                saveLabel(count: images.count)
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
            .disabled(isSavingImages || selectedPaths.isEmpty || images.isEmpty)
            .help(title)
        } else {
            Menu {
                Button("Save as folder.jpg") { saveImages(release, all: false) }
                Button("Save all \(images.count) images") { saveImages(release, all: true) }
            } label: {
                saveLabel(count: images.count)
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .controlSize(.small)
            .disabled(isSavingImages || selectedPaths.isEmpty)
            .help(title)
        }
    }

    @ViewBuilder
    private func saveLabel(count: Int) -> some View {
        if isSavingImages {
            ProgressView().controlSize(.small)
        } else {
            HStack(spacing: 4) {
                Image(systemName: "photo")
                if count > 1 {
                    Text("\(count)").font(AppFonts.monoSized(10))
                }
            }
        }
    }

    /// The tooltip on the save-artwork control: what it writes and, for the
    /// primary image, its resolution when the provider states it.
    private func saveArtTooltip(_ release: Release) -> String {
        let images = release.images ?? []
        var text = "Save this release's artwork next to the selected tracks"
        if let primary = images.first, primary.width > 0, primary.height > 0 {
            text += " — front cover \(primary.width)×\(primary.height)"
        }
        if images.count > 1 { text += " · \(images.count) images" }
        return text
    }

    /// Label / catalogue-number picker (#90): a release can list several pairs
    /// (even from one label); the user picks the single one the import writes.
    @ViewBuilder
    private func labelPicker(_ release: Release) -> some View {
        let labels = release.labels ?? []
        HStack(spacing: 6) {
            Text("Label · cat#")
                .font(AppFonts.sans(11))
                .foregroundStyle(.secondary)
            Picker("Label · cat#", selection: $labelIndex) {
                ForEach(Array(labels.enumerated()), id: \.offset) { index, label in
                    Text(label.label).tag(index)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
            .controlSize(.small)
            .help("Which label and catalogue number to write")
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 6)
    }

    @ViewBuilder
    private var importStatus: some View {
        if selectedPaths.isEmpty {
            Text("Select files in the table to import this release onto.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else if isAligning {
            HStack(spacing: 6) {
                ProgressView().controlSize(.small)
                Text("Aligning…").font(.caption).foregroundStyle(.secondary)
            }
        } else if let alignment {
            let matched = alignment.compactMap { $0 }.count
            Text("\(matched) of \(selectedPaths.count) selected file(s) matched")
                .font(.caption)
                .foregroundStyle(matched == selectedPaths.count
                                 ? AnyShapeStyle(.secondary)
                                 : AnyShapeStyle(.orange))
        }
    }

    // A release-candidate cover: fetched once over the bridge and cached there,
    // a placeholder until it arrives or if the source carries no art.
    /// The cover art. It fills whatever frame the caller gives — the card sizes
    /// it to the row's full height (a square), the release header to a fixed box.
    private struct CandidateCover: View {
        let library: Library
        let source: Source
        let url: String?
        @State private var image: NSImage?

        var body: some View {
            ZStack {
                RoundedRectangle(cornerRadius: 4).fill(.quaternary)
                if let image {
                    // Fit by the longer side: a square cover fills the box, a
                    // rectangular one sits whole inside it, never cropped.
                    Image(nsImage: image).resizable().scaledToFit()
                } else {
                    Image(systemName: "opticaldisc")
                        .foregroundStyle(.tertiary)
                        .font(.system(size: 18))
                }
            }
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

    // MARK: - Length match against the selection (#188)
    // A release card says what each track should run to; the table knows what the
    // selected files really run to. The difference tells a CD rip filed under a
    // vinyl catalogue number from the vinyl itself — and it has to read BEFORE an
    // import, because after one the wrong lengths are already written as tags.
    // Both sides are in memory, so this is arithmetic, not I/O, and it follows the
    // selection as it changes (the view recomputes when `selection` does).

    /// Within this many seconds two lengths are the same recording; up to `near` a
    /// plausible other master; past `pairLimit` nothing is paired at all.
    private static let matchSecs = 2
    private static let nearSecs = 10
    private static let pairLimitSecs = 120

    /// One release track paired with the selected file closest to it in length.
    private struct DeltaPair {
        let delta: Int  // file seconds minus track seconds
        let path: String
        let secs: Int
    }

    /// Pair each release track with the selected file closest to it in length, one
    /// file to one track, closest pairs claimed first — the same rule as the Tauri
    /// `durationPairs`. Returns the map (track index → pair) and how many selected
    /// files could take part at all.
    private func durationPairs(_ release: Release) -> (pairs: [Int: DeltaPair], files: Int) {
        let byPath = Dictionary(library.tracks.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        var files: [(path: String, secs: Int)] = []
        for path in selectedPaths {
            if let secs = byPath[path]?.durationSecs, secs > 0 {
                files.append((path, Int(secs)))
            }
        }
        guard !files.isEmpty else { return ([:], 0) }
        var pairs: [(i: Int, j: Int, delta: Int)] = []
        for (i, track) in release.tracks.enumerated() {
            guard let ts = track.durationSecs, ts > 0 else { continue }
            for (j, file) in files.enumerated() {
                let delta = file.secs - ts
                if abs(delta) <= Self.pairLimitSecs { pairs.append((i, j, delta)) }
            }
        }
        pairs.sort { abs($0.delta) < abs($1.delta) }
        var byTrack: [Int: DeltaPair] = [:]
        var takenFiles = Set<Int>()
        for p in pairs where byTrack[p.i] == nil && !takenFiles.contains(p.j) {
            byTrack[p.i] = DeltaPair(delta: p.delta, path: files[p.j].path, secs: files[p.j].secs)
            takenFiles.insert(p.j)
        }
        return (byTrack, files.count)
    }

    private enum DeltaBand { case match, near, off }

    private func deltaBand(_ delta: Int) -> DeltaBand {
        let d = abs(delta)
        if d <= Self.matchSecs { return .match }
        if d <= Self.nearSecs { return .near }
        return .off
    }

    private func deltaColor(_ band: DeltaBand) -> Color {
        switch band {
        case .match: .diffAdd
        case .near: .secondary
        case .off: .diffDel
        }
    }

    /// "+2s" / "-1:14" — seconds while they read as seconds, m:ss beyond a minute.
    private func deltaLabel(_ delta: Int) -> String {
        let d = abs(delta)
        let sign = delta > 0 ? "+" : delta < 0 ? "-" : ""
        return "\(sign)\(d < 60 ? "\(d)s" : String(format: "%d:%02d", d / 60, d % 60))"
    }

    /// The "N of M lengths match" tally, and whether it is a hit — nil when there
    /// is nothing to compare (no selected files, or the release states no lengths).
    private func fitTally(_ release: Release, pairs: [Int: DeltaPair], files: Int) -> (text: String, hit: Bool)? {
        let stated = release.tracks.filter { ($0.durationSecs ?? 0) > 0 }.count
        guard files > 0, stated > 0 else { return nil }
        let hits = pairs.values.filter { deltaBand($0.delta) == .match }.count
        return hits > 0 ? ("\(hits) of \(stated) lengths match", true) : ("no lengths match", false)
    }

    /// The tracklist, inline under the card. A `LazyVStack` (not a `List`, which
    /// scrolls inside itself and cannot nest in the results scroll view): each
    /// track a selected file mapped to is ticked, so the mapping reads against
    /// the list.
    /// A short tracklist (≤ 50) is drawn whole so the card just grows and the
    /// results scroll carries it; a long one goes in a bounded, lazy `List` with
    /// its own scrollbar so a 3000-track release neither builds every row up front
    /// nor stretches the card off-screen.
    @ViewBuilder
    private func tracklist(_ release: Release) -> some View {
        let matchedTracks = Set((alignment ?? []).compactMap { $0 })
        let dur = durationPairs(release)
        if let tally = fitTally(release, pairs: dur.pairs, files: dur.files) {
            Text(tally.text)
                .font(AppFonts.sans(11, .semibold))
                .foregroundStyle(tally.hit ? AnyShapeStyle(Color.diffAdd) : AnyShapeStyle(Color.diffDel))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12)
                .padding(.vertical, 4)
        }
        if release.tracks.count <= 50 {
            VStack(spacing: 0) {
                ForEach(Array(release.tracks.enumerated()), id: \.element.id) { index, track in
                    trackRow(index: index, track: track, release: release,
                             matched: matchedTracks, pair: dur.pairs[index])
                        .padding(.horizontal, 12)
                        .padding(.vertical, 3)
                    if index < release.tracks.count - 1 {
                        Divider().padding(.leading, 12)
                    }
                }
            }
            .padding(.bottom, 6)
        } else {
            List {
                ForEach(Array(release.tracks.enumerated()), id: \.element.id) { index, track in
                    trackRow(index: index, track: track, release: release,
                             matched: matchedTracks, pair: dur.pairs[index])
                        .listRowInsets(EdgeInsets(top: 3, leading: 12, bottom: 3, trailing: 12))
                        .listRowBackground(Color.clear)
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .frame(height: 560)
        }
    }

    /// The title · artist for a track — title in the text colour, a differing
    /// track artist appended in the accent colour after a dot.
    private func titleText(_ track: ReleaseTrack, _ release: Release) -> Text {
        let hasArtist = (track.artist.map { !$0.isEmpty && $0 != release.artist }) ?? false
        return Text(track.title).foregroundColor(.primary)
            + (hasArtist ? Text(" · \(track.artist ?? "")").foregroundColor(.appAccent) : Text(""))
    }

    /// A row with its own sideways scroll on the title · artist cell. The length
    /// cell carries the release's own time and, when a selected file pairs with
    /// this track by length (#188), the difference to it — coloured by how close.
    private func trackRow(
        index: Int, track: ReleaseTrack, release: Release, matched: Set<Int>, pair: DeltaPair?
    ) -> some View {
        HStack(spacing: 8) {
            Image(systemName: matched.contains(index) ? "checkmark.circle.fill" : "circle")
                .foregroundStyle(matched.contains(index) ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary))
                .font(.caption)
            Text(track.position)
                .font(AppFonts.monoSized(11))
                .foregroundStyle(.secondary)
                .frame(minWidth: 26, alignment: .leading)
            ScrollView(.horizontal, showsIndicators: false) {
                titleText(track, release)
                    .font(AppFonts.sans(11))
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 4) {
                Text(track.length.isEmpty ? "—" : track.length)
                    .foregroundStyle(.tertiary)
                if let pair {
                    Text(deltaLabel(pair.delta))
                        .foregroundStyle(deltaColor(deltaBand(pair.delta)))
                        .help("\((pair.path as NSString).lastPathComponent) runs \(formatTime(pair.secs))")
                }
            }
            .font(AppFonts.monoSized(11))
        }
    }

    /// m:ss for a whole number of seconds.
    private func formatTime(_ secs: Int) -> String {
        String(format: "%d:%02d", secs / 60, secs % 60)
    }

    // MARK: - Actions

    private func run(reset: Bool = true) {
        guard hasQuery, !isSearching else { return }
        isSearching = true
        error = nil
        if reset {
            release = nil
            openID = nil
            page = 0
            candidates = []
            counts = [:]
        }
        let next = page + 1
        Task {
            let result = await library.search(
                source, query: query, format: mediaFilter, page: next, perPage: perPage
            )
            switch result {
            case .success(let found):
                page = next
                // A full page back suggests there is another to fetch.
                hasMore = found.count >= perPage
                let seen = Set(candidates.map(\.id))
                let added = found.filter { !seen.contains($0.id) }
                candidates.append(contentsOf: added)
                if candidates.isEmpty { error = "No releases matched." }
                prefetchCounts(added)
            case .failure(let failure):
                if reset { candidates = [] }
                error = failure.message
            }
            isSearching = false
        }
    }

    /// Fill each new candidate's counts in the background, four at a time (the
    /// Tauri prefetch pool), so the counts appear without bursting the provider.
    private func prefetchCounts(_ items: [Candidate]) {
        guard !items.isEmpty else { return }
        let src = source
        Task {
            let pool = 4
            var index = 0
            while index < items.count {
                let batch = items[index..<min(index + pool, items.count)]
                await withTaskGroup(of: (String, ReleaseCounts?).self) { group in
                    for candidate in batch {
                        group.addTask {
                            (candidate.id, await library.releaseCounts(src, id: candidate.id))
                        }
                    }
                    for await (id, result) in group {
                        if let result { counts[id] = result }
                    }
                }
                index += pool
            }
        }
    }

    private func open(_ candidate: Candidate) {
        openID = candidate.id
        isLoadingRelease = true
        alignment = nil
        labelIndex = 0
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
                alignment: alignment,
                labelIndex: labelIndex
            )
            if case .failure(let failure) = result {
                error = failure.message
            }
            isStaging = false
        }
    }

    /// Stage the release's cover onto the selected files (#207). Outcomes land in
    /// the status bar (`lastMessage`), leaving the results list in place.
    private func embedCover(_ release: Release) {
        guard !selectedPaths.isEmpty else { return }
        isEmbedding = true
        Task {
            _ = await library.embedCover(
                paths: selectedPaths, coverURL: release.coverImageURL, source: source
            )
            isEmbedding = false
        }
    }

    /// Save the release's primary (or all) image(s) next to the first selected
    /// file (#102). On a conflict the backend reports the existing files and the
    /// call is held for the overwrite alert.
    private func saveImages(_ release: Release, all: Bool) {
        let images = release.images ?? []
        guard let first = selectedPaths.first, !images.isEmpty else { return }
        let urls = all ? images.map(\.url) : [images[0].url]
        isSavingImages = true
        Task {
            let result = await library.saveReleaseImages(
                source: source, path: first, urls: urls, overwrite: false
            )
            isSavingImages = false
            if case .success(let saved) = result {
                if saved.conflicts.isEmpty {
                    library.note("Saved \(saved.written.count) image(s)")
                } else {
                    saveConflict = SaveConflict(path: first, urls: urls, names: saved.conflicts)
                }
            }
        }
    }

    /// The second half of an overwrite: re-run the held save with `overwrite`.
    private func confirmSave(_ conflict: SaveConflict) {
        saveConflict = nil
        isSavingImages = true
        Task {
            let result = await library.saveReleaseImages(
                source: source, path: conflict.path, urls: conflict.urls, overwrite: true
            )
            isSavingImages = false
            if case .success(let saved) = result {
                library.note("Saved \(saved.written.count) image(s)")
            }
        }
    }
}

/// The media kinds the card's glyph distinguishes, mirroring the Tauri set.
enum MediaKind {
    case vinyl, cd, cassette, digital, generic
}


/// The media-type glyph, drawn to match the Tauri SVGs (a 16-unit viewBox) so a
/// record, a CD, a cassette, a digital waveform and a plain note read apart —
/// SF Symbols has no vinyl/cassette pair distinct enough for this.
struct MediaGlyph: View {
    let kind: MediaKind
    let size: CGFloat

    var body: some View {
        Canvas { ctx, sz in
            let u = sz.width / 16.0
            let white = GraphicsContext.Shading.color(.white)
            func circle(_ cx: CGFloat, _ cy: CGFloat, _ r: CGFloat) -> Path {
                Path(ellipseIn: CGRect(x: (cx - r) * u, y: (cy - r) * u, width: 2 * r * u, height: 2 * r * u))
            }
            func rect(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat, _ radius: CGFloat = 0) -> Path {
                Path(roundedRect: CGRect(x: x * u, y: y * u, width: w * u, height: h * u), cornerRadius: radius * u)
            }
            switch kind {
            case .vinyl:
                ctx.stroke(circle(8, 8, 7), with: white, lineWidth: 1 * u)
                ctx.stroke(circle(8, 8, 4.2), with: .color(.white.opacity(0.55)), lineWidth: 0.8 * u)
                ctx.fill(circle(8, 8, 1.4), with: white)
            case .cd:
                ctx.stroke(circle(8, 8, 7), with: white, lineWidth: 1 * u)
                ctx.stroke(circle(8, 8, 2.5), with: white, lineWidth: 1 * u)
            case .cassette:
                ctx.stroke(rect(1.5, 3.5, 13, 9, 1.2), with: white, lineWidth: 1 * u)
                ctx.stroke(circle(5.5, 8, 1.4), with: white, lineWidth: 0.8 * u)
                ctx.stroke(circle(10.5, 8, 1.4), with: white, lineWidth: 0.8 * u)
                ctx.fill(rect(4.5, 10.5, 7, 1.2), with: white)
            case .digital:
                let bars: [(CGFloat, CGFloat, CGFloat)] = [(2.2, 6, 4), (5.2, 3, 10), (8.2, 5, 6), (11.2, 7, 2)]
                for (x, y, h) in bars {
                    ctx.fill(rect(x, y, 1.6, h, 0.8), with: white)
                }
            case .generic:
                ctx.fill(circle(6, 11.5, 2.2), with: white)
                ctx.fill(rect(7.9, 3, 1.3, 8.5), with: white)
                var flag = Path()
                flag.move(to: CGPoint(x: 8.2 * u, y: 3.2 * u))
                flag.addQuadCurve(to: CGPoint(x: 12.2 * u, y: 7 * u), control: CGPoint(x: 12.2 * u, y: 3.8 * u))
                ctx.stroke(flag, with: white, lineWidth: 1.3 * u)
            }
        }
        .frame(width: size, height: size)
    }
}
