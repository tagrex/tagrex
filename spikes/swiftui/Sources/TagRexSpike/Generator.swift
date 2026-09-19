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

    // Save-chain-as-group prompt.
    @State private var savingGroup = false
    @State private var newGroupName = ""

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
        .alert("Save chain", isPresented: $savingGroup) {
            TextField("Name", text: $newGroupName)
            Button("Save") {
                let name = newGroupName.trimmingCharacters(in: .whitespaces)
                if !name.isEmpty {
                    var saved = group
                    saved.name = name
                    Task { await library.saveActionGroup(saved) }
                }
                newGroupName = ""
            }
            Button("Cancel", role: .cancel) { newGroupName = "" }
        } message: {
            Text("Save the current \(enabledCount)-step chain to reuse later.")
        }
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
                    ForEach(ChainEditor.scopes, id: \.0) { Text($0.1).tag($0.0) }
                }
                .labelsHidden()
                Spacer()
                presetMenu
            }

            ChainEditor(rules: $rules)

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

            // Layer the chain on top of an already-staged import or from-name
            // (#G-4), so its values are cleaned up before Apply.
            if library.hasStagedPlan {
                Button {
                    isStaging = true
                    Task {
                        _ = await library.transformOverStagedPlan(groups: [group])
                        isStaging = false
                    }
                } label: {
                    Label("Apply chain to staged changes", systemImage: "square.stack.3d.down.forward")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(isStaging || enabledCount == 0)
                .help("Run this chain over the changes already staged, before Apply")
            }
        }
        .font(AppFonts.body)
        .padding(12)
    }

    @ViewBuilder
    private var presetMenu: some View {
        Menu {
            Button("Save current chain…") { savingGroup = true }
            if !library.savedActionGroups.isEmpty {
                Section("Saved") {
                    ForEach(library.savedActionGroups) { preset in
                        Menu(preset.name) {
                            Button("Load") { loadPreset(preset) }
                            Button("Delete", role: .destructive) {
                                Task { await library.deleteActionGroup(named: preset.name) }
                            }
                        }
                    }
                }
            }
            if !builtins.isEmpty {
                Section("Built-in") {
                    ForEach(builtins) { preset in
                        Button {
                            loadPreset(preset)
                        } label: {
                            Text(preset.note.isEmpty ? preset.name : "\(preset.name) — \(preset.note)")
                        }
                    }
                }
            }
        } label: {
            Label("Presets", systemImage: "square.stack.3d.up")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("Save or load a rule chain")
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
