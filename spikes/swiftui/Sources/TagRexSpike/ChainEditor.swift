// The rule-chain editor (#57), shared by the Generator and From-name: a column
// of editable steps — kind, its args, an optional per-rule scope override,
// enable, reorder, remove — plus an Add button. The owner supplies the binding
// and decides what the chain runs over (files, or a staged plan). Extracted from
// GeneratorPanel so From-name can reuse it for its clean-up chain (F1).

import SwiftUI

@MainActor
struct ChainEditor: View {
    @Binding var rules: [ChainRule]
    /// Whether the per-rule scope override is offered. From-name runs over the
    /// captured tags, where a per-rule scope means little, so it hides it.
    var showsScope = true

    /// scope key → label, in menu order — the whole-tag/filename scopes plus the
    /// field scopes a rule can target.
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

    /// The rule kinds the editor offers args for. Others (from a preset) still
    /// run; they just show no arg row.
    static let kinds: [(String, String)] = [
        ("case", "Change case"),
        ("replace", "Find & replace"),
        ("diacritics", "Strip accents"),
        ("transliterate", "Transliterate"),
        ("key", "Musical key"),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach($rules) { $rule in
                ruleCard($rule)
            }
            Button {
                rules.append(ChainRule())
            } label: {
                Label("Add rule", systemImage: "plus")
            }
            .buttonStyle(.borderless)
        }
    }

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

            if showsScope {
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
