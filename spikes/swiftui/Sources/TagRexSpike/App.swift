// The window (#271). Layout follows the current web UI one for one — the same
// toolbar order, the same five columns, the trailing panel, the status bar, and
// the same discipline: an edit is staged, shown in the table as a diff, and
// written only when Apply is pressed.

import SwiftUI
import UniformTypeIdentifiers

enum Mode: String, CaseIterable, Identifiable {
    case tagger, renamer, generator, deduplicator, exporter

    var id: Self { self }

    /// The web UI's own five names, in its own agent-noun pattern — the tab is
    /// a verb applied to the table, so it is named for the thing that does it.
    /// Shortening the last two to "Duplicates" and "Export" bought a narrower
    /// picker and broke the row.
    var title: String {
        switch self {
        case .tagger: "Tagger"
        case .renamer: "Renamer"
        case .generator: "Generator"
        case .deduplicator: "Deduplicator"
        case .exporter: "Exporter"
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

/// The mode tabs, matching the Tauri top bar: an icon and an UPPERCASE label per
/// mode, the active one tinted. A custom control rather than a segmented Picker,
/// which shows text or icon but not both.
@MainActor
struct ModeTabBar: View {
    @Binding var selection: Mode

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Mode.allCases) { mode in
                let isOn = mode == selection
                Button {
                    selection = mode
                } label: {
                    HStack(spacing: 5) {
                        Image(systemName: mode.symbol)
                        Text(mode.title.uppercased())
                            .fontWeight(.medium)
                    }
                    .font(.system(size: 11))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .foregroundStyle(isOn ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                    .background {
                        if isOn {
                            RoundedRectangle(cornerRadius: 6).fill(.tint.opacity(0.15))
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help(mode.title)
            }
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
    @State private var showingSettings = false
    /// Bumped to ask the filter field for the keyboard. A counter rather
    /// than a Bool: focus is an event, and a Bool that is already true
    /// cannot fire a second time.
    @State private var focusFilter = 0

    private var rows: [Track] { library.visibleTracks.sorted(using: sortOrder) }

    /// What the panel edits and the status bar counts: the selection narrowed to
    /// the rows the filter leaves on screen.
    ///
    /// The set itself is never pruned. The filter runs on every keystroke, so
    /// pruning would let one character destroy a hand-built selection with no
    /// way back — and the web UI holds the same line: a re-render "never
    /// silently wipes or widens the selection". But the scope of an edit has to
    /// be something the user can see. Narrowing here keeps both: type into the
    /// filter and the panel follows what is on screen, clear it and the whole
    /// selection is still there. Staged edits are keyed by file and are not
    /// touched either way — a row that scrolls out of the filter keeps its
    /// staged change and still applies.
    private var visibleSelection: Set<Track.ID> {
        selection.intersection(rows.lazy.map(\.id))
    }

    /// Selected rows the filter is currently hiding. Reported rather than
    /// silently dropped, so an empty panel with rows selected is explained.
    private var hiddenSelectionCount: Int { selection.count - visibleSelection.count }

    var body: some View {
        @Bindable var library = library

        TrackTable(
            rows: rows,
            selection: $selection,
            sortOrder: $sortOrder,
            staged: library.staged,
            renames: library.stagedRenames,
            showsOldValues: library.showsOldValues
        )
            .overlay(alignment: .bottom) {
                if library.hasStagedPlan { ChangePlanBar(library: library) }
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                StatusBar(
                    library: library,
                    queue: rows.map(\.id),
                    selectedFirst: rows.first { visibleSelection.contains($0.id) }?.id,
                    shown: rows.count,
                    total: library.tracks.count,
                    selected: visibleSelection.count,
                    hidden: hiddenSelectionCount
                )
            }
            .inspector(isPresented: $showsInspector) {
                ModePanel(library: library, mode: mode, selection: visibleSelection)
                    .inspectorColumnWidth(min: 320, ideal: 380, max: 560)
                    // Declared on the inspector, not beside the other items: an
                    // inspector's own toolbar content is what claims the
                    // titlebar strip above its column, and with nothing claiming
                    // it every trailing item packs to the far edge of the window
                    // — which is how the filter ended up over the panel. It is
                    // also where the toggle belongs, above the thing it hides.
                    .toolbar {
                        ToolbarItem {
                            Button {
                                showsInspector.toggle()
                            } label: {
                                Label("Panel", systemImage: "sidebar.trailing")
                            }
                            .help("Show or hide the panel")
                        }
                    }
            }
            // The window title is dropped from the toolbar rather than shown:
            // it landed between the folder group and the centred picker, in the
            // title face, saying the app's own name — which the menu bar
            // already does. The folder is named by the button that opens it.
            .background {
                // Command-F, which .searchable used to provide. Zero-sized and
                // behind everything: it exists for the shortcut alone.
                Button("Filter") { focusFilter += 1 }
                    .keyboardShortcut("f", modifiers: .command)
                    .opacity(0)
                    .frame(width: 0, height: 0)
            }
            .toolbar(removing: .title)
            // Tahoe welds adjacent toolbar items into one glass capsule and
            // breaks it wherever a ToolbarSpacer sits, so the spacers are the
            // grouping. Choosing a folder and re-reading it are one subject and
            // share a capsule; undo and the panel toggle have nothing to do with
            // each other and get one each. A spacer inside .navigation does not
            // split — that placement is a single titlebar accessory — which is
            // why the leading pair is still written as a group.
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
                    ModeTabBar(selection: $mode)
                }

                // The filter is a toolbar item of its own rather than
                // .searchable: that modifier is wired to the far trailing corner
                // of the window, which is above the inspector column, so the
                // control that filters the table sat over the panel — and no
                // arrangement of the other items moves it, which is why this one
                // is built by hand.
                ToolbarItem {
                    FilterField(text: $library.filter, focusRequest: focusFilter)
                        .frame(width: 230)
                }
                .sharedBackgroundVisibility(.hidden)

                ToolbarSpacer(.fixed)

                ToolbarItem {
                    Button {
                        Task { await library.undo() }
                    } label: {
                        Label("Undo the last applied batch", systemImage: "arrow.uturn.backward")
                    }
                    .disabled(library.root == nil || library.isBusy)
                    .help("Undo the last applied batch")
                }

                ToolbarSpacer(.fixed)

                ToolbarItem {
                    Button {
                        showingSettings = true
                    } label: {
                        Label("Settings", systemImage: "gearshape")
                    }
                    .help("Settings")
                }
            }
            .sheet(isPresented: $showingSettings) {
                SettingsView(library: library)
            }
            .fileImporter(isPresented: $choosingFolder, allowedContentTypes: [.folder]) { result in
                guard case .success(let folder) = result else { return }
                Task { await library.open(folder) }
            }
            .navigationTitle("TagRex")
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

/// The filter field. An AppKit search field rather than a SwiftUI TextField:
/// SwiftUI hosts toolbar content outside the view hierarchy that declares it,
/// and a TextField put there never becomes first responder — a click sets a
/// caret in it, every keystroke after that goes to the table, which type-selects
/// on them, and @FocusState from the declaring view does not reach across the
/// boundary to fix it. NSSearchField owns its responder handling, so it works in
/// the one place the field has to be. It also brings its own bezel and its own
/// clear button, which is why the item hides the shared glass behind it.
@MainActor
struct FilterField: NSViewRepresentable {
    @Binding var text: String
    /// Every increment is one request for the keyboard.
    let focusRequest: Int

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.placeholderString = "Filter — try artist:aphex"
        field.delegate = context.coordinator
        // Filter as it is typed; the table is in memory and the plan is staged,
        // so there is nothing to defer until Return.
        field.sendsSearchStringImmediately = true
        field.sendsWholeSearchString = false
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        context.coordinator.text = $text
        // Only when they differ: assigning while the user types moves the caret
        // to the end of the line.
        if field.stringValue != text { field.stringValue = text }

        if context.coordinator.servedRequest != focusRequest {
            context.coordinator.servedRequest = focusRequest
            // Not on the first update — that would steal the keyboard from the
            // table the moment the window opens.
            if focusRequest > 0 { field.window?.makeFirstResponder(field) }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    final class Coordinator: NSObject, NSSearchFieldDelegate {
        var text: Binding<String>
        var servedRequest = 0

        init(text: Binding<String>) { self.text = text }

        func controlTextDidChange(_ notification: Notification) {
            guard let field = notification.object as? NSSearchField else { return }
            text.wrappedValue = field.stringValue
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
    /// path → the new name a staged rename gives it, shown in the File column as
    /// a diff the way a tag change shows in its column.
    let renames: [String: String]
    let showsOldValues: Bool

    var body: some View {
        Table(rows, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("File", value: \.file) { track in
                DiffCell(
                    value: renames[track.id] ?? track.file,
                    old: renames[track.id] == nil ? nil : track.file,
                    showsOld: showsOldValues
                )
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
    // ONLINE first, matching the Tauri default (its `subtab active` is online).
    @State private var subtab = 0
    @State private var drafts: [Field: String] = [:]

    private var tracks: [Track] {
        library.tracks.filter { selection.contains($0.id) }
    }

    var body: some View {
        Group {
            if mode == .renamer {
                RenamerPanel(library: library, selection: selection)
            } else if mode == .generator {
                GeneratorPanel(library: library, selection: selection)
            } else if mode == .deduplicator {
                DuplicatesPanel(library: library)
            } else if mode == .exporter {
                ExportPanel(library: library, selection: selection)
            } else {
                taggerPanel
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }

    private var taggerPanel: some View {
        VStack(alignment: .leading, spacing: 0) {
            Picker("", selection: $subtab) {
                Text("ONLINE").tag(0)
                Text("EDITOR").tag(1)
                Text("FROM NAME").tag(2)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(12)
            Divider()

            if subtab == 0 {
                OnlinePanel(library: library, selection: selection)
            } else if subtab == 2 {
                FromNamePanel(library: library, selection: selection)
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
    /// The visible rows in order, and the first selected one — what the player
    /// bar walks and where Play starts.
    let queue: [String]
    let selectedFirst: String?
    /// Rows the filter leaves in the table — the denominator, since that is what
    /// a count in the table is counted out of.
    let shown: Int
    /// The whole open folder. Named only when the filter is holding some of it
    /// back, so the number the denominator dropped from is still readable.
    let total: Int
    let selected: Int
    /// Selected but filtered off screen. Named in the count so an empty panel
    /// with a selection behind it is not a mystery.
    let hidden: Int

    private var isFiltered: Bool { shown != total }

    var body: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 10) {
                PlayerBar(library: library, queue: queue, selectedFirst: selectedFirst)
                Divider().frame(height: 14)
                if library.isBusy {
                    ProgressView().controlSize(.small)
                }
                if !library.lastMessage.isEmpty {
                    Text(library.lastMessage).lineLimit(1)
                }
                Spacer()
                Text(summary)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
        }
        .background(.bar)
    }

    /// The right-hand count. It names the hidden part of the selection rather
    /// than leaving the panel to go quiet for no visible reason.
    private var summary: String {
        switch (selected, hidden) {
        case (0, 0):
            isFiltered ? "\(shown) of \(total) tracks" : "\(total) tracks"
        case (0, let hidden):
            "\(hidden) selected, all hidden by the filter"
        case (let selected, 0):
            "\(selected) of \(shown) selected"
        case (let selected, let hidden):
            "\(selected) of \(shown) selected · \(hidden) hidden by the filter"
        }
    }
}
