// The Generator panel (#303, #57): build a chain of transform rules — a case
// change, a find & replace, an accent strip — each optionally targeting its own
// field, preview what the whole chain changes, and stage it through the same
// gate. Shipped presets (builtin action groups) load into the editor. One
// action group, run through preview_transform_groups.

import SwiftUI

@MainActor
struct GeneratorPanel: View {
    let library: Library
    let selection: Set<Track.ID>

    /// scope key → label, in menu order. The first entries are the whole-tag /
    /// filename scopes; the field scopes let a rule target one column.
    static let scopes: [(String, String)] = [
        ("tags", "All tags"),
        ("artist", "Artist"),
        ("title", "Title"),
        ("album", "Album"),
        ("albumartist", "Album artist"),
        ("year", "Year"),
        ("genre", "Genre"),
        ("track", "Track"),
        ("filename", "Filename"),
        ("fileext", "Extension"),
    ]

    /// The rule kinds the editor offers args for. Others (loaded from a preset)
    /// still run; they just show no arg row.
    private static let kinds: [(String, String)] = [
        ("case", "Change case"),
        ("replace", "Find & replace"),
        ("diacritics", "Strip accents"),
        ("transliterate", "Transliterate"),
        ("key", "Musical key"),
    ]

    private enum GenMode: String, CaseIterable, Identifiable {
        case rules, number, vinyl
        var id: Self { self }
        var title: String {
            switch self {
            case .rules: "Rules"
            case .number: "Number"
            case .vinyl: "Vinyl"
            }
        }
    }

    @State private var genMode: GenMode = .rules

    @State private var groupScope = "title"
    @State private var rules: [ChainRule] = [ChainRule()]
    @State private var builtins: [ActionGroup] = []

    // Number-tracks form.
    @State private var numberStart = "1"
    @State private var numberTotal = true
    @State private var numberDisc = ""

    @State private var pairs: [TransformPair] = []
    @State private var error: String?
    @State private var isStaging = false

    private var paths: [String] {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? library.visibleTracks.map(\.id) : selected
    }

    /// The action group the chain describes.
    private var group: ActionGroup {
        ActionGroup(name: "chain", scope: groupScope, rules: rules.map(\.transformRule))
    }

    private var enabledCount: Int { rules.filter(\.enabled).count }

    private var refreshKey: String {
        let sig = rules.map { "\($0.kind)\($0.style)\($0.from)\($0.to)\($0.regex)\($0.wholeWord)\($0.caseSensitive)\($0.enabled)\($0.scopeOverride ?? "")" }.joined()
        return "\(groupScope)|\(sig)|\(paths.count)|\(paths.first ?? "")"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Picker("", selection: $genMode) {
                ForEach(GenMode.allCases) { Text($0.title.uppercased()).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(12)
            Divider()

            switch genMode {
            case .rules:
                form
                Divider()
                preview
            case .number:
                numberForm
            case .vinyl:
                vinylForm
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .task(id: refreshKey) { await refresh() }
        .task { builtins = await library.builtinActionGroups() }
    }

    // MARK: - Number tracks (#G-2)

    private var numberForm: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Text("Start at").foregroundStyle(.secondary)
                TextField("1", text: $numberStart)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 60)
                    .multilineTextAlignment(.trailing)
            }
            Toggle("Write track total", isOn: $numberTotal)
                .toggleStyle(.checkbox)
            HStack(spacing: 8) {
                Text("Disc #").foregroundStyle(.secondary)
                TextField("(leave empty)", text: $numberDisc)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 60)
                    .multilineTextAlignment(.trailing)
            }
            Text("Numbers the \(scopeCount) in table order, from the start value.")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Stage numbering") {
                    let start = max(0, Int(numberStart.trimmingCharacters(in: .whitespaces)) ?? 1)
                    let disc = numberDisc.trimmingCharacters(in: .whitespaces)
                    library.numberTracks(paths: paths, start: start, writeTotal: numberTotal,
                                         disc: disc.isEmpty ? nil : disc)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .disabled(paths.isEmpty)
            }
            Spacer()
        }
        .font(AppFonts.body)
        .padding(14)
    }

    // MARK: - Split vinyl sides (#G-3)

    private var vinylForm: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Split vinyl-side positions")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)
            Text("Turns a side position in the track tag (A1, B2) into a disc and a track number — side A → disc 1, B → disc 2.")
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Split vinyl sides") {
                    _ = library.splitVinylSides(paths: paths)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .disabled(paths.isEmpty)
            }
            Spacer()
        }
        .font(AppFonts.body)
        .padding(14)
    }

    private var scopeCount: String {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? "\(paths.count) visible file(s)" : "\(paths.count) selected file(s)"
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text("Apply to").foregroundStyle(.secondary)
                Picker("", selection: $groupScope) {
                    ForEach(Self.scopes, id: \.0) { Text($0.1).tag($0.0) }
                }
                .labelsHidden()
                Spacer()
                presetMenu
            }

            ForEach($rules) { $rule in
                ruleCard($rule)
            }

            Button {
                rules.append(ChainRule())
            } label: {
                Label("Add rule", systemImage: "plus")
            }
            .buttonStyle(.borderless)

            HStack {
                Text(scopeLabel).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button {
                    stage()
                } label: {
                    if isStaging { ProgressView().controlSize(.small) } else { Text("Stage") }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .disabled(isStaging || pairs.isEmpty)
            }
        }
        .font(AppFonts.body)
        .padding(12)
    }

    @ViewBuilder
    private var presetMenu: some View {
        Menu {
            if builtins.isEmpty {
                Text("No presets").disabled(true)
            } else {
                ForEach(builtins) { preset in
                    Button {
                        loadPreset(preset)
                    } label: {
                        Text(preset.note.isEmpty ? preset.name : "\(preset.name) — \(preset.note)")
                    }
                }
            }
        } label: {
            Label("Presets", systemImage: "square.stack.3d.up")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("Load a shipped rule chain")
    }

    // MARK: - Rule card

    @ViewBuilder
    private func ruleCard(_ rule: Binding<ChainRule>) -> some View {
        let index = rules.firstIndex { $0.id == rule.wrappedValue.id } ?? 0
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Picker("", selection: rule.kind) {
                    ForEach(Self.kinds, id: \.0) { Text($0.1).tag($0.0) }
                    // Keep a loaded preset's unknown kind selectable, not blanked.
                    if !Self.kinds.contains(where: { $0.0 == rule.wrappedValue.kind }) {
                        Text(rule.wrappedValue.kind).tag(rule.wrappedValue.kind)
                    }
                }
                .labelsHidden()
                .controlSize(.small)
                Spacer()
                Toggle("On", isOn: rule.enabled)
                    .toggleStyle(.checkbox)
                    .controlSize(.small)
                    .labelsHidden()
                    .help("Include this step")
                Button { move(index, by: -1) } label: { Image(systemName: "chevron.up") }
                    .buttonStyle(.borderless).controlSize(.small).disabled(index == 0)
                Button { move(index, by: 1) } label: { Image(systemName: "chevron.down") }
                    .buttonStyle(.borderless).controlSize(.small).disabled(index == rules.count - 1)
                Button { rules.removeAll { $0.id == rule.wrappedValue.id } } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless).controlSize(.small).disabled(rules.count == 1)
            }

            ruleArgs(rule)

            HStack(spacing: 6) {
                Text("on").font(.caption).foregroundStyle(.tertiary)
                Picker("", selection: rule.scopeOverride) {
                    Text("(group scope)").tag(String?.none)
                    ForEach(Self.scopes, id: \.0) { Text($0.1).tag(String?.some($0.0)) }
                }
                .labelsHidden()
                .controlSize(.small)
                .fixedSize()
            }
        }
        .padding(8)
        .background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color.cardBorder, lineWidth: 1))
        .opacity(rule.wrappedValue.enabled ? 1 : 0.5)
    }

    @ViewBuilder
    private func ruleArgs(_ rule: Binding<ChainRule>) -> some View {
        switch rule.wrappedValue.kind {
        case "case":
            Picker("", selection: rule.style) {
                Text("lower case").tag("lower")
                Text("UPPER CASE").tag("upper")
                Text("Title Case").tag("title")
                Text("Sentence case").tag("sentence")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .controlSize(.small)
        case "replace":
            VStack(alignment: .leading, spacing: 4) {
                TextField("Find", text: rule.from).textFieldStyle(.roundedBorder)
                TextField("Replace with", text: rule.to).textFieldStyle(.roundedBorder)
                HStack(spacing: 12) {
                    Toggle("Regex", isOn: rule.regex)
                    Toggle("Whole word", isOn: rule.wholeWord)
                    Toggle("Case", isOn: rule.caseSensitive)
                }
                .toggleStyle(.checkbox)
                .font(.caption)
            }
        case "key":
            Picker("", selection: rule.style) {
                Text("Camelot").tag("camelot")
                Text("Open Key").tag("openkey")
                Text("Musical").tag("musical")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .controlSize(.small)
        default:
            EmptyView()
        }
    }

    private func move(_ index: Int, by delta: Int) {
        let target = index + delta
        guard rules.indices.contains(index), rules.indices.contains(target) else { return }
        rules.swapAt(index, target)
    }

    private func loadPreset(_ preset: ActionGroup) {
        groupScope = preset.scope
        rules = preset.rules.map(ChainRule.init(from:))
        if rules.isEmpty { rules = [ChainRule()] }
    }

    private var scopeLabel: String {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        let base = selected.isEmpty ? "all \(paths.count) file(s)" : "\(paths.count) selected"
        let steps = "\(enabledCount) step\(enabledCount == 1 ? "" : "s")"
        return pairs.isEmpty ? "\(steps) · \(base)" : "\(pairs.count) change · \(base)"
    }

    @ViewBuilder
    private var preview: some View {
        if let error {
            ContentUnavailableView {
                Label("Transform failed", systemImage: "exclamationmark.triangle")
            } description: {
                Text(error)
            }
        } else if pairs.isEmpty {
            ContentUnavailableView(
                "Nothing to change",
                systemImage: "wand.and.stars",
                description: Text("This chain leaves every value as it is.")
            )
        } else {
            List(pairs) { pair in
                VStack(alignment: .leading, spacing: 2) {
                    if pair.label != "file" {
                        Text(pair.label).font(.caption).foregroundStyle(.secondary)
                    }
                    HStack(spacing: 6) {
                        Text(pair.old.isEmpty ? "—" : pair.old)
                            .foregroundStyle(.tertiary)
                            .strikethrough()
                        Image(systemName: "arrow.right").font(.caption2).foregroundStyle(.tertiary)
                        Text(pair.new.isEmpty ? "—" : pair.new)
                            .foregroundStyle(.green)
                    }
                    .font(AppFonts.body)
                    .lineLimit(1)
                }
                .padding(.vertical, 1)
            }
            .listStyle(.inset)
        }
    }

    private func refresh() async {
        try? await Task.sleep(for: .milliseconds(250))
        if Task.isCancelled { return }
        error = nil
        switch await library.transformGroupsPreview(groups: [group], paths: paths) {
        case .success(let found): pairs = found
        case .failure(let failure): pairs = []; error = failure.message
        }
    }

    private func stage() {
        isStaging = true
        Task {
            if case .failure(let failure) = await library.stageTransformGroups(groups: [group], paths: paths) {
                error = failure.message
            }
            isStaging = false
        }
    }
}

/// One editable step in the chain. Holds every rule kind's args; `transformRule`
/// projects the fields the current kind uses into the backend rule.
struct ChainRule: Identifiable {
    let id = UUID()
    var kind = "case"
    var style = "title"
    var from = ""
    var to = ""
    var regex = false
    var wholeWord = false
    var caseSensitive = false
    var enabled = true
    /// nil = follow the group's scope; a scope key overrides it for this step.
    var scopeOverride: String?

    var transformRule: TransformRule {
        TransformRule(
            kind: kind, from: from, to: to, regex: regex, whole_word: wholeWord,
            case_sensitive: caseSensitive, style: style, enabled: enabled, scope: scopeOverride)
    }

    init() {}

    /// Build an editable step from a backend rule (loading a preset).
    init(from rule: TransformRule) {
        kind = rule.kind
        style = rule.style
        from = rule.from
        to = rule.to
        regex = rule.regex
        wholeWord = rule.whole_word
        caseSensitive = rule.case_sensitive
        enabled = rule.enabled
        scopeOverride = rule.scope
    }
}
