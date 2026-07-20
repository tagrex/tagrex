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
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, Sink};
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
}

enum Cmd {
    Play(PathBuf),
    SetNext(PathBuf),
    Pause,
    Resume,
    Stop,
    Seek(f64),
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
    pub fn status(&self) -> PlayerStatus {
        self.status.lock().unwrap().clone()
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
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
    let (_stream, handle) = match OutputStream::try_default() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("audio: no output device, preview disabled: {err}");
            return;
        }
    };

    let mut sink = Sink::try_new(&handle).expect("sink on a valid output stream");
    let mut queue: VecDeque<Track> = VecDeque::new();
    let mut clock = PlayClock::default();
    let mut paused = false;

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
                    sink = Sink::try_new(&handle).expect("sink on a valid output stream");
                    queue.clear();
                    paused = false;
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
                    sink = Sink::try_new(&handle).expect("sink on a valid output stream");
                    queue.clear();
                    clock.stop();
                    paused = false;
                }
                Cmd::Seek(secs) => {
                    if !queue.is_empty() {
                        let _ = sink.try_seek(Duration::from_secs_f64(secs.max(0.0)));
                        clock.seek(Duration::from_secs_f64(secs.max(0.0)));
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
        }
        if queue.is_empty() {
            clock.stop();
            paused = false;
        }

        write_status(&status, &queue, &clock, paused);
    }
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
    let decoder = match Decoder::new(BufReader::new(file)) {
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

fn write_status(
    status: &Arc<Mutex<PlayerStatus>>,
    queue: &VecDeque<Track>,
    clock: &PlayClock,
    paused: bool,
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
            *guard = PlayerStatus {
                path: Some(current.path.to_string_lossy().into_owned()),
                is_paused: paused,
                position_secs: position,
                duration_secs: duration,
                wants_next: queue.len() == 1,
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
