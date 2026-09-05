// The Settings sheet: the online credentials/throttle and the ID3 write
// revision, over `load_settings` / `save_settings` and the Discogs token
// commands. A subset of the Tauri settings (app/ui/js/settings.js) — the online
// and write essentials; fonts, theme, import fields and the rest come later.

import SwiftUI

@MainActor
struct SettingsView: View {
    let library: Library
    @Environment(\.dismiss) private var dismiss

    @State private var draft = OnlineSettings()
    @State private var loaded = false
    @State private var isSaving = false

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    SecureField("Discogs token", text: $draft.discogsToken)
                    TextField("Proxy", text: $draft.proxy, prompt: Text("Direct connection"))
                    TextField(
                        "Rate limit",
                        value: $draft.rateLimitPerMin,
                        format: .number
                    )
                    .help("Discogs requests per minute; 0 turns the throttle off.")
                } header: {
                    Text("Online")
                } footer: {
                    Text("A Discogs personal access token turns on release-cover "
                        + "thumbnails and lifts the search rate limit. MusicBrainz "
                        + "needs none.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }

                Section("Writing") {
                    Picker("ID3v2 revision", selection: $draft.id3v23) {
                        Text("v2.4").tag(false)
                        Text("v2.3").tag(true)
                    }
                    .pickerStyle(.segmented)
                }
            }
            .formStyle(.grouped)

            Divider()

            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button {
                    isSaving = true
                    Task {
                        await library.saveOnlineSettings(draft)
                        dismiss()
                    }
                } label: {
                    if isSaving {
                        ProgressView().controlSize(.small)
                    } else {
                        Text("Save")
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(isSaving || !loaded)
            }
            .padding(12)
        }
        .frame(width: 460, height: 340)
        .task {
            guard !loaded else { return }
            draft = await library.loadOnlineSettings()
            loaded = true
        }
    }
}
