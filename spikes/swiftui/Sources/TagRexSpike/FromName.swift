// The "From name" panel (#306): read a file's name through a mask and stage the
// captured values into its tags — the extract direction of the rename grammar.

import SwiftUI

@MainActor
struct FromNamePanel: View {
    let library: Library
    let selection: Set<Track.ID>

    @State private var mask = "%artist% - %title%"
    @State private var probe: NameProbe?
    @State private var error: String?
    @State private var isStaging = false

    /// The files to write onto: the selection, or every visible row.
    private var paths: [String] {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? library.visibleTracks.map(\.id) : selected
    }

    /// The file the live probe reads — the first target.
    private var probePath: String? { paths.first }

    private var refreshKey: String { "\(mask)|\(probePath ?? "")" }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            form
            Divider()
            probeView
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .task(id: refreshKey) { await refresh() }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Name mask")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)
            TextField("%artist% - %title%", text: $mask)
                .textFieldStyle(.roundedBorder)
                .font(AppFonts.mono)
            Text("The mask reads values out of the file's name into its tags.")
                .font(.caption)
                .foregroundStyle(.tertiary)
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
                .disabled(isStaging || probe?.matched != true)
            }
        }
        .padding(12)
    }

    private var scopeLabel: String {
        let selected = library.tracks.map(\.id).filter(selection.contains)
        return selected.isEmpty ? "all \(paths.count) file(s)" : "\(paths.count) selected"
    }

    @ViewBuilder
    private var probeView: some View {
        if let error {
            ContentUnavailableView {
                Label("Could not read the name", systemImage: "exclamationmark.triangle")
            } description: {
                Text(error)
            }
        } else if probePath == nil {
            ContentUnavailableView(
                "No files",
                systemImage: "doc",
                description: Text("Open a folder to read names from.")
            )
        } else if let probe {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Reading").font(.caption).foregroundStyle(.secondary)
                        Text(probe.subject).font(AppFonts.mono).lineLimit(2)
                    }

                    if probe.matched {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Captured").font(.caption).foregroundStyle(.secondary)
                            ForEach(Array(probe.pairs.enumerated()), id: \.offset) { _, pair in
                                HStack(spacing: 10) {
                                    Text(fieldLabel(pair.field))
                                        .frame(width: 92, alignment: .leading)
                                        .foregroundStyle(.secondary)
                                    Text(pair.value.isEmpty ? "—" : pair.value)
                                        .foregroundStyle(.green)
                                }
                                .font(AppFonts.body)
                            }
                        }
                    } else {
                        Label("The mask does not match this name.", systemImage: "xmark.circle")
                            .font(.callout)
                            .foregroundStyle(.orange)
                    }
                }
                .padding(14)
            }
        }
    }

    /// A field key's display label, falling back to the raw key for anything the
    /// stand's Field enum does not name.
    private func fieldLabel(_ key: String) -> String {
        Field(rawValue: key)?.label ?? key
    }

    private func refresh() async {
        guard let path = probePath else { probe = nil; return }
        try? await Task.sleep(for: .milliseconds(250))
        if Task.isCancelled { return }
        error = nil
        switch await library.probeFromName(mask: mask, path: path) {
        case .success(let result): probe = result
        case .failure(let failure): probe = nil; error = failure.message
        }
    }

    private func stage() {
        isStaging = true
        Task {
            if case .failure(let failure) = await library.stageFromName(mask: mask, paths: paths) {
                error = failure.message
            }
            isStaging = false
        }
    }
}
