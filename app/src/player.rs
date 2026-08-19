//! Native audio backend for the in-app preview player.
//!
//! Playback lives on a dedicated thread that owns a rodio [`Sink`] (rodio's
//! `OutputStream` is `!Send`, so it can't sit in Tauri's shared state). The UI
//! talks to it through [`Player`]: commands go over an mpsc channel, and a
//! shared [`PlayerStatus`] snapshot is polled back for the seek bar / time.
//!
//! Gapless is the whole point (#30): the sink is kept primed with the current
//! track *and* the next one, so rodio streams from one into the other with no
//! decode gap at the boundary. The frontend feeds the "next" track whenever the
//! current one changes (see the status `wants_next` flag), which also realizes
//! auto-advance (#29) natively — the sink simply plays through the queue.
//!
//! Every format we handle decodes through rodio/Symphonia, including OGG, which
//! the previous WebView `<audio>` player couldn't play.

use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::stream::{DeviceSinkBuilder, MixerDeviceSink};
// rodio 0.22 renamed its `Sink` to `Player`, which is also the name of the
// handle in this module — keep the old name for the queue on the device so the
// two never read as the same thing.
use rodio::{Decoder, Player as Sink};
use serde::Serialize;
use tagrex_core::model::TagEngine;

/// Snapshot of the player, polled by the UI.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PlayerStatus {
    /// Path of the track currently playing, or `None` when idle.
    pub path: Option<String>,
    pub is_paused: bool,
    pub position_secs: f64,
    pub duration_secs: f64,
    /// True when a track is playing but no next track is queued yet, so the UI
    /// should feed the next one to keep playback gapless.
    pub wants_next: bool,
    /// The last seek on this track was refused by the decoder (#190). The clock
    /// does NOT move in that case, so the bar has to go back to where the audio
    /// really is — and say so, rather than showing a position nothing is playing
    /// from. Cleared when a track starts or playback stops.
    pub seek_refused: bool,
}

enum Cmd {
    Play(PathBuf),
    SetNext(PathBuf),
    Pause,
    Resume,
    Stop,
    Seek(f64),
    SetVolume(f32),
}

/// Handle to the audio thread. `Send + Sync`, so it lives directly in Tauri's
/// managed state.
pub struct Player {
    tx: Sender<Cmd>,
    status: Arc<Mutex<PlayerStatus>>,
}

impl Player {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let status = Arc::new(Mutex::new(PlayerStatus::default()));
        let status_for_thread = Arc::clone(&status);
        std::thread::spawn(move || audio_thread(rx, status_for_thread));
        Self { tx, status }
    }

    pub fn play(&self, path: PathBuf) {
        let _ = self.tx.send(Cmd::Play(path));
    }
    pub fn set_next(&self, path: PathBuf) {
        let _ = self.tx.send(Cmd::SetNext(path));
    }
    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }
    pub fn resume(&self) {
        let _ = self.tx.send(Cmd::Resume);
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
    pub fn seek(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Seek(secs));
    }
    pub fn set_volume(&self, level: f32) {
        let _ = self.tx.send(Cmd::SetVolume(level));
    }
    pub fn status(&self) -> PlayerStatus {
        self.status.lock().unwrap().clone()
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

/// How many buckets a waveform is reduced to (#101). Wide enough that a
/// three-minute track shows its structure at any window width the player bar
/// ever has, small enough that the whole thing is a kilobyte of JSON.
const WAVEFORM_BUCKETS: usize = 1000;

/// Samples per coarse peak while decoding. The pass does not know how long the
/// file will turn out to be — a duration read from tags can be wrong, and some
/// formats do not state one at all — so it collects peaks at a fixed
/// granularity and reduces that to [`WAVEFORM_BUCKETS`] afterwards. An hour of
/// 44.1 kHz stereo is about 310k coarse peaks, a megabyte held for the length
/// of one call.
const WAVEFORM_WINDOW: usize = 1024;

/// The quietest peak a waveform is scaled against (#101). Normalising to the
/// loudest sample is what makes a quiet recording legible, but a track that is
/// nearly silent would be amplified into a wall of noise — so the divisor never
/// goes below this, which caps the amplification at 20x.
const WAVEFORM_FLOOR: f32 = 0.05;

/// The amplitude envelope of `path`, as [`WAVEFORM_BUCKETS`] values of 0..=255
/// (#101).
///
/// Decoded through the same rodio/Symphonia path playback uses, deliberately:
/// anything the player can play is then something the bar can draw, and there
/// is no second decoder configuration to drift out of step with the first.
///
/// **RMS, not peak.** The obvious envelope — the loudest sample in each
/// bucket — draws a solid block for anything mastered in the last thirty years:
/// measured on a real track it averaged 216 of 255 with almost every bucket at
/// the ceiling, which tells you nothing about where the intro ends. RMS follows
/// how loud a passage actually *is*, so an intro, a breakdown and a drop are
/// three different heights. The result is normalised to the file's own loudest
/// passage, so the picture fills the bar whatever the recording level.
///
/// Blocking and slow — seconds for an hour-long mix. The caller must keep it off
/// the main thread.
pub fn waveform(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let decoder = Decoder::try_from(file).map_err(|err| err.to_string())?;

    let mut coarse: Vec<f32> = Vec::new();
    let mut sum_squares = 0.0f64;
    let mut in_window = 0usize;
    for sample in decoder {
        sum_squares += (sample as f64) * (sample as f64);
        in_window += 1;
        if in_window == WAVEFORM_WINDOW {
            coarse.push((sum_squares / WAVEFORM_WINDOW as f64).sqrt() as f32);
            sum_squares = 0.0;
            in_window = 0;
        }
    }
    // The tail of the file is a window like any other, however short.
    if in_window > 0 {
        coarse.push((sum_squares / in_window as f64).sqrt() as f32);
    }
    if coarse.is_empty() {
        return Err("nothing decoded".to_string());
    }

    let loudest = coarse.iter().copied().fold(0.0f32, f32::max);
    // Loudest PASSAGE, not loudest sample — see above; the floor below is what
    // keeps a near-silent file from being amplified into a wall of noise.
    let scale = 1.0 / loudest.max(WAVEFORM_FLOOR);
    let mut buckets = Vec::with_capacity(WAVEFORM_BUCKETS);
    for index in 0..WAVEFORM_BUCKETS {
        // Ranges are computed from the index rather than stepped, so the
        // rounding error cannot accumulate and leave the last bucket empty.
        let from = index * coarse.len() / WAVEFORM_BUCKETS;
        let to = ((index + 1) * coarse.len() / WAVEFORM_BUCKETS).max(from + 1);
        let loudest_here = coarse[from..to.min(coarse.len())]
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        buckets.push(((loudest_here * scale).clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    Ok(buckets)
}

/// One queued track: its path and total duration (from lofty, since rodio's
/// `total_duration` is unreliable for MP3).
struct Track {
    path: PathBuf,
    duration: Duration,
}

fn audio_thread(rx: Receiver<Cmd>, status: Arc<Mutex<PlayerStatus>>) {
    // If no audio device is available (e.g. a headless box), give up quietly:
    // commands are dropped and the status stays idle. The rest of the app is
    // unaffected.
    let mut device = match DeviceSinkBuilder::open_default_sink() {
        Ok(device) => device,
        Err(err) => {
            eprintln!("audio: no output device, preview disabled: {err}");
            return;
        }
    };
    // The device sink lives as long as this thread; its parting message on drop
    // is noise in a terminal-launched build.
    device.log_on_drop(false);

    let mut sink = new_sink(&device);
    let mut queue: VecDeque<Track> = VecDeque::new();
    let mut clock = PlayClock::default();
    let mut paused = false;
    // Play and Stop each replace the sink, and a fresh sink is always full
    // volume — so the level has to live out here and be re-applied to every new
    // sink, or turning the volume down would silently reset on the next track.
    let mut volume: f32 = 1.0;
    // Whether the decoder refused the last seek on the current track (#190).
    let mut seek_refused = false;
    sink.set_volume(volume);

    loop {
        // Wait for a command, but wake every 200 ms to advance the queue and
        // refresh the status snapshot while playing.
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(cmd) => match cmd {
                Cmd::Play(path) => {
                    // Silence the old sink, then play on a fresh one — a new
                    // Sink guarantees a clean, re-appendable queue regardless of
                    // rodio's drop-vs-stop semantics.
                    sink.stop();
                    sink = new_sink(&device);
                    sink.set_volume(volume);
                    queue.clear();
                    paused = false;
                    seek_refused = false;
                    if enqueue(&sink, &mut queue, path) {
                        clock.start();
                    } else {
                        clock.stop();
                    }
                }
                Cmd::SetNext(path) => {
                    // Only prime a next track when exactly the current one is
                    // queued; ignore otherwise (already primed, or idle).
                    if queue.len() == 1 {
                        enqueue(&sink, &mut queue, path);
                    }
                }
                Cmd::Pause => {
                    if !queue.is_empty() {
                        sink.pause();
                        clock.pause();
                        paused = true;
                    }
                }
                Cmd::Resume => {
                    if !queue.is_empty() {
                        sink.play();
                        clock.resume();
                        paused = false;
                    }
                }
                Cmd::Stop => {
                    sink.stop();
                    sink = new_sink(&device);
                    sink.set_volume(volume);
                    queue.clear();
                    clock.stop();
                    paused = false;
                    seek_refused = false;
                }
                Cmd::SetVolume(level) => {
                    volume = level.clamp(0.0, 1.0);
                    sink.set_volume(volume);
                }
                Cmd::Seek(secs) => {
                    if !queue.is_empty() {
                        // The clock is what the UI shows, so it may only move
                        // when the audio did (#190). A decoder that refuses is
                        // reported instead of being papered over.
                        let target = Duration::from_secs_f64(secs.max(0.0));
                        match sink.try_seek(target) {
                            Ok(()) => {
                                clock.seek(target);
                                seek_refused = false;
                            }
                            Err(err) => {
                                eprintln!("audio: seek refused: {err}");
                                seek_refused = true;
                            }
                        }
                    }
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        // The sink drains sources as they finish. When it holds fewer than our
        // queue, a track boundary was crossed: drop the finished front and
        // restart the clock for the new current track.
        while queue.len() > sink.len() && !queue.is_empty() {
            queue.pop_front();
            clock.start();
            seek_refused = false; // a new track, and a new answer on seeking
        }
        if queue.is_empty() {
            clock.stop();
            paused = false;
            seek_refused = false;
        }

        write_status(&status, &queue, &clock, paused, seek_refused);
    }
}

/// A fresh queue on the output device. Play and Stop each start one, because a
/// new sink guarantees a clean, re-appendable queue regardless of rodio's
/// drop-vs-stop semantics.
fn new_sink(device: &MixerDeviceSink) -> Sink {
    Sink::connect_new(device.mixer())
}

/// Decode `path` and append it to the sink, recording it in `queue`. Returns
/// false (and leaves the queue untouched) if the file can't be opened/decoded.
fn enqueue(sink: &Sink, queue: &mut VecDeque<Track>, path: PathBuf) -> bool {
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("audio: can't open {}: {err}", path.display());
            return false;
        }
    };
    // From the `File`, not a plain reader: that is what tells the decoder how
    // long the file is, and without the length Symphonia calls a FLAC
    // unseekable (#190).
    let decoder = match Decoder::try_from(file) {
        Ok(decoder) => decoder,
        Err(err) => {
            eprintln!("audio: can't decode {}: {err}", path.display());
            return false;
        }
    };
    let duration = TagEngine::read_duration(&path).unwrap_or(Duration::ZERO);
    sink.append(decoder);
    queue.push_back(Track { path, duration });
    true
}

/// How close to the end of the current track the queue gets primed with the
/// next one. Priming at track START (the old behaviour) meant the next source
/// was already appended to the sink the moment playback began — and rodio has
/// no way to remove an appended source, so a Repeat change made mid-track was
/// silently ignored and only took effect one track later. Priming late leaves
/// that decision open for almost the whole track; a few seconds is still far
/// more than enough to open and append the next decoder gaplessly.
const PRIME_LEAD_SECS: f64 = 5.0;

fn write_status(
    status: &Arc<Mutex<PlayerStatus>>,
    queue: &VecDeque<Track>,
    clock: &PlayClock,
    paused: bool,
    seek_refused: bool,
) {
    let mut guard = status.lock().unwrap();
    match queue.front() {
        Some(current) => {
            let duration = current.duration.as_secs_f64();
            // Never report past the track length (the wall clock can overshoot
            // by a poll interval before the boundary is detected).
            let position = clock.position().as_secs_f64().min(if duration > 0.0 {
                duration
            } else {
                f64::MAX
            });
            // With an unknown duration (0) there's nothing to count down from,
            // so fall back to priming immediately rather than never.
            let near_end = duration <= 0.0 || duration - position <= PRIME_LEAD_SECS;
            *guard = PlayerStatus {
                path: Some(current.path.to_string_lossy().into_owned()),
                is_paused: paused,
                position_secs: position,
                duration_secs: duration,
                wants_next: queue.len() == 1 && near_end,
                seek_refused,
            };
        }
        None => *guard = PlayerStatus::default(),
    }
}

/// Wall-clock position tracker for the current track. rodio plays in real time,
/// so elapsed wall time (minus paused spans, plus any seek offset) equals the
/// playback position. Kept free of `rodio` so its arithmetic is unit-testable.
#[derive(Debug)]
struct PlayClock {
    running: bool,
    /// Position at the last (re)start — 0 on a fresh track, the target on seek.
    base: Duration,
    /// When the current running span began.
    started: Instant,
    /// Total paused time accumulated since `base`/`started`.
    paused_total: Duration,
    /// Set while paused, to when the pause began.
    paused_at: Option<Instant>,
}

impl Default for PlayClock {
    fn default() -> Self {
        Self {
            running: false,
            base: Duration::ZERO,
            started: Instant::now(),
            paused_total: Duration::ZERO,
            paused_at: None,
        }
    }
}

impl PlayClock {
    fn start(&mut self) {
        self.start_at(Instant::now());
    }
    fn pause(&mut self) {
        self.pause_at(Instant::now());
    }
    fn resume(&mut self) {
        self.resume_at(Instant::now());
    }
    fn seek(&mut self, to: Duration) {
        self.seek_at(to, Instant::now());
    }
    fn stop(&mut self) {
        self.running = false;
    }
    fn position(&self) -> Duration {
        self.position_at(Instant::now())
    }

    // `*_at` variants take an explicit `now` so the arithmetic can be tested
    // deterministically.
    fn start_at(&mut self, now: Instant) {
        self.running = true;
        self.base = Duration::ZERO;
        self.started = now;
        self.paused_total = Duration::ZERO;
        self.paused_at = None;
    }
    fn pause_at(&mut self, now: Instant) {
        if self.running && self.paused_at.is_none() {
            self.paused_at = Some(now);
        }
    }
    fn resume_at(&mut self, now: Instant) {
        if let Some(at) = self.paused_at.take() {
            self.paused_total += now.saturating_duration_since(at);
        }
    }
    fn seek_at(&mut self, to: Duration, now: Instant) {
        self.base = to;
        self.started = now;
        self.paused_total = Duration::ZERO;
        // Preserve paused state: if paused, restart the pause span at `now`.
        self.paused_at = self.paused_at.map(|_| now);
    }
    fn position_at(&self, now: Instant) -> Duration {
        if !self.running {
            return Duration::ZERO;
        }
        let mut elapsed = now.saturating_duration_since(self.started);
        elapsed = elapsed.saturating_sub(self.paused_total);
        if let Some(at) = self.paused_at {
            elapsed = elapsed.saturating_sub(now.saturating_duration_since(at));
        }
        self.base + elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_tracks_elapsed_pause_and_seek() {
        let t0 = Instant::now();
        let mut c = PlayClock::default();
        assert_eq!(c.position_at(t0), Duration::ZERO); // idle

        c.start_at(t0);
        assert_eq!(
            c.position_at(t0 + Duration::from_secs(3)),
            Duration::from_secs(3)
        );

        // Pause at 3s, hold for 2s: position frozen at 3s.
        c.pause_at(t0 + Duration::from_secs(3));
        assert_eq!(
            c.position_at(t0 + Duration::from_secs(5)),
            Duration::from_secs(3)
        );

        // Resume at 5s: at 6s wall time, 1s more played -> 4s.
        c.resume_at(t0 + Duration::from_secs(5));
        assert_eq!(
            c.position_at(t0 + Duration::from_secs(6)),
            Duration::from_secs(4)
        );

        // Seek to 30s at 6s: at 7s wall time -> 31s.
        c.seek_at(Duration::from_secs(30), t0 + Duration::from_secs(6));
        assert_eq!(
            c.position_at(t0 + Duration::from_secs(7)),
            Duration::from_secs(31)
        );

        // A fresh track resets to 0.
        c.start_at(t0 + Duration::from_secs(7));
        assert_eq!(c.position_at(t0 + Duration::from_secs(7)), Duration::ZERO);

        c.stop();
        assert_eq!(c.position_at(t0 + Duration::from_secs(8)), Duration::ZERO);
    }

    #[test]
    fn seek_while_paused_keeps_position_frozen() {
        let t0 = Instant::now();
        let mut c = PlayClock::default();
        c.start_at(t0);
        c.pause_at(t0 + Duration::from_secs(2));
        // Seek to 50s while paused; time keeps passing but position holds at 50.
        c.seek_at(Duration::from_secs(50), t0 + Duration::from_secs(4));
        assert_eq!(
            c.position_at(t0 + Duration::from_secs(10)),
            Duration::from_secs(50)
        );
        // Resume at 10s: at 11s -> 51s.
        c.resume_at(t0 + Duration::from_secs(10));
        assert_eq!(
            c.position_at(t0 + Duration::from_secs(11)),
            Duration::from_secs(51)
        );
    }
}
