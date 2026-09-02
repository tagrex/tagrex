// The window (#271). Layout follows the current web UI one for one — the same
// toolbar order, the same five columns, the trailing panel, the status bar, and
// the same discipline: an edit is staged, shown in the table as a diff, and
// written only when Apply is pressed.

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
            WorkspaceView(library: library)
                .frame(minWidth: 980, minHeight: 620)
        }
        .defaultSize(width: 1240, height: 760)
        .windowToolbarStyle(.unified)
    }
}

@MainActor
struct WorkspaceView: View {
    let library: Library

    @State private var mode: Mode = .tagger
    @State private var selection = Set<Track.ID>()
    @State private var sortOrder = [KeyPathComparator(\Track.file)]
    @State private var showsInspector = true
    @State private var choosingFolder = false

    private var rows: [Track] { library.visibleTracks.sorted(using: sortOrder) }

    var body: some View {
        @Bindable var library = library

        TrackTable(
            rows: rows,
            selection: $selection,
            sortOrder: $sortOrder,
            staged: library.staged,
            showsOldValues: library.showsOldValues
        )
            .overlay(alignment: .bottom) {
                if library.hasStagedPlan { ChangePlanBar(library: library) }
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                StatusBar(library: library, total: library.tracks.count, selected: selection.count)
            }
            .inspector(isPresented: $showsInspector) {
                ModePanel(library: library, mode: mode, selection: selection)
                    .inspectorColumnWidth(min: 320, ideal: 380, max: 560)
            }
            .searchable(text: $library.filter, prompt: "Filter — try artist:aphex")
            .toolbar {
                ToolbarItemGroup(placement: .navigation) {
                    Button {
                        choosingFolder = true
                    } label: {
                        Label(library.rootName, systemImage: "folder")
                            .labelStyle(.titleAndIcon)
                    }
                    .help("Choose a folder to open")

                    Button {
                        Task { await library.rescan() }
                    } label: {
                        Label("Re-read", systemImage: "arrow.clockwise")
                    }
                    .disabled(library.root == nil)
                    .help("Re-read the open folder")
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
                        Task { await library.undo() }
                    } label: {
                        Label("Undo the last applied batch", systemImage: "arrow.uturn.backward")
                    }
                    .disabled(library.root == nil || library.isBusy)
                    .help("Undo the last applied batch")

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
            .navigationTitle("TagRex")
            .navigationSubtitle(library.lastMessage)
            .task {
                // Opening a folder by hand is a dialog; for screenshots, CI and
                // a quick look at a known library, TAGREX_SPIKE_ROOT skips it.
                guard let path = ProcessInfo.processInfo.environment["TAGREX_SPIKE_ROOT"],
                      !path.isEmpty
                else { return }
                await library.open(URL(fileURLWithPath: path))
                if let first = rows.first { selection = [first.id] }
            }
    }
}

// MARK: - Table

@MainActor
struct TrackTable: View {
    let rows: [Track]
    @Binding var selection: Set<Track.ID>
    @Binding var sortOrder: [KeyPathComparator<Track>]

    /// Handed in rather than read from the environment. A TableColumn's content
    /// closure escapes the view's environment chain, so an @Environment read
    /// inside a cell trips the "no value for key" assertion the moment the table
    /// re-lays out — which is what a click on a column header does.
    let staged: [String: [Field: String]]
    let showsOldValues: Bool

    var body: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("File", value: \.file) { track in
                Text(track.file).font(AppFonts.mono)
            }
            .width(min: 180, ideal: 300)

            TableColumn("Artist", value: \.artist) { cell($0, .artist) }
                .width(min: 90, ideal: 150)
            TableColumn("Title", value: \.title) { cell($0, .title) }
                .width(min: 90, ideal: 190)
            TableColumn("Album", value: \.album) { cell($0, .album) }
                .width(min: 90, ideal: 160)
            TableColumn("Year", value: \.year) { cell($0, .year) }
                .width(56)
        }
        .tableStyle(.inset(alternatesRowBackgrounds: true))
    }

    private func cell(_ track: Track, _ field: Field) -> DiffCell {
        let stagedValue = staged[track.id]?[field]
        return DiffCell(
            value: stagedValue ?? track.value(for: field),
            old: stagedValue == nil ? nil : track.value(for: field),
            showsOld: showsOldValues
        )
    }
}

/// One cell, in all three states the app knows: unchanged, staged, and staged
/// with the old value beside it. A plain value view — it reads nothing from the
/// environment, which is what keeps the table from crashing when it re-sorts.
struct DiffCell: View {
    /// What the cell shows: the staged value when there is one, else the file's.
    let value: String
    /// The file's own value, present only when the cell is staged.
    let old: String?
    let showsOld: Bool

    var body: some View {
        HStack(spacing: 6) {
            Text(displayed(value))
                .font(AppFonts.body)
                .foregroundStyle(colour)
                .fontWeight(old == nil ? .regular : .semibold)

            if let old, showsOld {
                Text(displayed(old))
                    .font(.caption)
                    .strikethrough()
                    .foregroundStyle(.tertiary)
            }
        }
    }

    private func displayed(_ text: String) -> String {
        text.isEmpty ? "—" : text
    }

    private var colour: AnyShapeStyle {
        if old != nil { return AnyShapeStyle(.green) }
        return value.isEmpty ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.primary)
    }
}

/// The gate. Nothing reaches disk until this bar is used.
@MainActor
struct ChangePlanBar: View {
    let library: Library

    var body: some View {
        @Bindable var library = library

        HStack(spacing: 12) {
            Text("**\(library.stagedFileCount)** to apply")
            Divider().frame(height: 14)
            Toggle("Show old values", isOn: $library.showsOldValues)
                .toggleStyle(.checkbox)
            Divider().frame(height: 14)
            Button("Discard") { library.discard() }
            Button("Apply") { Task { await library.apply() } }
                .buttonStyle(.borderedProminent)
                .disabled(library.isBusy)
        }
        .font(.callout)
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background(.regularMaterial, in: .capsule)
        .overlay(Capsule().strokeBorder(.separator))
        .shadow(radius: 8, y: 2)
        .padding(.bottom, 16)
    }
}

// MARK: - Panel

@MainActor
struct ModePanel: View {
    let library: Library
    let mode: Mode
    let selection: Set<Track.ID>
    @State private var subtab = 1
    @State private var drafts: [Field: String] = [:]

    private var tracks: [Track] {
        library.tracks.filter { selection.contains($0.id) }
    }

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
                Divider()
            }

            if mode != .tagger || subtab != 1 {
                ContentUnavailableView(
                    "Not in this build",
                    systemImage: "hammer",
                    description: Text("This build carries the tag editor; the other modes come next.")
                )
            } else if tracks.isEmpty {
                ContentUnavailableView(
                    "Nothing selected",
                    systemImage: "square.dashed",
                    description: Text("Pick a row to edit its tags.")
                )
            } else {
                editor
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .onChange(of: selection) { _, _ in drafts = [:] }
    }

    /// Laid out by hand rather than with Form: the grouped form style trails the
    /// value, sizes the label column per row and ignores the field's own frame,
    /// so a column of fields came out ragged and right-aligned. This panel edits
    /// a table and should read like one.
    private var editor: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                group("Tag fields") {
                    ForEach(Field.allCases) { field in
                        row(field.label) {
                            TextField("", text: binding(field), prompt: prompt(field))
                                .textFieldStyle(.roundedBorder)
                                .font(AppFonts.body)
                                .foregroundStyle(isStaged(field)
                                                 ? AnyShapeStyle(.green)
                                                 : AnyShapeStyle(.primary))
                                .onSubmit { stage(field) }
                        }
                    }
                }

                group("File") {
                    fact("Format", shared { $0.format })
                    fact("Length", shared { $0.duration })
                    fact("Bitrate", shared { $0.bitrateKbps.map { "\($0) kbps" } ?? "—" })
                }

                Text("Return stages a field. Nothing is written until Apply.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(14)
        }
    }

    private func group<Content: View>(
        _ title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)
            content()
        }
    }

    /// One panel row: a fixed label column, then the control filling the rest —
    /// which is what keeps every field the same width.
    private func row<Content: View>(
        _ label: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        HStack(spacing: 10) {
            Text(label)
                .frame(width: 92, alignment: .leading)
                .foregroundStyle(.secondary)
            content()
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func fact(_ label: String, _ value: String) -> some View {
        row(label) {
            Text(value.isEmpty ? "—" : value)
                .textSelection(.enabled)
        }
    }

    private func binding(_ field: Field) -> Binding<String> {
        Binding(
            get: {
                if let draft = drafts[field] { return draft }
                if let staged = stagedShared(field) { return staged }
                return shared { $0.value(for: field) }
            },
            set: { drafts[field] = $0 }
        )
    }

    private func stage(_ field: Field) {
        guard let draft = drafts[field] else { return }
        library.stage(field, to: draft, for: tracks.map(\.id))
        drafts.removeValue(forKey: field)
    }

    /// A staged value the whole selection shares, when there is one.
    private func stagedShared(_ field: Field) -> String? {
        let values = tracks.compactMap { library.stagedValue(field, for: $0.id) }
        guard values.count == tracks.count, Set(values).count == 1 else { return nil }
        return values.first
    }

    /// The selection's shared value, or the app's own <multiple values>. A
    /// field showing this is left alone unless it is typed in, which is the
    /// rule the web editor follows.
    private func shared(_ pick: (Track) -> String) -> String {
        let values = Set(tracks.map(pick))
        return values.count == 1 ? (values.first ?? "") : ""
    }

    /// What an empty field shows: the app's own <multiple values> when the
    /// selection disagrees, nothing when it is simply empty.
    private func prompt(_ field: Field) -> Text {
        let values = Set(tracks.map { $0.value(for: field) })
        return Text(values.count > 1 ? "<multiple values>" : "")
    }

    private func isStaged(_ field: Field) -> Bool {
        tracks.contains { library.stagedValue(field, for: $0.id) != nil }
    }
}

// MARK: - Status bar

@MainActor
struct StatusBar: View {
    let library: Library
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
                if library.isBusy {
                    ProgressView().controlSize(.small)
                }
                Text(library.lastMessage.isEmpty
                     ? "Playback is out of scope for this build"
                     : library.lastMessage)
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
