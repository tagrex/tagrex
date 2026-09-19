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

    @State private var volume = 1.0
    /// The now-playing track's waveform (1000 buckets) and cover, refreshed when
    /// the loaded path changes.
    @State private var buckets: [UInt8] = []
    @State private var coverData: Data?

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

            repeatButton

            if loaded {
                transport
            } else {
                Text("Playback: pick a row and press play")
            }
        }
        .task(id: status?.path) { await loadTrackMedia() }
    }

    /// Off / all / one, cycled — the glyph and tint say which (`applyRepeatMode`).
    private var repeatButton: some View {
        Button { library.cycleRepeat() } label: {
            Image(systemName: library.repeatMode == .one ? "repeat.1" : "repeat")
                .foregroundStyle(library.repeatMode == .off
                                 ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.tint))
        }
        .buttonStyle(.borderless)
        .help(repeatHelp)
    }

    private var repeatHelp: String {
        switch library.repeatMode {
        case .off: "Repeat off"
        case .all: "Repeat all"
        case .one: "Repeat one"
        }
    }

    @ViewBuilder
    private var transport: some View {
        cover
        Text(library.nowPlaying?.title.isEmpty == false
             ? library.nowPlaying!.title
             : (library.nowPlaying?.file ?? "—"))
            .lineLimit(1)
            .frame(maxWidth: 160, alignment: .leading)

        let duration = max(status?.durationSecs ?? 0, 0.1)
        let progress = min(max((status?.positionSecs ?? 0) / duration, 0), 1)
        WaveformSeekBar(buckets: buckets, progress: progress) { fraction in
            library.seek(to: fraction * duration)
        }
        .frame(width: 200, height: 22)

        Text("\(clock(status?.positionSecs ?? 0)) / \(clock(status?.durationSecs ?? 0))")
            .monospacedDigit()

        Image(systemName: "speaker.fill")
        Slider(value: Binding(get: { volume }, set: { volume = $0; library.setVolume($0) }), in: 0...1)
            .controlSize(.mini)
            .frame(width: 70)
    }

    /// The now-playing cover, a small square before the title.
    @ViewBuilder
    private var cover: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 3).fill(.quaternary)
            if let coverData, let image = NSImage(data: coverData) {
                Image(nsImage: image).resizable().scaledToFill()
            } else {
                Image(systemName: "music.note").font(.system(size: 10)).foregroundStyle(.tertiary)
            }
        }
        .frame(width: 22, height: 22)
        .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    private func playOrPause() {
        if loaded {
            library.togglePause()
        } else if let path = startPath {
            library.play(path, queue: queue)
        }
    }

    /// Fetch the waveform and cover for the loaded track (or clear them).
    private func loadTrackMedia() async {
        guard let path = status?.path else { buckets = []; coverData = nil; return }
        buckets = await library.waveform(path: path) ?? []
        coverData = await library.coverSummary(paths: [path])?.sharedSet.first?.data
    }

    private func clock(_ secs: Double) -> String {
        let total = Int(secs.rounded())
        return String(format: "%d:%02d", total / 60, total % 60)
    }
}

/// A waveform that doubles as the seek control: 1000 amplitude buckets drawn as
/// vertical bars, the played portion in the accent and the rest dimmed, a click
/// or drag anywhere seeking to that fraction.
struct WaveformSeekBar: View {
    let buckets: [UInt8]
    let progress: Double
    let onSeek: (Double) -> Void

    var body: some View {
        GeometryReader { geo in
            Canvas { ctx, size in
                guard !buckets.isEmpty else { return }
                let mid = size.height / 2
                let bars = min(Int(size.width), buckets.count)
                guard bars > 0 else { return }
                for i in 0..<bars {
                    let amp = CGFloat(buckets[i * buckets.count / bars]) / 255
                    let x = CGFloat(i) / CGFloat(bars) * size.width
                    let barHeight = max(1, amp * (size.height - 2))
                    let played = Double(i) / Double(bars) <= progress
                    var bar = Path()
                    bar.move(to: CGPoint(x: x, y: mid - barHeight / 2))
                    bar.addLine(to: CGPoint(x: x, y: mid + barHeight / 2))
                    ctx.stroke(
                        bar,
                        with: .color(played ? Color.appAccent : Color.secondary.opacity(0.35)),
                        lineWidth: 1)
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onEnded { value in
                        onSeek(min(max(value.location.x / geo.size.width, 0), 1))
                    }
            )
        }
    }
}
