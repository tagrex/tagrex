// The preview player (#297 ABI, #301 UI): a transport in the status bar that
// plays the selected track, shows a seek bar driven by the player's own clock,
// and advances gaplessly through the visible rows.

import SwiftUI

/// The player's state, as `player_status` reports it. Snake-case keys mapped by
/// hand, since the ABI does not convert them.
struct PlayerStatus: Decodable, Equatable {
    var path: String?
    var isPaused: Bool
    var positionSecs: Double
    var durationSecs: Double
    var wantsNext: Bool
    var seekRefused: Bool

    enum CodingKeys: String, CodingKey {
        case path
        case isPaused = "is_paused"
        case positionSecs = "position_secs"
        case durationSecs = "duration_secs"
        case wantsNext = "wants_next"
        case seekRefused = "seek_refused"
    }
}

@MainActor
struct PlayerBar: View {
    let library: Library
    /// The visible rows in order — what playback walks, and where Play starts.
    let queue: [String]
    /// The first selected row, in visible order: what Play starts with.
    let selectedFirst: String?

    /// While the user drags the seek bar, the thumb follows this instead of the
    /// clock, so it doesn't fight the 300 ms status poll.
    @State private var scrubbing: Double?
    @State private var volume = 1.0

    private var status: PlayerStatus? { library.playerStatus }
    private var loaded: Bool { status?.path != nil }

    /// What Play starts: the selected row, else the first visible one.
    private var startPath: String? { selectedFirst ?? queue.first }

    var body: some View {
        HStack(spacing: 10) {
            Button(action: playOrPause) {
                Image(systemName: library.isPlaying ? "pause.fill" : "play.fill")
            }
            .buttonStyle(.borderless)
            .disabled(startPath == nil && !loaded)
            .help(library.isPlaying ? "Pause" : "Play")

            Button { library.stopPlayback() } label: {
                Image(systemName: "stop.fill")
            }
            .buttonStyle(.borderless)
            .disabled(!loaded)
            .help("Stop")

            if loaded {
                transport
            } else {
                Text("Playback: pick a row and press play")
            }
        }
    }

    @ViewBuilder
    private var transport: some View {
        Text(library.nowPlaying?.title.isEmpty == false
             ? library.nowPlaying!.title
             : (library.nowPlaying?.file ?? "—"))
            .lineLimit(1)
            .frame(maxWidth: 200, alignment: .leading)

        let duration = max(status?.durationSecs ?? 0, 0.1)
        Slider(
            value: Binding(
                get: { scrubbing ?? status?.positionSecs ?? 0 },
                set: { scrubbing = $0 }
            ),
            in: 0...duration,
            onEditingChanged: { editing in
                if !editing, let target = scrubbing {
                    library.seek(to: target)
                    scrubbing = nil
                }
            }
        )
        .controlSize(.mini)
        .frame(minWidth: 120, maxWidth: 240)

        Text("\(clock(scrubbing ?? status?.positionSecs ?? 0)) / \(clock(status?.durationSecs ?? 0))")
            .monospacedDigit()

        Image(systemName: "speaker.fill")
        Slider(value: Binding(get: { volume }, set: { volume = $0; library.setVolume($0) }), in: 0...1)
            .controlSize(.mini)
            .frame(width: 70)
    }

    private func playOrPause() {
        if loaded {
            library.togglePause()
        } else if let path = startPath {
            library.play(path, queue: queue)
        }
    }

    private func clock(_ secs: Double) -> String {
        let total = Int(secs.rounded())
        return String(format: "%d:%02d", total / 60, total % 60)
    }
}
