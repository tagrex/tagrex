// The window (#271). Layout follows the current web UI one for one — the same
// toolbar order, the same five columns, the same trailing panel and status bar —
// with native metrics and the app's own typefaces where they are bundled.

import SwiftUI
import UniformTypeIdentifiers

enum Mode: String, CaseIterable, Identifiable {
    case tagger, renamer, generator, deduplicator, exporter

    var id: Self { self }

    var title: String {
        switch self {
        case .tagger: "Tagger"
        case .renamer: "Renamer"
        case .generator: "Generator"
        case .deduplicator: "Duplicates"
        case .exporter: "Export"
        }
    }

    var symbol: String {
        switch self {
        case .tagger: "tag"
        case .renamer: "pencil"
        case .generator: "wand.and.stars"
        case .deduplicator: "square.on.square"
        case .exporter: "square.and.arrow.up"
        }
    }
}

@main
@MainActor
struct TagRexSpikeApp: App {
    @State private var library = Library()

    init() { AppFonts.register() }

    var body: some Scene {
        WindowGroup {
            WorkspaceView()
                .environment(library)
                .frame(minWidth: 980, minHeight: 620)
        }
        .defaultSize(width: 1240, height: 760)
        .windowToolbarStyle(.unified)
    }
}

@MainActor
struct WorkspaceView: View {
    @Environment(Library.self) private var library

    @State private var mode: Mode = .tagger
    @State private var selection = Set<Track.ID>()
    @State private var sortOrder = [KeyPathComparator(\Track.file)]
    @State private var showsInspector = true
    @State private var choosingFolder = false

    private var rows: [Track] { library.visibleTracks.sorted(using: sortOrder) }

    private var selected: [Track] {
        rows.filter { selection.contains($0.id) }
    }

    var body: some View {
        @Bindable var library = library

        TrackTable(rows: rows, selection: $selection, sortOrder: $sortOrder)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                StatusBar(total: library.tracks.count, selected: selection.count)
            }
            .inspector(isPresented: $showsInspector) {
                ModePanel(mode: mode, tracks: selected)
                    .inspectorColumnWidth(min: 320, ideal: 380, max: 560)
            }
            .searchable(text: $library.filter, prompt: "Filter — try artist:aphex")
            .toolbar {
                ToolbarItemGroup(placement: .navigation) {
                    Button {
                        choosingFolder = true
                    } label: {
                        Label(library.rootName, systemImage: "folder")
                    }
                    .help("Choose a folder to open")
                }

                ToolbarItem(placement: .principal) {
                    Picker("Tool", selection: $mode) {
                        ForEach(Mode.allCases) { mode in
                            Label(mode.title, systemImage: mode.symbol)
                                .labelStyle(.titleAndIcon)
                                .tag(mode)
                        }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                }

                ToolbarItemGroup {
                    Button {
                    } label: {
                        Label("Undo the last applied batch", systemImage: "arrow.uturn.backward")
                    }
                    .disabled(true)
                    .help("Read-only stand: nothing is ever written, so there is nothing to undo")

                    Button {
                        showsInspector.toggle()
                    } label: {
                        Label("Panel", systemImage: "sidebar.trailing")
                    }
                }
            }
            .fileImporter(isPresented: $choosingFolder, allowedContentTypes: [.folder]) { result in
                guard case .success(let folder) = result else { return }
                Task { await library.open(folder) }
            }
            .navigationTitle("TagRex — read-only stand")
            .task {
                // Opening a folder by hand is a dialog; for screenshots, CI and
                // a quick look at a known library, TAGREX_SPIKE_ROOT skips it.
                guard let path = ProcessInfo.processInfo.environment["TAGREX_SPIKE_ROOT"],
                      !path.isEmpty
                else { return }
                await library.open(URL(fileURLWithPath: path))
            }
    }
}

struct TrackTable: View {
    let rows: [Track]
    @Binding var selection: Set<Track.ID>
    @Binding var sortOrder: [KeyPathComparator<Track>]

    var body: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("File", value: \.file) { track in
                Text(track.file).font(AppFonts.mono)
            }
            .width(min: 180, ideal: 300)

            TableColumn("Artist", value: \.artist) { Text($0.artist).font(AppFonts.body) }
                .width(min: 90, ideal: 150)
            TableColumn("Title", value: \.title) { Text($0.title).font(AppFonts.body) }
                .width(min: 90, ideal: 190)
            TableColumn("Album", value: \.album) { track in
                Text(track.album.isEmpty ? "no album" : track.album)
                    .font(AppFonts.body)
                    .foregroundStyle(track.album.isEmpty ? .tertiary : .primary)
            }
            .width(min: 90, ideal: 160)
            TableColumn("Year", value: \.year) { track in
                Text(track.year).font(AppFonts.body).monospacedDigit()
            }
            .width(56)
        }
        .tableStyle(.inset(alternatesRowBackgrounds: true))
    }
}

struct ModePanel: View {
    let mode: Mode
    let tracks: [Track]

    @State private var subtab = 1

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if mode == .tagger {
                Picker("", selection: $subtab) {
                    Text("Online").tag(0)
                    Text("Editor").tag(1)
                    Text("From name").tag(2)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(12)
            }

            Divider()

            if tracks.isEmpty {
                ContentUnavailableView(
                    "Nothing selected",
                    systemImage: "square.dashed",
                    description: Text("Pick a row to see its tags.")
                )
            } else {
                Form {
                    Section("Tag fields") {
                        field("Artist", tracks.map(\.artist))
                        field("Title", tracks.map(\.title))
                        field("Album", tracks.map(\.album))
                        field("Album artist", tracks.map(\.albumartist))
                        field("Year", tracks.map(\.year))
                        field("Genre", tracks.map(\.genre))
                        field("Track", tracks.map(\.track))
                    }
                    Section("File") {
                        field("Format", tracks.map(\.format))
                        field("Length", tracks.map(\.duration))
                        field("Bitrate", tracks.map { $0.bitrateKbps.map { "\($0) kbps" } ?? "—" })
                    }
                }
                .formStyle(.grouped)
            }

            Spacer(minLength: 0)

            HStack {
                Spacer()
                Text("Read-only — nothing is written")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(12)
        }
    }

    /// One row of the field editor: the shared value, or the app's own
    /// <multiple values> when the selection disagrees.
    private func field(_ label: String, _ values: [String]) -> some View {
        let unique = Set(values)
        let shared = unique.count == 1 ? (unique.first ?? "") : nil

        return LabeledContent(label) {
            Text(shared.map { $0.isEmpty ? "—" : $0 } ?? "<multiple values>")
                .font(AppFonts.body)
                .foregroundStyle(shared == nil ? .secondary : .primary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct StatusBar: View {
    let total: Int
    let selected: Int

    var body: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 10) {
                Image(systemName: "backward.end.fill")
                Image(systemName: "play.fill")
                Image(systemName: "forward.end.fill")
                Divider().frame(height: 14)
                Text("Playback is out of scope for the stand")
                Spacer()
                Text(selected > 0 ? "\(selected) of \(total) selected" : "\(total) tracks")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
        }
        .background(.bar)
    }
}
