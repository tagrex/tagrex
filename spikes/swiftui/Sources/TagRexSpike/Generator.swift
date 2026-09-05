// The Generator panel (#303): pick a scope and one transform rule, preview what
// it changes, and stage it through the same gate. One rule for now; a chain of
// them is a later step.

import SwiftUI

@MainActor
struct GeneratorPanel: View {
    let library: Library
    let selection: Set<Track.ID>

    /// scope key → label, in menu order.
    private static let scopes: [(String, String)] = [
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

    @State private var scope = "title"
    @State private var kind = "case"

    // Rule args.
    @State private var caseStyle = "title"
    @State private var replaceFrom = ""
    @State private var replaceTo = ""
    @State private var regex = false
    @State private var wholeWord = false
    @State private var caseSensitive = false

    @State private var pairs: [TransformPair] = []
    @State private var error: String?
    @State private var isStaging = false

    private var paths: [String] {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? library.visibleTracks.map(\.id) : selected
    }

    /// The rule the current controls describe.
    private var rule: TransformRule {
        switch kind {
        case "case":
            TransformRule(kind: "case", style: caseStyle)
        case "replace":
            TransformRule(
                kind: "replace", from: replaceFrom, to: replaceTo,
                regex: regex, whole_word: wholeWord, case_sensitive: caseSensitive)
        default:
            TransformRule(kind: kind)
        }
    }

    private var refreshKey: String {
        "\(scope)|\(kind)|\(caseStyle)|\(replaceFrom)|\(replaceTo)|\(regex)\(wholeWord)\(caseSensitive)|\(paths.count)|\(paths.first ?? "")"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            form
            Divider()
            preview
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .task(id: refreshKey) { await refresh() }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 10) {
            labeled("Apply to") {
                Picker("", selection: $scope) {
                    ForEach(Self.scopes, id: \.0) { Text($0.1).tag($0.0) }
                }
                .labelsHidden()
            }

            labeled("Rule") {
                Picker("", selection: $kind) {
                    Text("Change case").tag("case")
                    Text("Find & replace").tag("replace")
                    Text("Strip accents").tag("diacritics")
                    Text("Transliterate").tag("transliterate")
                }
                .labelsHidden()
            }

            ruleArgs

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
    private var ruleArgs: some View {
        switch kind {
        case "case":
            Picker("", selection: $caseStyle) {
                Text("lower case").tag("lower")
                Text("UPPER CASE").tag("upper")
                Text("Title Case").tag("title")
                Text("Sentence case").tag("sentence")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        case "replace":
            VStack(alignment: .leading, spacing: 6) {
                TextField("Find", text: $replaceFrom).textFieldStyle(.roundedBorder)
                TextField("Replace with", text: $replaceTo).textFieldStyle(.roundedBorder)
                HStack(spacing: 12) {
                    Toggle("Regex", isOn: $regex)
                    Toggle("Whole word", isOn: $wholeWord)
                    Toggle("Case", isOn: $caseSensitive)
                }
                .toggleStyle(.checkbox)
                .font(.caption)
            }
        default:
            EmptyView()
        }
    }

    private var scopeLabel: String {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        let base = selected.isEmpty ? "all \(paths.count) file(s)" : "\(paths.count) selected"
        return pairs.isEmpty ? base : "\(pairs.count) change · \(base)"
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
                description: Text("This rule leaves every value as it is.")
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

    private func labeled<Content: View>(_ label: String, @ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: 10) {
            Text(label).frame(width: 68, alignment: .leading).foregroundStyle(.secondary)
            content()
        }
    }

    private func refresh() async {
        try? await Task.sleep(for: .milliseconds(250))
        if Task.isCancelled { return }
        error = nil
        switch await library.transformPreview(rules: [rule], scope: scope, paths: paths) {
        case .success(let found): pairs = found
        case .failure(let failure): pairs = []; error = failure.message
        }
    }

    private func stage() {
        isStaging = true
        Task {
            if case .failure(let failure) = await library.stageTransform(
                rules: [rule], scope: scope, paths: paths) {
                error = failure.message
            }
            isStaging = false
        }
    }
}
