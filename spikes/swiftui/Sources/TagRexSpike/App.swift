// The window (#271). Layout follows the current web UI one for one — the same
// toolbar order, the same five columns, the trailing panel, the status bar, and
// the same discipline: an edit is staged, shown in the table as a diff, and
// written only when Apply is pressed.

import AppKit
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

/// The brand green the Tauri UI uses as its accent (`--accent`), lighter on a
/// dark appearance the way the web theme brightens it.
private func brandAccent() -> NSColor {
    NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        return isDark
            ? NSColor(srgbRed: 0x0b / 255, green: 0x7d / 255, blue: 0x5c / 255, alpha: 1)
            : NSColor(srgbRed: 0x0b / 255, green: 0x6b / 255, blue: 0x53 / 255, alpha: 1)
    }
}

extension Color {
    /// Applied app-wide with `.tint` so SwiftUI controls read green, not blue.
    static let appAccent = Color(nsColor: brandAccent())

    /// The Tauri card/badge border (`--border`): a faint line that all but blends
    /// with the surface — #e2e5ea light, #2c313b dark.
    static let cardBorder = Color(nsColor: NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        return isDark
            ? NSColor(srgbRed: 0x2c / 255, green: 0x31 / 255, blue: 0x3b / 255, alpha: 1)
            : NSColor(srgbRed: 0xe2 / 255, green: 0xe5 / 255, blue: 0xea / 255, alpha: 1)
    })

    /// The Tauri diff-green (`--add`): #15803d light, #3fca74 dark. Used for a
    /// length delta that matches and the "N lengths match" tally.
    static let diffAdd = Color(nsColor: NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        return isDark
            ? NSColor(srgbRed: 0x3f / 255, green: 0xca / 255, blue: 0x74 / 255, alpha: 1)
            : NSColor(srgbRed: 0x15 / 255, green: 0x80 / 255, blue: 0x3d / 255, alpha: 1)
    })

    /// The Tauri diff-red (`--del`): #dc2626 light, #f87171 dark. Used for a
    /// length delta that is off by more than the near band and the "no lengths
    /// match" tally.
    static let diffDel = Color(nsColor: NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        return isDark
            ? NSColor(srgbRed: 0xf8 / 255, green: 0x71 / 255, blue: 0x71 / 255, alpha: 1)
            : NSColor(srgbRed: 0xdc / 255, green: 0x26 / 255, blue: 0x26 / 255, alpha: 1)
    })
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
                .tint(.appAccent)
        }
        // 1728 wide so the 432pt panel opens at 25% — a 75/25 split.
        .defaultSize(width: 1728, height: 1000)
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
    /// Group the table by folder (#129, T1). On by default, like the web UI.
    @State private var groupByFolder = true
    /// Which optional columns show (#43, T2), persisted in display order as CSV.
    @AppStorage("table.columns") private var columnsCSV = "artist,title,album,year"
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

    private var visibleColumns: Set<String> {
        Set(columnsCSV.split(separator: ",").map(String.init))
    }

    /// Toggle a column, rewriting the CSV in the picker's display order.
    private func toggleColumn(_ key: String) {
        var set = visibleColumns
        if set.contains(key) { set.remove(key) } else { set.insert(key) }
        columnsCSV = TrackTable.optionalColumns.map(\.key).filter(set.contains).joined(separator: ",")
    }

    var body: some View {
        @Bindable var library = library

        TrackTable(
            rows: rows,
            selection: $selection,
            sortOrder: $sortOrder,
            staged: library.staged,
            renames: library.stagedRenames,
            showsOldValues: library.showsOldValues,
            grouped: groupByFolder,
            rootPath: library.root?.path,
            visibleColumns: visibleColumns
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
                    // A 75/25 split (table/panel): on the ~1728-wide window the
                    // panel is 432 = 25%, and that is also its minimum so it never
                    // gets narrow enough to cramp the release cards.
                    .inspectorColumnWidth(min: 432, ideal: 432, max: 720)
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

                    Button {
                        groupByFolder.toggle()
                    } label: {
                        Label("Group by folder", systemImage: groupByFolder
                              ? "rectangle.grid.1x2.fill" : "rectangle.grid.1x2")
                    }
                    .help(groupByFolder ? "Grouping by folder — click to flatten" : "Group the table by folder")

                    Menu {
                        ForEach(TrackTable.optionalColumns, id: \.key) { column in
                            Toggle(column.label, isOn: Binding(
                                get: { visibleColumns.contains(column.key) },
                                set: { _ in toggleColumn(column.key) }
                            ))
                        }
                    } label: {
                        Label("Columns", systemImage: "tablecells")
                    }
                    .help("Choose which columns to show")
                }

                ToolbarItem(placement: .principal) {
                    Picker("Tool", selection: $mode) {
                        ForEach(Mode.allCases) { mode in
                            Text(mode.title.uppercased()).tag(mode)
                        }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
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
    let staged: [String: [String: String]]
    /// path → the new name a staged rename gives it, shown in the File column as
    /// a diff the way a tag change shows in its column.
    let renames: [String: String]
    let showsOldValues: Bool
    /// Group rows by their containing folder under a section header (#129, T1).
    /// On by default, matching the web UI's `groupBy = "folder"`.
    let grouped: Bool
    /// The open library root, so a folder header reads relative to it
    /// ("gui-test/CD1") rather than as an absolute path.
    let rootPath: String?
    /// Which optional columns are shown (#43, T2). File is always present.
    let visibleColumns: Set<String>

    /// The optional columns the picker offers, in display order — each a modeled
    /// field with its own keypath so the column stays sortable.
    static let optionalColumns: [(key: String, label: String)] = [
        ("artist", "Artist"), ("title", "Title"), ("album", "Album"),
        ("albumartist", "Album Artist"), ("track", "Track"),
        ("year", "Year"), ("genre", "Genre"),
    ]

    var body: some View {
        Table(of: Track.self, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("File", value: \.file) { track in
                DiffCell(
                    value: renames[track.id] ?? track.file,
                    old: renames[track.id] == nil ? nil : track.file,
                    showsOld: showsOldValues
                )
            }
            .width(min: 180, ideal: 300)

            if visibleColumns.contains("artist") {
                TableColumn("Artist", value: \.artist) { cell($0, .artist) }
                    .width(min: 90, ideal: 150)
            }
            if visibleColumns.contains("title") {
                TableColumn("Title", value: \.title) { cell($0, .title) }
                    .width(min: 90, ideal: 190)
            }
            if visibleColumns.contains("album") {
                TableColumn("Album", value: \.album) { cell($0, .album) }
                    .width(min: 90, ideal: 160)
            }
            if visibleColumns.contains("albumartist") {
                TableColumn("Album Artist", value: \.albumartist) { cell($0, .albumartist) }
                    .width(min: 90, ideal: 150)
            }
            if visibleColumns.contains("track") {
                TableColumn("Track", value: \.track) { cell($0, .track) }
                    .width(56)
            }
            if visibleColumns.contains("year") {
                TableColumn("Year", value: \.year) { cell($0, .year) }
                    .width(56)
            }
            if visibleColumns.contains("genre") {
                TableColumn("Genre", value: \.genre) { cell($0, .genre) }
                    .width(min: 80, ideal: 130)
            }
        } rows: {
            if grouped {
                ForEach(folderGroups, id: \.key) { group in
                    Section(group.label) {
                        ForEach(group.tracks) { TableRow($0) }
                    }
                }
            } else {
                ForEach(rows) { TableRow($0) }
            }
        }
        .tableStyle(.inset(alternatesRowBackgrounds: true))
    }

    /// Rows bucketed by their parent folder, folders in path order and each
    /// folder's rows kept in the incoming sort order. A view overlay — it never
    /// reorders `rows` itself, the way the web grouping is "a view overlay".
    private var folderGroups: [(key: String, label: String, tracks: [Track])] {
        var byFolder: [String: [Track]] = [:]
        var order: [String] = []
        for track in rows {
            let folder = (track.path as NSString).deletingLastPathComponent
            if byFolder[folder] == nil { order.append(folder) }
            byFolder[folder, default: []].append(track)
        }
        return order.sorted().map { (key: $0, label: folderLabel($0), tracks: byFolder[$0]!) }
    }

    /// A folder header relative to the session root, starting with the root's own
    /// name (`gui-test/CD1`), so nested folders read as a tree (`folderGroupLabel`).
    private func folderLabel(_ key: String) -> String {
        // Trailing-slash trim only, no symlink resolution: standardizingPath can
        // rewrite /private/tmp to /tmp, and then the root no longer prefixes the
        // track paths (which keep /private) and every header falls back to a leaf.
        guard var root = rootPath, !root.isEmpty else {
            return (key as NSString).lastPathComponent
        }
        while root.count > 1, root.hasSuffix("/") { root.removeLast() }
        let rootLeaf = (root as NSString).lastPathComponent
        if key == root { return rootLeaf }
        if key.hasPrefix(root + "/") {
            let rel = String(key.dropFirst(root.count + 1))
            return "\(rootLeaf)/\(rel)"
        }
        return (key as NSString).lastPathComponent
    }

    private func cell(_ track: Track, _ field: Field) -> DiffCell {
        let stagedValue = staged[track.id]?[field.rawValue]
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
    /// In-flight edits, keyed by storage key, not yet staged (Return stages).
    @State private var drafts: [String: String] = [:]
    /// Which collapsible editor groups are folded. Advanced starts folded, the
    /// way the web editor opens it (#136); Standard opens.
    @State private var collapsedGroups: Set<String> = ["advanced"]
    /// The inline add-field row (#114): idle until opened, then a name/value pair.
    @State private var addingField = false
    @State private var newFieldName = ""
    @State private var newFieldValue = ""

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

    /// A dynamic field editor over the selection's real tags (E1), grouped the
    /// way the web editor groups them (`renderFieldEditor`): Core always open,
    /// Standard and Advanced collapsible. Laid out by hand rather than with Form:
    /// the grouped form style trails the value and sizes the label column per row,
    /// so a column of fields came out ragged. This panel edits a table and reads
    /// like one — a fixed label column, the control filling the rest.
    private var editor: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                TagBlocksBar(library: library, tracks: tracks)

                CoverWell(library: library, tracks: tracks)

                ForEach(EditorFields.groups(presentKeys: presentKeys)) { fieldGroup($0) }

                addFieldRow

                group("File", collapsible: false) {
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

    /// The tag keys the editor lays out: every key present on a selected file,
    /// plus any key already staged for one — so a field staged a moment ago (or a
    /// custom frame) still lists after the selection redraws.
    private var presentKeys: Set<String> {
        var keys = Set<String>()
        for track in tracks {
            keys.formUnion(track.tags.keys)
            keys.formUnion((library.staged[track.id] ?? [:]).keys)
        }
        return keys
    }

    /// The add-field affordance (#114): idle shows just "Add field"; opening it
    /// reveals an inline name/value row that stages a custom frame across the
    /// selection. Any name without a `custom:` prefix becomes one, so this adds
    /// arbitrary frames — the known fields already have their own rows.
    @ViewBuilder
    private var addFieldRow: some View {
        if addingField {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    TextField("Field name", text: $newFieldName)
                        .textFieldStyle(.roundedBorder)
                        .font(AppFonts.body)
                        .onSubmit { addField() }
                    if !presentCustomNames.isEmpty {
                        Menu {
                            ForEach(presentCustomNames, id: \.self) { name in
                                Button(name) { newFieldName = name }
                            }
                        } label: {
                            Image(systemName: "list.bullet")
                        }
                        .menuStyle(.borderlessButton)
                        .fixedSize()
                        .help("Custom fields already on the selection")
                    }
                }
                HStack(spacing: 6) {
                    TextField("Value", text: $newFieldValue)
                        .textFieldStyle(.roundedBorder)
                        .font(AppFonts.body)
                        .onSubmit { addField() }
                    Button("Add") { addField() }
                        .disabled(newFieldName.trimmingCharacters(in: .whitespaces).isEmpty)
                    Button {
                        addingField = false
                        newFieldName = ""
                        newFieldValue = ""
                    } label: {
                        Image(systemName: "xmark")
                    }
                    .buttonStyle(.borderless)
                    .help("Close")
                }
            }
        } else {
            Button {
                addingField = true
            } label: {
                Label("Add field", systemImage: "plus")
                    .font(AppFonts.body)
            }
            .buttonStyle(.borderless)
        }
    }

    /// The custom frame names already present across the selection, offered as
    /// suggestions when adding a field (`populateKnownFields`).
    private var presentCustomNames: [String] {
        var names = Set<String>()
        for key in presentKeys where key.hasPrefix("custom:") {
            names.insert(String(key.dropFirst("custom:".count)))
        }
        return names.sorted()
    }

    /// Stage the typed field across the selection, then keep the row open and
    /// refocus so several fields add in a row (#114).
    private func addField() {
        let name = newFieldName.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        // A bare known field keeps its own key; anything else becomes a custom
        // frame — the known fields already have rows, so this row adds customs.
        let key: String
        if name.hasPrefix("custom:") || EditorFields.extended.contains(where: { $0.key == name }) {
            key = name
        } else {
            key = "custom:\(name)"
        }
        library.stage(key, to: newFieldValue, for: tracks.map(\.id))
        // Reveal the group the new row lands in so the add is visible: a named
        // frame is promoted to Standard, an unnamed one drops into Advanced.
        let raw = key.hasPrefix("custom:") ? String(key.dropFirst("custom:".count)) : key
        let landsInAdvanced = key.hasPrefix("custom:")
            && EditorFields.knownCustomLabels[raw.uppercased()] == nil
        collapsedGroups.remove(landsInAdvanced ? "advanced" : "standard")
        newFieldName = ""
        newFieldValue = ""
    }

    @ViewBuilder
    private func fieldGroup(_ fieldGroup: EditorFields.Group) -> some View {
        let collapsed = fieldGroup.collapsible && collapsedGroups.contains(fieldGroup.id)
        group(fieldGroup.title, collapsible: fieldGroup.collapsible,
              count: fieldGroup.rows.count, collapsed: collapsed,
              toggle: { toggleGroup(fieldGroup.id) }) {
            if !collapsed {
                ForEach(fieldGroup.rows) { fieldRow($0) }
            }
        }
    }

    @ViewBuilder
    private func fieldRow(_ fieldRow: EditorFields.Row) -> some View {
        switch fieldRow {
        case .single(let key):
            row(EditorFields.label(for: key)) {
                HStack(spacing: 6) {
                    fieldInput(key)
                    lockButton([key])
                }
            }
        case .duo(let label, let numberKey, let totalKey):
            row(label) {
                HStack(spacing: 6) {
                    fieldInput(numberKey).frame(width: 64)
                    Text("/").foregroundStyle(.tertiary)
                    fieldInput(totalKey).frame(width: 64)
                    Spacer()
                    // Lock the pair as a unit — half a "3 / 12" protects nothing.
                    lockButton([numberKey, totalKey])
                }
            }
        }
    }

    /// A padlock that locks a field (#63): a locked field is skipped by every
    /// plan the backend builds, so it survives imports, transforms and edits.
    private func lockButton(_ keys: [String]) -> some View {
        let on = keys.allSatisfy(library.isLocked)
        return Button {
            Task { await library.toggleLock(keys) }
        } label: {
            Image(systemName: on ? "lock.fill" : "lock.open")
                .font(.caption)
                .foregroundStyle(on ? AnyShapeStyle(.tint) : AnyShapeStyle(.tertiary))
        }
        .buttonStyle(.borderless)
        .help(on ? "Locked — plans skip this field" : "Lock this field against changes")
    }

    /// One field's text input, with the staged-green tint, the shared/multiple
    /// prompt, and an inline validation hint for the numeric/typed fields.
    @ViewBuilder
    private func fieldInput(_ key: String) -> some View {
        let hint = EditorFields.validationHint(for: key, value: currentValue(key))
        let locked = library.isLocked(key)
        VStack(alignment: .leading, spacing: 2) {
            TextField("", text: binding(key), prompt: prompt(key))
                .textFieldStyle(.roundedBorder)
                .font(EditorFields.numericKeys.contains(key) ? AppFonts.monoSized(13) : AppFonts.body)
                .multilineTextAlignment(EditorFields.numericKeys.contains(key) ? .trailing : .leading)
                .foregroundStyle(isStaged(key) ? AnyShapeStyle(.green) : AnyShapeStyle(.primary))
                .onSubmit { stage(key) }
                // A locked field is inert — the backend gate drops it from every
                // plan anyway, so editing it here would only mislead.
                .disabled(locked)
                .opacity(locked ? 0.5 : 1)
            if let hint {
                Text(hint).font(.caption2).foregroundStyle(.red)
            }
        }
    }

    private func group<Content: View>(
        _ title: String,
        collapsible: Bool,
        count: Int? = nil,
        collapsed: Bool = false,
        toggle: (() -> Void)? = nil,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Text(title)
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.secondary)
                if let count, collapsible {
                    Text("\(count)")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                    Image(systemName: "chevron.down")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .rotationEffect(.degrees(collapsed ? -90 : 0))
                }
                Spacer()
            }
            .contentShape(Rectangle())
            .onTapGesture { if collapsible { toggle?() } }
            content()
        }
    }

    private func toggleGroup(_ id: String) {
        if collapsedGroups.contains(id) { collapsedGroups.remove(id) } else { collapsedGroups.insert(id) }
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

    private func binding(_ key: String) -> Binding<String> {
        Binding(
            get: { currentValue(key) },
            set: { drafts[key] = $0 }
        )
    }

    /// What a field shows: the in-flight draft, else the staged value the whole
    /// selection shares, else the selection's own shared value.
    private func currentValue(_ key: String) -> String {
        if let draft = drafts[key] { return draft }
        if let staged = stagedShared(key) { return staged }
        return shared { $0.value(forKey: key) }
    }

    private func stage(_ key: String) {
        guard let draft = drafts[key] else { return }
        // A value that would not write (a non-numeric year, say) is not staged —
        // the same gate the web editor puts before staging.
        if EditorFields.validationHint(for: key, value: draft) != nil { return }
        library.stage(key, to: draft, for: tracks.map(\.id))
        drafts.removeValue(forKey: key)
    }

    /// A staged value the whole selection shares, when there is one.
    private func stagedShared(_ key: String) -> String? {
        let values = tracks.compactMap { library.stagedValue(key, for: $0.id) }
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
    private func prompt(_ key: String) -> Text {
        let values = Set(tracks.map { $0.value(forKey: key) })
        return Text(values.count > 1 ? "<multiple values>" : "")
    }

    private func isStaged(_ key: String) -> Bool {
        tracks.contains { library.stagedValue(key, for: $0.id) != nil }
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
