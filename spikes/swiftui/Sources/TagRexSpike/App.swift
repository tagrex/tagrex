// The window (#271). Layout follows the current web UI one for one — the same
// toolbar order, the same five columns, the trailing panel, the status bar, and
// the same discipline: an edit is staged, shown in the table as a diff, and
// written only when Apply is pressed.

import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// A saved filter + sort + group view (#44) — the Tauri "filter/sort preset":
/// name, filter text and flags, the primary sort column/direction, and the
/// grouping key (empty when ungrouped).
struct FilterPreset: Codable, Identifiable, Equatable {
    var name: String
    var filter: String
    var regex: Bool
    var caseSensitive: Bool
    var sortKey: String?
    var sortAscending: Bool
    var group: String

    var id: String { name }

    /// A one-line summary for the menu row's tooltip — mirrors Tauri's
    /// `presetSummary` (`app/ui/app.js`): filter, then sort (by column label),
    /// then "group by <key>" (the raw key, same as the JS).
    var summary: String {
        var bits: [String] = []
        if !filter.isEmpty {
            var f = "filter \u{201C}\(filter)\u{201D}"
            if regex { f += " (regex)" }
            if caseSensitive { f += " (Aa)" }
            bits.append(f)
        }
        if let sortKey {
            bits.append("sort \(GroupField.label(for: sortKey)) \(sortAscending ? "\u{2191}" : "\u{2193}")")
        }
        if !group.isEmpty { bits.append("group by \(group)") }
        return bits.isEmpty ? "empty view" : bits.joined(separator: " · ")
    }
}

/// The grouping keys the group menu offers, and the labels used for the "(no
/// X)" group-header fallback and the preset-summary sort label — mirrors
/// Tauri's `GROUP_COMMON` + `EXTENDED_FIELDS` (`app/ui/js/columns.js`,
/// `app/ui/js/fields.js`).
enum GroupField {
    static let common: [(key: String, label: String)] = [
        ("", "None"), ("folder", "Folder"), ("release", "Release id"),
        ("artist", "Artist"), ("album", "Album"), ("albumartist", "Album Artist"),
    ]
    static let extra: [(key: String, label: String)] = [
        ("title", "Title"), ("track", "Track"), ("tracktotal", "Track Total"),
        ("disc", "Disc"), ("year", "Year"), ("genre", "Genre"),
        ("comment", "Comment"), ("composer", "Composer"), ("publisher", "Publisher"),
        ("catalognumber", "Catalogue #"), ("bpm", "BPM"), ("isrc", "ISRC"),
        ("key", "Key"), ("url", "URL"), ("media", "Media"),
    ]
    static func label(for key: String) -> String {
        if key == "file" { return "File" }
        if key == "length" { return "Length" }
        return (common + extra).first { $0.key == key }?.label ?? key.capitalized
    }
}

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

/// The mode tabs (icon + label, active one underlined) — the Tauri top-bar look
/// (`.mode-tab`), not a segmented control. Icons were dropped from an earlier,
/// differently-styled tab bar; this one restores them alongside the underline.
@MainActor
struct ModeTabBar: View {
    @Binding var mode: Mode

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Mode.allCases) { item in
                let isActive = item == mode
                Button {
                    mode = item
                } label: {
                    // One row — icon inline with the label, the underline a
                    // 2px border directly on the tab itself (Tauri's
                    // `.mode-tab { border-bottom: 2px solid transparent }`),
                    // not a separate row below a VStack. Every tab is
                    // semibold (`font-weight: var(--fw-medium)` is on the
                    // base rule, not just `.active`); only the icon dims
                    // until active/hovered (`.mode-tab > .ico { opacity: .8 }`).
                    HStack(spacing: 6) {
                        Image(systemName: item.symbol)
                            .opacity(isActive ? 1 : 0.8)
                        Text(item.title.uppercased())
                    }
                    .font(AppFonts.sans(12, .semibold))
                    .foregroundStyle(isActive ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                    .padding(.vertical, 10)
                    .padding(.horizontal, 14)
                    .overlay(alignment: .bottom) {
                        Rectangle()
                            .fill(isActive ? Color.appAccent : .clear)
                            .frame(height: 2)
                    }
                }
                .buttonStyle(.plain)
                // Without this, the system paints a persistent accent focus
                // ring around whichever tab last held keyboard focus (usually
                // the first one, TAGGER, since it's first in view order) —
                // showing an outline that has nothing to do with which tab is
                // actually selected (that's the underline above).
                .focusEffectDisabled()
            }
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
    /// Which field the table is grouped by, empty for ungrouped — mirrors
    /// Tauri's `groupBy` (`app/ui/js/state.js`), whose default is "folder".
    @State private var groupBy = "folder"
    /// Which folder groups are collapsed — lives here (not inside TrackTable)
    /// so the header's "Collapse all groups" button can drive it too.
    @State private var collapsedGroups: Set<String> = []
    /// Saved filter + sort + group presets (#44), persisted as JSON.
    @AppStorage("filterPresets") private var presetsRaw = "[]"
    @State private var presetNameDraft = ""
    @State private var showPresetsPopover = false
    @State private var showColumnsPopover = false
    /// The "Rules to run on what this panel produces" shortcut (`transform-btn`):
    /// a chain, set for the session (not persisted, matching Tauri), that's
    /// auto-applied to every plan any mode stages — see the `onChange` below.
    @State private var transformShortcutRules: [ChainRule] = [ChainRule()]
    @State private var transformShortcutActive = false
    @State private var showTransformShortcut = false
    /// Which optional columns show (#43, T2), persisted in display order as CSV.
    @AppStorage("table.columns") private var columnsCSV = "artist,title,album,year"
    /// A user-defined mask column (T2 custom column), persisted; empty = none.
    @AppStorage("table.customColumn") private var customMask = ""
    /// The custom column rendered per path, refreshed when the mask or rows change.
    @State private var customValues: [String: String] = [:]
    @State private var showCustomPrompt = false
    @State private var customDraft = ""
    /// Recently opened folders (most recent first), persisted as newline-joined
    /// paths, capped — the path bar's recent-folders menu (#177 parity).
    @AppStorage("recentFolders") private var recentsRaw = ""
    @State private var showPathPrompt = false
    @State private var pathDraft = ""
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

    // MARK: - Library path

    /// The open folder shown with its immediate parent for context, the way the
    /// Tauri path indicator reads (".../Temp music/various_…"). Middle-truncated
    /// by the label's frame.
    private var pathLabelText: String {
        guard let root = library.root else { return library.rootName }
        let name = root.lastPathComponent
        let parent = root.deletingLastPathComponent().lastPathComponent
        return parent.isEmpty ? name : ".../\(parent)/\(name)"
    }

    private var recents: [String] {
        recentsRaw.split(separator: "\n").map(String.init)
    }

    /// The group keys the table currently has under `groupBy` (same bucketing
    /// `TrackTable.groupedRows` does), needed here so the header's "collapse
    /// all" button can act on all of them without reaching into TrackTable.
    private var currentGroupKeys: [String] {
        guard !groupBy.isEmpty else { return [] }
        var seen = Set<String>()
        var order: [String] = []
        for track in rows {
            let key = track.groupKey(by: groupBy)
            if seen.insert(key).inserted { order.append(key) }
        }
        return order
    }

    // MARK: - Filter/sort presets (#44)

    private var presets: [FilterPreset] {
        (try? JSONDecoder().decode([FilterPreset].self, from: Data(presetsRaw.utf8))) ?? []
    }

    private func writePresets(_ list: [FilterPreset]) {
        guard let data = try? JSONEncoder().encode(list.sorted { $0.name < $1.name }) else { return }
        presetsRaw = String(data: data, encoding: .utf8) ?? "[]"
    }

    /// Store the current view under `name`, replacing a same-named one —
    /// mirrors the Tauri `saveCurrentPreset`.
    private func saveCurrentPreset(_ name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        let sort = sortOrder.first
        let preset = FilterPreset(
            name: trimmed,
            filter: library.filter,
            regex: library.filterRegex,
            caseSensitive: library.filterCaseSensitive,
            sortKey: sort.flatMap(sortKeyName),
            sortAscending: sort?.order == .forward,
            group: groupBy
        )
        writePresets(presets.filter { $0.name != trimmed } + [preset])
    }

    /// Re-apply a saved preset: filter text/flags, sort and grouping.
    private func applyPreset(_ preset: FilterPreset) {
        library.filter = preset.filter
        library.filterRegex = preset.regex
        library.filterCaseSensitive = preset.caseSensitive
        groupBy = preset.group
        if let key = preset.sortKey, let comparator = sortComparator(for: key, ascending: preset.sortAscending) {
            sortOrder = [comparator]
        }
    }

    /// The storage key a sort comparator's keypath corresponds to, for saving.
    private func sortKeyName(_ comparator: KeyPathComparator<Track>) -> String? {
        TrackTable.optionalColumns.map(\.key).first { key in
            sortComparator(for: key, ascending: true)?.keyPath == comparator.keyPath
        } ?? (comparator.keyPath == \Track.file ? "file" : nil)
    }

    /// A comparator for a storage key, the reverse of `sortKeyName` — every
    /// column the table can sort by.
    private func sortComparator(for key: String, ascending: Bool) -> KeyPathComparator<Track>? {
        let order: SortOrder = ascending ? .forward : .reverse
        switch key {
        case "file": return KeyPathComparator(\Track.file, order: order)
        case "artist": return KeyPathComparator(\Track.artist, order: order)
        case "title": return KeyPathComparator(\Track.title, order: order)
        case "album": return KeyPathComparator(\Track.album, order: order)
        case "albumartist": return KeyPathComparator(\Track.albumartist, order: order)
        case "track": return KeyPathComparator(\Track.track, order: order)
        case "year": return KeyPathComparator(\Track.year, order: order)
        case "genre": return KeyPathComparator(\Track.genre, order: order)
        case "catalognumber": return KeyPathComparator(\Track.catalognumber, order: order)
        case "length": return KeyPathComparator(\Track.durationSort, order: order)
        default: return nil
        }
    }

    /// Open a folder and remember it at the top of the recents (deduped, capped).
    private func openFolder(_ url: URL) async {
        await library.open(url)
        guard library.root != nil else { return }
        var list = recents.filter { $0 != url.path }
        list.insert(url.path, at: 0)
        recentsRaw = list.prefix(8).joined(separator: "\n")
    }

    /// The full header, replacing the native window toolbar entirely: everything
    /// lives in content now, in the same two rows Tauri's own header uses.
    ///
    /// Row 1 mirrors `<header class="topbar">` exactly — brand, mode tabs, a
    /// spacer, then the library path group, panel toggle, and undo/settings —
    /// all ONE row. Earlier this was split across a native toolbar (path/undo/
    /// settings) and a content row (brand/tabs/view-tools), which put path and
    /// tabs in different strips than Tauri does and left a stray native toolbar
    /// row above everything. Row 3 is Tauri's separate `.view-tabs` bar:
    /// grouping, columns, the eraser, and the filter — nothing from row 1 or 2
    /// belongs here.
    ///
    /// Row 1 (`topbar-main`) is brand + tabs + app actions; row 2 is the
    /// library path on its own full-width line (Tauri #343 — a long folder
    /// name used to crowd the tabs when it shared their row).
    private var header: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text("/tagrex/")
                    .font(AppFonts.monoSized(12, .semibold))
                    .foregroundStyle(.tint)

                ModeTabBar(mode: $mode)

                Spacer(minLength: 12)

                Button {
                    showsInspector.toggle()
                } label: {
                    Image(systemName: "sidebar.trailing")
                }
                .buttonStyle(.borderless)
                .focusEffectDisabled()
                .help("Show or hide the panel")

                Divider().frame(height: 16)

                Button {
                    Task { await library.undo() }
                } label: {
                    Image(systemName: "arrow.uturn.backward")
                }
                .buttonStyle(.borderless)
                .focusEffectDisabled()
                .disabled(library.root == nil || library.isBusy)
                .help("Undo the last applied batch")

                Button {
                    showingSettings = true
                } label: {
                    Image(systemName: "gearshape")
                }
                .buttonStyle(.borderless)
                .focusEffectDisabled()
                .help("Settings")
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)

            HStack(spacing: 10) {
                libraryPathControls
            }
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 8)
            Divider()

            HStack(spacing: 10) {
                // Group by any modeled field (`group-btn`/`populateGroupMenu`),
                // not just an on/off folder toggle — the common groupings first,
                // then every other field below a separator. Tints while grouping
                // is on, same as the Tauri button.
                Menu {
                    ForEach(GroupField.common, id: \.key) { field in
                        Button {
                            groupBy = field.key
                        } label: {
                            if groupBy == field.key {
                                Label(field.label, systemImage: "checkmark")
                            } else {
                                Text(field.label)
                            }
                        }
                    }
                    Divider()
                    ForEach(GroupField.extra, id: \.key) { field in
                        Button {
                            groupBy = field.key
                        } label: {
                            if groupBy == field.key {
                                Label(field.label, systemImage: "checkmark")
                            } else {
                                Text(field.label)
                            }
                        }
                    }
                } label: {
                    Image(systemName: groupBy.isEmpty ? "rectangle.grid.1x2" : "rectangle.grid.1x2.fill")
                }
                .menuStyle(.borderlessButton)
                // Tauri's group-btn is a plain icon button, no caret — suppress
                // SwiftUI's automatic disclosure indicator.
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Group by: \(GroupField.label(for: groupBy))")

                // Collapse/expand every group at once (`toggle-groups`): while
                // any group is still open it offers "collapse", once everything
                // is shut it flips to "expand" — one control, not two buttons.
                if !groupBy.isEmpty, !currentGroupKeys.isEmpty {
                    Button {
                        if collapsedGroups.count < currentGroupKeys.count {
                            collapsedGroups = Set(currentGroupKeys)
                        } else {
                            collapsedGroups.removeAll()
                        }
                    } label: {
                        Image(systemName: collapsedGroups.count < currentGroupKeys.count
                              ? "arrow.down.right.and.arrow.up.left"
                              : "arrow.up.left.and.arrow.down.right")
                    }
                    .buttonStyle(.borderless)
                    .focusEffectDisabled()
                    .help(collapsedGroups.count < currentGroupKeys.count
                          ? "Collapse every group" : "Expand every group")
                }

                // Filter flags (#44) sit inside the field's own box in Tauri
                // (`.filter-ctl` is `position: relative`, the flags `absolute`
                // over a `padding-right` reserved for them) rather than beside
                // it as separate controls.
                FilterField(text: Bindable(library).filter, focusRequest: focusFilter,
                            invalid: library.filterInvalid)
                    .frame(maxWidth: 220)
                    .overlay(alignment: .trailing) {
                        HStack(spacing: 2) {
                            filterFlag(".*", on: Bindable(library).filterRegex,
                                       help: "Match the filter as a regular expression")
                            filterFlag("Aa", on: Bindable(library).filterCaseSensitive,
                                       help: "Match case-sensitively")
                        }
                        .padding(.trailing, 4)
                    }

                presetsMenu

                columnsMenu

                // `tb-div`: everything to the left configures the view; the
                // transform chain and eraser to the right act on the selection.
                Divider().frame(height: 16)

                transformShortcutButton

                // Clear text tags on the selection (#toolbar), surfaced here so it
                // doesn't need a trip into the editor. Cover art and cue points are
                // kept; it's previewed in the change bar before anything is written.
                Button {
                    let paths = rows.filter { visibleSelection.contains($0.id) }.map(\.id)
                    Task { _ = await library.stageClearTags(paths: paths) }
                } label: {
                    Image(systemName: "eraser")
                }
                .buttonStyle(.borderless)
                .focusEffectDisabled()
                .disabled(visibleSelection.isEmpty)
                .help("Clear text tags on the selected files (cover and cue points are kept)")
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            Divider()
        }
        .background(.bar)
    }

    /// The library path group (open folder / recents / re-read) — Tauri's `.lib`
    /// div, now a content row instead of a native toolbar item. Tauri splits
    /// this into a path-text button (#root-display, no icon) and a SEPARATE
    /// folder-icon button (#lib-action) right after the recents chevron — two
    /// affordances that both open the chooser, not one combined button. Since
    /// #343 the path box has its own full-width row and fills it (`flex: 1`),
    /// rather than the 220pt cap it needed sharing the row with the tabs.
    @ViewBuilder
    private var libraryPathControls: some View {
        Button {
            choosingFolder = true
        } label: {
            Text(pathLabelText)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .buttonStyle(.bordered)
        .focusEffectDisabled()
        .help(library.root?.path ?? "Choose a folder to open")

        Menu {
            Button("Browse…") { choosingFolder = true }
            Button("Open path…") {
                pathDraft = library.root?.path ?? ""
                showPathPrompt = true
            }
            if !recents.isEmpty {
                Divider()
                Section("Recent") {
                    ForEach(recents, id: \.self) { path in
                        Button {
                            Task { await openFolder(URL(fileURLWithPath: path)) }
                        } label: {
                            Text((path as NSString).lastPathComponent)
                        }
                    }
                }
            }
        } label: {
            Image(systemName: "chevron.down")
        }
        .menuStyle(.borderlessButton)
        // Without this, SwiftUI appends its own disclosure caret next to the
        // explicit chevron.down label above — two carets side by side.
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Recent folders, or open by path")

        Button {
            choosingFolder = true
        } label: {
            Image(systemName: "folder")
        }
        .buttonStyle(.borderless)
        .focusEffectDisabled()
        .help("Choose a folder to open")

        Button {
            Task { await library.rescan() }
        } label: {
            Image(systemName: "arrow.clockwise")
        }
        .buttonStyle(.borderless)
        .focusEffectDisabled()
        .disabled(library.root == nil)
        .help("Re-read the open folder")
    }

    /// One monospaced filter-flag toggle (`.*`, `Aa`) — tinted when on.
    private func filterFlag(_ label: String, on: Binding<Bool>, help: String) -> some View {
        Button {
            on.wrappedValue.toggle()
        } label: {
            Text(label)
                .font(AppFonts.monoSized(11, .semibold))
                .foregroundStyle(on.wrappedValue ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                .frame(width: 24, height: 20)
                .background(on.wrappedValue ? AnyShapeStyle(.tint.opacity(0.15)) : AnyShapeStyle(.clear),
                           in: RoundedRectangle(cornerRadius: 4))
        }
        .buttonStyle(.plain)
        .help(help)
    }

    /// Bookmark popover: saved filter+sort+group presets — mirrors Tauri's
    /// `renderPresetsMenu` (`app/ui/app.js`), a popover rather than a native
    /// submenu: each row is a click-to-apply name plus an inline delete ×, and
    /// the save affordance is a name field + Save button right in the popover,
    /// not a separate dialog.
    private var presetsMenu: some View {
        Button {
            presetNameDraft = ""
            showPresetsPopover.toggle()
        } label: {
            Image(systemName: "bookmark")
        }
        .buttonStyle(.borderless)
        .focusEffectDisabled()
        .help("Save and re-apply filter + sort presets")
        .popover(isPresented: $showPresetsPopover, arrowEdge: .bottom) {
            VStack(alignment: .leading, spacing: 6) {
                if presets.isEmpty {
                    Text("No saved presets")
                        .font(AppFonts.sans(11))
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(presets) { preset in
                        HStack(spacing: 6) {
                            Button(preset.name) {
                                applyPreset(preset)
                                showPresetsPopover = false
                            }
                            .buttonStyle(.plain)
                            .help(preset.summary)
                            Spacer(minLength: 8)
                            Button {
                                writePresets(presets.filter { $0.name != preset.name })
                            } label: {
                                Image(systemName: "xmark")
                            }
                            .buttonStyle(.plain)
                            .focusEffectDisabled()
                            .help("Delete \u{201C}\(preset.name)\u{201D}")
                        }
                    }
                    Divider()
                }
                HStack(spacing: 6) {
                    TextField("Save current as…", text: $presetNameDraft)
                        .textFieldStyle(.plain)
                        .onSubmit(commitPresetSave)
                    Button("Save", action: commitPresetSave)
                        .buttonStyle(.plain)
                        .foregroundStyle(.tint)
                }
            }
            .padding(10)
            .frame(minWidth: 220)
        }
    }

    private func commitPresetSave() {
        guard !presetNameDraft.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        saveCurrentPreset(presetNameDraft)
        presetNameDraft = ""
    }

    // MARK: - Columns (#43)

    /// Columns menu: a popover split into a Visible list and, below a "Hidden"
    /// label, everything not currently shown — mirrors Tauri's
    /// `renderColumnsMenu`. Two pieces of that popover are NOT reproduced here,
    /// both for the same reason: SwiftUI's `Table` declares its columns in a
    /// fixed source order (conditionally shown or hidden, never reordered at
    /// runtime) and has no per-column width state to act on, so there is
    /// nothing for a drag handle or a "Fit to content"/"Autofit" control to
    /// drive without rebuilding the table on a custom grid.
    private var columnsMenu: some View {
        Button {
            showColumnsPopover.toggle()
        } label: {
            Image(systemName: "tablecells")
        }
        .buttonStyle(.borderless)
        .focusEffectDisabled()
        .help("Choose which columns to show")
        .popover(isPresented: $showColumnsPopover, arrowEdge: .bottom) {
            let visible = TrackTable.optionalColumns.filter { visibleColumns.contains($0.key) }
            let hidden = TrackTable.optionalColumns.filter { !visibleColumns.contains($0.key) }
            VStack(alignment: .leading, spacing: 2) {
                ForEach(visible, id: \.key) { column in
                    Toggle(column.label, isOn: Binding(
                        get: { true }, set: { _ in toggleColumn(column.key) }
                    ))
                    .toggleStyle(.checkbox)
                }
                if !hidden.isEmpty {
                    Text("Hidden")
                        .font(AppFonts.sans(10, .semibold))
                        .foregroundStyle(.secondary)
                        .padding(.top, 6)
                    ForEach(hidden, id: \.key) { column in
                        Toggle(column.label, isOn: Binding(
                            get: { false }, set: { _ in toggleColumn(column.key) }
                        ))
                        .toggleStyle(.checkbox)
                    }
                }
                Divider().padding(.vertical, 4)
                Button(customMask.isEmpty ? "Add column…" : "Edit column…") {
                    customDraft = customMask
                    showCustomPrompt = true
                }
                .buttonStyle(.plain)
                .foregroundStyle(.tint)
                if !customMask.isEmpty {
                    Button("Remove column", role: .destructive) { customMask = "" }
                        .buttonStyle(.plain)
                }
                Button("Reset", action: resetColumns)
                    .buttonStyle(.plain)
                    .foregroundStyle(.tint)
                    .padding(.top, 4)
            }
            .padding(10)
            .frame(minWidth: 200)
        }
    }

    /// Back to the default set, order and (nothing else — this stand has no
    /// per-column widths yet) — mirrors Tauri's `resetColumns`.
    private func resetColumns() {
        columnsCSV = "artist,title,album,year,length"
    }

    /// "Rules to run on what this panel produces" (`transform-btn`): opens the
    /// shared chain editor; a non-empty, enabled chain runs automatically over
    /// whatever any mode stages next (see the `onChange(of:)` on the table).
    private var transformShortcutButton: some View {
        Button {
            showTransformShortcut = true
        } label: {
            Image(systemName: "wand.and.stars")
                .foregroundStyle(transformShortcutActive ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
        }
        .buttonStyle(.borderless)
        .focusEffectDisabled()
        .help(transformShortcutActive
              ? "Rules to run on what this panel produces — \(transformShortcutRules.filter(\.enabled).count) step(s) set"
              : "Rules to run on what this panel produces — not set")
        .popover(isPresented: $showTransformShortcut) {
            VStack(alignment: .leading, spacing: 10) {
                Text("Auto-run on every staged plan")
                    .font(.caption).fontWeight(.semibold).foregroundStyle(.secondary)
                ChainEditor(rules: $transformShortcutRules)
                    .frame(width: 320)
                HStack {
                    Toggle("Active", isOn: $transformShortcutActive)
                        .toggleStyle(.checkbox)
                    Spacer()
                    Button("Close") { showTransformShortcut = false }
                }
            }
            .padding(12)
        }
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
            groupBy: groupBy,
            rootPath: library.root?.path,
            visibleColumns: visibleColumns,
            customMask: customMask,
            customValues: customValues,
            onPlayToggle: { track in
                if library.playerStatus?.path == track.id {
                    library.togglePause()
                } else {
                    library.play(track.id, queue: rows.map(\.id))
                }
            },
            collapsedGroups: $collapsedGroups
        )
            .task(id: "\(customMask)|\(rows.count)|\(rows.first?.id ?? "")|\(rows.last?.id ?? "")") {
                customValues = customMask.isEmpty
                    ? [:]
                    : await library.renderColumn(pattern: customMask, paths: rows.map(\.id))
            }
            .overlay(alignment: .bottom) {
                if library.hasStagedPlan { ChangePlanBar(library: library) }
            }
            // Table controls above the grid, not in the top strip — the way the
            // Tauri `.view-tabs` bar carries grouping, columns and the filter.
            // Keeping them out of the toolbar leaves the modes and folder path
            // room, so a long album-folder name can't push anything into the
            // ">>" overflow, and the filter field works in the normal hierarchy
            // (no toolbar-over-inspector focus trap).
            .safeAreaInset(edge: .top, spacing: 0) {
                header
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                StatusBar(
                    library: library,
                    queue: rows.map(\.id),
                    selectedFirst: rows.first { visibleSelection.contains($0.id) }?.id
                )
            }
            .inspector(isPresented: $showsInspector) {
                ModePanel(library: library, mode: mode, selection: visibleSelection)
                    // The selection counter lives under the PANEL (Tauri's
                    // sb-right, #289), a separate zone from the player's — that
                    // split is what lets the waveform run the table's full width
                    // instead of stopping short to leave room for this text.
                    .safeAreaInset(edge: .bottom, spacing: 0) {
                        SelectionSummary(
                            shown: rows.count,
                            total: library.tracks.count,
                            selected: visibleSelection.count,
                            hidden: hiddenSelectionCount
                        )
                    }
                    // A 75/25 split (table/panel): on the ~1728-wide window the
                    // panel is 432 = 25%, and that is also its minimum so it never
                    // gets narrow enough to cramp the release cards.
                    .inspectorColumnWidth(min: 432, ideal: 432, max: 720)
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
            // The native window toolbar carries nothing now — every control that
            // used to live there (path, undo, settings, the panel toggle) moved
            // into `header`, the content row that also holds the brand and mode
            // tabs, matching Tauri's single `<header class="topbar">` strip.
            // Splitting those controls across a native toolbar row AND a content
            // row (the previous layout) put them somewhere Tauri never puts them.
            .toolbar(removing: .title)
            .sheet(isPresented: $showingSettings) {
                SettingsView(library: library)
            }
            .alert("Custom column", isPresented: $showCustomPrompt) {
                TextField("%artist% - %album%", text: $customDraft)
                Button("Set") { customMask = customDraft.trimmingCharacters(in: .whitespaces) }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("A mask rendered per row as an extra column — e.g. %catalognumber% or $upper(%genre%).")
            }
            .alert("Open path", isPresented: $showPathPrompt) {
                TextField("/path/to/music/library", text: $pathDraft)
                Button("Open") {
                    let path = pathDraft.trimmingCharacters(in: .whitespaces)
                    if !path.isEmpty { Task { await openFolder(URL(fileURLWithPath: path)) } }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("Type or paste a folder path to open.")
            }
            // The transform shortcut (#toolbar.transformTitle) auto-runs its
            // chain over whatever any mode just staged. hasStagedPlan flips
            // false→true exactly once per stage (an already-true→true update,
            // which is what applying the chain itself produces, is not a
            // change, so this can't re-trigger itself).
            .onChange(of: library.hasStagedPlan) { wasStaged, isStaged in
                guard isStaged, !wasStaged, transformShortcutActive else { return }
                let groups = [ActionGroup(name: "shortcut", scope: "tags",
                                          rules: transformShortcutRules.map(\.transformRule))]
                Task { _ = await library.transformOverStagedPlan(groups: groups) }
            }
            .fileImporter(isPresented: $choosingFolder, allowedContentTypes: [.folder]) { result in
                guard case .success(let folder) = result else { return }
                Task { await openFolder(folder) }
            }
            .navigationTitle("TagRex")
            .task {
                // Opening a folder by hand is a dialog; for screenshots, CI and
                // a quick look at a known library, TAGREX_SPIKE_ROOT skips it.
                guard let path = ProcessInfo.processInfo.environment["TAGREX_SPIKE_ROOT"],
                      !path.isEmpty
                else { return }
                await openFolder(URL(fileURLWithPath: path))
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
    /// The regex doesn't compile — colour the text red (the filter is inert).
    var invalid = false

    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField()
        field.placeholderString = "Filter… (try artist:aphex)"
        field.delegate = context.coordinator
        // Filter as it is typed; the table is in memory and the plan is staged,
        // so there is nothing to defer until Return.
        field.sendsSearchStringImmediately = true
        field.sendsWholeSearchString = false
        return field
    }

    func updateNSView(_ field: NSSearchField, context: Context) {
        context.coordinator.text = $text
        field.textColor = invalid ? .systemRed : nil
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
    /// Which field rows are grouped by under a section header, empty for
    /// ungrouped (#129, #43) — matches the web UI's `groupBy`, whose default is
    /// "folder".
    let groupBy: String
    /// The open library root, so a folder header reads relative to it
    /// ("gui-test/CD1") rather than as an absolute path.
    let rootPath: String?
    /// Which optional columns are shown (#43, T2). File is always present.
    let visibleColumns: Set<String>
    /// A user-defined mask column (T2 custom column), empty when unset, plus the
    /// rendered value per path.
    let customMask: String
    let customValues: [String: String]
    /// Double-clicking the File cell of a row plays it (or toggles pause if it's
    /// already the loaded track) — mirrors the Tauri `td.file` dblclick.
    let onPlayToggle: (Track) -> Void

    /// Which folder groups are collapsed (view-only; never reorders `rows`) —
    /// toggled by clicking a group header's caret, mirroring Tauri's
    /// `collapsedGroups`/`toggleGroup`. A binding (not local @State) so the
    /// header's "Collapse all groups" button (`toggle-groups`) can drive it too.
    @Binding var collapsedGroups: Set<String>

    /// The optional columns the picker offers, in display order — mirrors
    /// Tauri's full pool (`allColumnKeys`: EXTENDED_FIELDS + the virtual
    /// columns), not just the handful shown by default. Each backs a real
    /// KeyPath so the column stays sortable ("position" is left out — it
    /// reconstructs the vinyl side notation from media+disc+track, a piece of
    /// logic this stand doesn't have yet).
    static let optionalColumns: [(key: String, label: String)] = [
        ("artist", "Artist"), ("title", "Title"), ("album", "Album"),
        ("albumartist", "Album Artist"), ("track", "Track"), ("tracktotal", "Track Total"),
        ("disc", "Disc"), ("year", "Year"), ("genre", "Genre"), ("comment", "Comment"),
        ("composer", "Composer"), ("publisher", "Publisher"), ("catalognumber", "Catalogue #"),
        ("bpm", "BPM"), ("isrc", "ISRC"), ("key", "Key"), ("url", "URL"), ("media", "Media"),
        ("length", "Length"), ("tagtypes", "Tag types"),
    ]

    var body: some View {
        Table(of: Track.self, selection: $selection, sortOrder: $sortOrder) {
            TableColumn("File", value: \.file) { track in
                DiffCell(
                    value: renames[track.id] ?? track.file,
                    old: renames[track.id] == nil ? nil : track.file,
                    showsOld: showsOldValues
                )
                .contentShape(Rectangle())
                .onTapGesture(count: 2) { onPlayToggle(track) }
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
            // The rest of EXTENDED_FIELDS, nested in builder sub-expressions to
            // stay under SwiftUI's 10-column-per-block limit (each nested
            // property only costs one slot in this block, however many columns
            // it holds internally, up to its own 10-column cap).
            extendedColumnsA
            extendedColumnsB
        } rows: {
            if !groupBy.isEmpty {
                ForEach(groupedRows, id: \.key) { group in
                    let isCollapsed = collapsedGroups.contains(group.key)
                    Section {
                        // Collapsed = no rows, not a hidden row, mirroring Tauri
                        // (collapsed groups are removed from the model entirely).
                        ForEach(isCollapsed ? [] : group.tracks) { TableRow($0) }
                    } header: {
                        // The group header as an accent band (#281): the name in
                        // the accent colour, on a faint accent wash with a
                        // leading accent bar, so it reads as a section boundary.
                        // The caret collapses/expands (click); the name selects
                        // the whole group (double-click) — mirrors Tauri, where
                        // those two gestures are deliberately kept apart so a
                        // plain click on the name never wipes the selection.
                        HStack(spacing: 6) {
                            Button {
                                if isCollapsed { collapsedGroups.remove(group.key) }
                                else { collapsedGroups.insert(group.key) }
                            } label: {
                                // A 12pt glyph in a 12pt frame was the actual
                                // clickable area — easy to miss by a couple of
                                // points. The frame is now real click-target size
                                // (22×22, full header height) with the glyph
                                // centred in it and the whole frame tappable, not
                                // just the drawn pixels.
                                Image(systemName: isCollapsed ? "chevron.right" : "chevron.down")
                                    .font(.system(size: 9, weight: .semibold))
                                    .foregroundStyle(.tint)
                                    .frame(width: 22, height: 22)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .focusEffectDisabled()

                            RoundedRectangle(cornerRadius: 1)
                                .fill(.tint)
                                .frame(width: 3, height: 12)
                            Text(group.label)
                                .font(AppFonts.sans(11, .semibold))
                                .foregroundStyle(.tint)
                                .contentShape(Rectangle())
                                .onTapGesture(count: 2) {
                                    selection = Set(group.tracks.map(\.id))
                                }
                            Spacer(minLength: 0)
                        }
                        .padding(.vertical, 3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.appAccent.opacity(0.12))
                    }
                }
            } else {
                ForEach(rows) { TableRow($0) }
            }
        }
        .tableStyle(.inset(alternatesRowBackgrounds: true))
    }

    /// Rows bucketed by `groupBy`, in first-appearance order over `rows` (which
    /// already reflects the table's own sort) — mirrors Tauri's `buildViewModel`
    /// grouping pass: groups never reorder the underlying files, they're a view
    /// overlay over whatever order the rows already have.
    private var groupedRows: [(key: String, label: String, tracks: [Track])] {
        var buckets: [String: [Track]] = [:]
        var order: [String] = []
        for track in rows {
            let key = track.groupKey(by: groupBy)
            if buckets[key] == nil { order.append(key) }
            buckets[key, default: []].append(track)
        }
        return order.map { (key: $0, label: groupHeaderLabel($0), tracks: buckets[$0]!) }
    }

    /// The header text for a group bucket — mirrors Tauri's `groupLabel`: the
    /// folder grouping reads relative to the session root, "no release id" is
    /// its own wording, and everything else falls back to "(no <Field>)".
    private func groupHeaderLabel(_ key: String) -> String {
        if key.isEmpty {
            if groupBy == "folder" { return "(no folder)" }
            if groupBy == "release" { return "(no release id)" }
            return "(no \(GroupField.label(for: groupBy).lowercased()))"
        }
        if groupBy == "folder" { return folderLabel(key) }
        return key
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

    /// The rest of EXTENDED_FIELDS plus Length, split into two builder
    /// properties (10 columns is SwiftUI's cap per `@TableColumnBuilder`
    /// block, and this alone would be 11). Each column is a plain tag lookup
    /// through the diff-aware `cell(_:key:current:)`, same as Catalogue #.
    @TableColumnBuilder<Track, KeyPathComparator<Track>>
    private var extendedColumnsA: some TableColumnContent<Track, KeyPathComparator<Track>> {
        if visibleColumns.contains("tracktotal") {
            TableColumn("Track Total", value: \.tracktotal) { cell($0, key: "tracktotal", current: $0.tracktotal) }
                .width(64)
        }
        if visibleColumns.contains("disc") {
            TableColumn("Disc", value: \.disc) { cell($0, key: "disc", current: $0.disc) }
                .width(56)
        }
        if visibleColumns.contains("comment") {
            TableColumn("Comment", value: \.comment) { cell($0, key: "comment", current: $0.comment) }
                .width(min: 90, ideal: 160)
        }
        if visibleColumns.contains("composer") {
            TableColumn("Composer", value: \.composer) { cell($0, key: "composer", current: $0.composer) }
                .width(min: 90, ideal: 150)
        }
        if visibleColumns.contains("publisher") {
            TableColumn("Publisher", value: \.publisher) { cell($0, key: "publisher", current: $0.publisher) }
                .width(min: 90, ideal: 150)
        }
        if visibleColumns.contains("catalognumber") {
            TableColumn("Catalogue #", value: \.catalognumber) { cell($0, key: "catalognumber", current: $0.catalognumber) }
                .width(min: 80, ideal: 120)
        }
        if visibleColumns.contains("bpm") {
            TableColumn("BPM", value: \.bpm) { cell($0, key: "bpm", current: $0.bpm) }
                .width(56)
        }
        if visibleColumns.contains("isrc") {
            TableColumn("ISRC", value: \.isrc) { cell($0, key: "isrc", current: $0.isrc) }
                .width(min: 80, ideal: 120)
        }
        if visibleColumns.contains("key") {
            TableColumn("Key", value: \.key) { cell($0, key: "key", current: $0.key) }
                .width(56)
        }
        if visibleColumns.contains("length") {
            TableColumn("Length", value: \.durationSort) { track in
                Text(track.duration.isEmpty ? "—" : track.duration)
                    .font(AppFonts.sans(11))
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            .width(64)
        }
    }

    /// The remaining fields, plus the read-only "Tag types" virtual column and
    /// the custom mask column — kept apart from `extendedColumnsA` only to stay
    /// under the per-block cap.
    @TableColumnBuilder<Track, KeyPathComparator<Track>>
    private var extendedColumnsB: some TableColumnContent<Track, KeyPathComparator<Track>> {
        if visibleColumns.contains("url") {
            TableColumn("URL", value: \.url) { cell($0, key: "url", current: $0.url) }
                .width(min: 90, ideal: 160)
        }
        if visibleColumns.contains("media") {
            TableColumn("Media", value: \.media) { cell($0, key: "media", current: $0.media) }
                .width(min: 70, ideal: 90)
        }
        if visibleColumns.contains("tagtypes") {
            // Derived, read-only (#47) — no staged/old diff, same as Length.
            TableColumn("Tag types", value: \.tagtypes) { track in
                Text(track.tagtypes.isEmpty ? "—" : track.tagtypes)
                    .font(AppFonts.sans(11))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .width(min: 90, ideal: 140)
        }
        // The custom mask column (T2) — computed, so not sortable.
        if !customMask.isEmpty {
            TableColumn(customMask) { track in
                Text(customValues[track.id] ?? "")
                    .font(AppFonts.sans(11))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .width(min: 90, ideal: 160)
        }
    }

    private func cell(_ track: Track, _ field: Field) -> DiffCell {
        cell(track, key: field.rawValue, current: track.value(for: field))
    }

    /// A diff cell for any storage key (used by the Catalogue # column, which is
    /// an extended field outside the modeled `Field` enum).
    private func cell(_ track: Track, key: String, current: String) -> DiffCell {
        let stagedValue = staged[track.id]?[key]
        return DiffCell(
            value: stagedValue ?? current,
            old: stagedValue == nil ? nil : current,
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
                .font(AppFonts.sans(11))
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
                        .menuIndicator(.hidden)
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
    /// bar walks and where Play starts. The selection counter itself lives under
    /// the panel now (`SelectionSummary`), not here — see the comment where it's
    /// attached.
    let queue: [String]
    let selectedFirst: String?

    var body: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 12) {
                // The player fills the bar so its waveform spans the whole panel
                // (the Tauri player bar), instead of a fixed stub with dead space.
                PlayerBar(library: library, queue: queue, selectedFirst: selectedFirst)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if library.isBusy {
                    ProgressView().controlSize(.small)
                }
                if !library.lastMessage.isEmpty {
                    Text(library.lastMessage).lineLimit(1)
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
        }
        .background(.bar)
    }
}

/// The counter (`sb-status`, Tauri #289): named the same way Tauri names it, but
/// positioned in its OWN zone under the side panel, not appended to the player
/// row. The two-zone split matters — it's what lets the player's waveform run
/// the full width of the table instead of stopping early to make room for text.
struct SelectionSummary: View {
    let shown: Int
    let total: Int
    let selected: Int
    let hidden: Int

    private var isFiltered: Bool { shown != total }

    var body: some View {
        VStack(spacing: 0) {
            Divider()
            HStack {
                Spacer()
                Text(text)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
        }
        .background(.bar)
    }

    /// Names the hidden part of the selection rather than leaving the panel to
    /// go quiet for no visible reason.
    private var text: String {
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
