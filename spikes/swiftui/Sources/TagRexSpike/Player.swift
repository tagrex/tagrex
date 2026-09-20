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
    /// What the volume was before a mute, so un-muting restores it rather than
    /// jumping to full.
    @State private var preMuteVolume = 1.0
    @State private var showVolumePopover = false
    /// The now-playing track's waveform (1000 buckets) and cover, refreshed when
    /// the loaded path changes.
    @State private var buckets: [UInt8] = []
    @State private var coverData: Data?

    private var status: PlayerStatus? { library.playerStatus }
    private var loaded: Bool { status?.path != nil }

    /// What Play starts: the selected row, else the first visible one.
    private var startPath: String? { selectedFirst ?? queue.first }

    /// The playing track's index in the queue, and the tracks either side of it,
    /// for the prev/next transport.
    private var currentIndex: Int? {
        guard let path = status?.path else { return nil }
        return queue.firstIndex(of: path)
    }
    private var prevPath: String? {
        guard let i = currentIndex, i > 0 else { return nil }
        return queue[i - 1]
    }
    private var nextPath: String? {
        guard let i = currentIndex, i + 1 < queue.count else { return nil }
        return queue[i + 1]
    }

    var body: some View {
        HStack(spacing: 10) {
            // The five transport glyphs grouped tight, as one cluster (`pl-transport`).
            HStack(spacing: 6) {
                Button { if let p = prevPath { library.play(p, queue: queue) } } label: {
                    Image(systemName: "backward.fill")
                }
                .disabled(prevPath == nil)
                .help("Previous track")

                Button(action: playOrPause) {
                    Image(systemName: library.isPlaying ? "pause.fill" : "play.fill")
                }
                .disabled(startPath == nil && !loaded)
                .help(library.isPlaying ? "Pause" : "Play")

                Button { library.stopPlayback() } label: {
                    Image(systemName: "stop.fill")
                }
                .disabled(!loaded)
                .help("Stop")

                Button { if let p = nextPath { library.play(p, queue: queue) } } label: {
                    Image(systemName: "forward.fill")
                }
                .disabled(nextPath == nil)
                .help("Next track")

                repeatButton
            }
            .buttonStyle(.borderless)

            volumeControl

            if loaded {
                cover
                mainColumn
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
        .help(repeatHelp)
    }

    private var repeatHelp: String {
        switch library.repeatMode {
        case .off: "Repeat off"
        case .all: "Repeat all"
        case .one: "Repeat one"
        }
    }

    /// A speaker icon that opens the volume slider in a popover (`pl-vol` /
    /// `pl-volume-pop`) — the slider doesn't sit permanently in the bar.
    private var volumeControl: some View {
        Button {
            showVolumePopover.toggle()
        } label: {
            Image(systemName: volumeIcon)
        }
        .buttonStyle(.borderless)
        .focusEffectDisabled()
        .help("Volume")
        .popover(isPresented: $showVolumePopover, arrowEdge: .bottom) {
            HStack(spacing: 8) {
                Button(action: toggleMute) {
                    Image(systemName: volumeIcon)
                }
                .buttonStyle(.borderless)
                .help(volume > 0 ? "Mute" : "Unmute")

                Slider(value: Binding(
                    get: { volume },
                    set: { volume = $0; library.setVolume($0) }
                ), in: 0...1)
                    .frame(width: 120)
            }
            .padding(10)
        }
    }

    private var volumeIcon: String {
        if volume <= 0 { return "speaker.slash.fill" }
        if volume < 0.5 { return "speaker.wave.1.fill" }
        return "speaker.wave.2.fill"
    }

    private func toggleMute() {
        if volume > 0 {
            preMuteVolume = volume
            volume = 0
        } else {
            volume = preMuteVolume > 0 ? preMuteVolume : 1
        }
        library.setVolume(volume)
    }

    /// The title/time line above the waveform, and the waveform itself below it
    /// spanning the full width (`pl-main`: `pl-line` + `pl-wave-wrap`) — stacked,
    /// not squeezed into a column beside the bar, so the title has the whole
    /// width and never ellipsises after a few words.
    private var mainColumn: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(nowPlayingLabel)
                    .font(AppFonts.mono)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text("\(clock(status?.positionSecs ?? 0)) / \(clock(status?.durationSecs ?? 0))")
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .fixedSize()
            }

            let duration = max(status?.durationSecs ?? 0, 0.1)
            let progress = min(max((status?.positionSecs ?? 0) / duration, 0), 1)
            WaveformSeekBar(buckets: buckets, progress: progress) { fraction in
                library.seek(to: fraction * duration)
            }
            .frame(height: 20)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// "Artist — Title", falling back to just the title or the file name —
    /// mirrors the Tauri `playerLabel`.
    private var nowPlayingLabel: String {
        guard let track = library.nowPlaying else {
            return status?.path.map { ($0 as NSString).lastPathComponent } ?? ""
        }
        let artist = track.artist.trimmingCharacters(in: .whitespaces)
        let title = track.title.trimmingCharacters(in: .whitespaces)
        if !artist.isEmpty, !title.isEmpty { return "\(artist) — \(title)" }
        return title.isEmpty ? track.file : title
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
                // Fixed-width rounded bars with an even gap, so the envelope reads
                // as a proper waveform instead of a sparse comb — each bar is the
                // peak of the bucket group it covers, mirrored about the centre.
                let span: CGFloat = 3
                let barWidth: CGFloat = 2
                let count = max(8, Int(size.width / span))
                let mid = size.height / 2
                for i in 0..<count {
                    let lo = i * buckets.count / count
                    let hi = max(lo + 1, (i + 1) * buckets.count / count)
                    var peak: CGFloat = 0
                    for b in lo..<min(hi, buckets.count) { peak = max(peak, CGFloat(buckets[b])) }
                    let amp = peak / 255
                    let barHeight = max(2, amp * (size.height - 2))
                    let x = CGFloat(i) * span
                    let rect = CGRect(x: x, y: mid - barHeight / 2, width: barWidth, height: barHeight)
                    let played = (Double(i) + 0.5) / Double(count) <= progress
                    ctx.fill(
                        Path(roundedRect: rect, cornerRadius: barWidth / 2),
                        with: .color(played ? Color.appAccent : .secondary.opacity(0.55)))
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
