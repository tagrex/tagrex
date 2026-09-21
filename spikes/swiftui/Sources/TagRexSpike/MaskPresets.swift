// Named mask presets (#360): a small shared pool of saved %mask% patterns any
// mask field can drop in. Tauri keeps one pool for every mask input — the
// rename mask, the folder pattern, FROM NAME, the export mask — because the
// placeholder vocabulary is the same everywhere (app/ui/js/maskpresets.js).
//
// Tauri persists the pool through the backend settings file, alongside the
// saved transform chains. The shared command layer this stand's FFI sits on
// doesn't have a `mask_presets` field yet (a gap in the Tauri feature's own
// backend wiring, not something to paper over here), so this pool lives in
// AppStorage instead — same JSON-in-UserDefaults shape as the filter/sort
// presets in App.swift. Every mask field reads and writes the same key, so a
// preset saved from one shows up in all the others, matching the "one shared
// pool" behavior even though the storage differs.

import SwiftUI

struct MaskPreset: Codable, Identifiable, Equatable {
    var name: String
    var mask: String

    var id: String { name }
}

/// The preset-btn + popover Tauri puts beside every mask field: save the
/// current mask under a name, list saved ones (click to apply, × to delete).
struct MaskPresetButton: View {
    @Binding var mask: String

    @AppStorage("maskPresets") private var presetsRaw = "[]"
    @State private var showPopover = false
    @State private var nameDraft = ""

    private var presets: [MaskPreset] {
        (try? JSONDecoder().decode([MaskPreset].self, from: Data(presetsRaw.utf8))) ?? []
    }

    private func write(_ list: [MaskPreset]) {
        guard let data = try? JSONEncoder().encode(list.sorted { $0.name < $1.name }) else { return }
        presetsRaw = String(data: data, encoding: .utf8) ?? "[]"
    }

    var body: some View {
        Button {
            nameDraft = ""
            showPopover.toggle()
        } label: {
            Image(systemName: "bookmark")
        }
        .buttonStyle(.borderless)
        .focusEffectDisabled()
        .help("Save and reuse mask presets")
        .popover(isPresented: $showPopover, arrowEdge: .bottom) {
            VStack(alignment: .leading, spacing: 6) {
                if presets.isEmpty {
                    Text("No saved presets")
                        .font(AppFonts.sans(11))
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(presets) { preset in
                        HStack(spacing: 6) {
                            Button {
                                mask = preset.mask
                                showPopover = false
                            } label: {
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(preset.name)
                                        .font(AppFonts.sans(11, .medium))
                                    Text(preset.mask)
                                        .font(AppFonts.monoSized(10))
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }
                            }
                            .buttonStyle(.plain)
                            .focusEffectDisabled()
                            .help(preset.mask)
                            Spacer(minLength: 8)
                            Button {
                                write(presets.filter { $0.name != preset.name })
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
                    TextField("Save current as…", text: $nameDraft)
                        .textFieldStyle(.plain)
                        .onSubmit(commitSave)
                    Button("Save", action: commitSave)
                        .buttonStyle(.plain)
                        .focusEffectDisabled()
                        .foregroundStyle(.tint)
                }
            }
            .padding(10)
            .frame(minWidth: 240)
        }
    }

    private func commitSave() {
        let trimmed = nameDraft.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        write(presets.filter { $0.name != trimmed } + [MaskPreset(name: trimmed, mask: mask)])
        nameDraft = ""
    }
}
