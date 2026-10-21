//! The system media session (#218): the Mac's media keys, the Now Playing
//! widget and headphone controls drive the preview player. On Linux the same
//! part is played by MPRIS on the session D-Bus (#516), which the desktop's
//! media widget, media keys and `playerctl` all talk to.
//!
//! The app registers with the OS as a media player rather than grabbing the
//! keys: macOS routes Play/Next/Previous to whichever app is "now playing",
//! so no Accessibility permission is needed and another player keeps its keys
//! while it is the one playing. That routing only works while the app says
//! what it is playing, which is why the frontend reports every change of
//! track and state through [`update`].
//!
//! On Windows it is the System Media Transport Controls (#515): media keys,
//! headphone buttons and the media flyout by the volume overlay.
//!
//! A remote command reaches the frontend as the [`EVENT`] event and presses
//! the same transport buttons a click would. Any other platform compiles the
//! no-op versions below.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use tagrex_player::Player;

// Only a platform with a session of its own reads these; any other compiles
// the no-op versions below.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", windows)),
    allow(dead_code)
)]
/// The event a remote command arrives as.
pub const EVENT: &str = "tagrex://media-control";

/// What the preview player is doing, as the frontend reports it.
#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

/// The track the system should show, and where playback is in it.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", windows)),
    allow(dead_code)
)]
#[derive(Deserialize)]
pub struct NowPlaying {
    pub state: PlaybackState,
    pub path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: f64,
    pub position_secs: f64,
}

/// A remote command, as the frontend receives it.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", windows)),
    allow(dead_code)
)]
#[derive(Clone, Serialize)]
pub struct MediaControl {
    /// `toggle`, `play`, `pause`, `stop`, `next`, `previous` or `seek`.
    pub action: &'static str,
    /// Where to seek to, for `seek` only.
    pub position_secs: Option<f64>,
}

/// The track whose cover was last handed to the system, so a state change on
/// the same track does not read the file again.
static COVER_FOR: Mutex<Option<String>> = Mutex::new(None);

/// Publish `now` to the system. The cover is read here, from the file, and
/// only for the track the player is actually playing — the frontend names a
/// path, but it cannot make this read any other file.
pub async fn update(app: AppHandle, now: NowPlaying) {
    let playing = app.state::<Player>().status().path;
    let track = now
        .path
        .clone()
        .filter(|path| Some(path) == playing.as_ref());
    let new_track = {
        let mut cover_for = COVER_FOR.lock().unwrap();
        let changed = *cover_for != track;
        cover_for.clone_from(&track);
        changed
    };
    // `None`: same track, keep the cover already published. `Some(None)`: a
    // new track without one.
    let cover = match (new_track, track) {
        (false, _) => None,
        (true, None) => Some(None),
        (true, Some(path)) => Some(
            tauri::async_runtime::spawn_blocking(move || {
                tagrex_core::model::TagEngine::read_cover(&PathBuf::from(path))
                    .ok()
                    .flatten()
                    .map(|art| art.data)
            })
            .await
            .ok()
            .flatten(),
        ),
    };
    publish(&app, now, cover);
}

#[cfg(target_os = "macos")]
pub use mac::{install, publish};

#[cfg(target_os = "linux")]
pub use mpris::{install, publish};

#[cfg(windows)]
pub use smtc::{install, publish};

/// No system media session on Windows yet.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn install(_app: &AppHandle) {}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn publish(_app: &AppHandle, _now: NowPlaying, _cover: Option<Option<Vec<u8>>>) {}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AllocAnyThread;
    use objc2_app_kit::NSImage;
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{NSData, NSDictionary, NSNumber, NSString};
    use objc2_media_player::{
        MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle,
        MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration,
        MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
        MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
        MPNowPlayingPlaybackState, MPRemoteCommand, MPRemoteCommandCenter, MPRemoteCommandEvent,
        MPRemoteCommandHandlerStatus,
    };
    use tauri::{AppHandle, Emitter};

    use super::{MediaControl, NowPlaying, PlaybackState, EVENT};

    thread_local! {
        // The current track's artwork, built once per track. Main thread only:
        // every MediaPlayer call here runs there.
        static ARTWORK: RefCell<Option<Retained<MPMediaItemArtwork>>> = const { RefCell::new(None) };
    }

    /// Register the remote command handlers. Called once, from `setup`, on the
    /// main thread — the thread the handlers are later called on.
    pub fn install(app: &AppHandle) {
        // SAFETY: MediaPlayer calls on the main thread with live objects; each
        // handler block is copied by the command and kept for the app's life.
        unsafe {
            let center = MPRemoteCommandCenter::sharedCommandCenter();
            let commands: [(Retained<MPRemoteCommand>, &'static str); 6] = [
                (center.togglePlayPauseCommand(), "toggle"),
                (center.playCommand(), "play"),
                (center.pauseCommand(), "pause"),
                (center.stopCommand(), "stop"),
                (center.nextTrackCommand(), "next"),
                (center.previousTrackCommand(), "previous"),
            ];
            for (command, action) in commands {
                let app = app.clone();
                let handler = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| {
                    send(&app, action, None)
                });
                command.setEnabled(true);
                command.addTargetWithHandler(&handler);
            }

            // Dragging the position in the Now Playing widget.
            let seek = center.changePlaybackPositionCommand();
            let app = app.clone();
            let handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
                let event: &AnyObject = event.as_ref();
                match event.downcast_ref::<MPChangePlaybackPositionCommandEvent>() {
                    Some(event) => send(&app, "seek", Some(event.positionTime())),
                    None => MPRemoteCommandHandlerStatus::CommandFailed,
                }
            });
            seek.setEnabled(true);
            seek.addTargetWithHandler(&handler);
        }
    }

    fn send(
        app: &AppHandle,
        action: &'static str,
        position_secs: Option<f64>,
    ) -> MPRemoteCommandHandlerStatus {
        let control = MediaControl {
            action,
            position_secs,
        };
        match app.emit(EVENT, control) {
            Ok(()) => MPRemoteCommandHandlerStatus::Success,
            Err(_) => MPRemoteCommandHandlerStatus::CommandFailed,
        }
    }

    /// Hand `now` to the system's Now Playing centre. `cover` is `None` to keep
    /// the current artwork, `Some` for a new track's (or its lack of one).
    pub fn publish(app: &AppHandle, now: NowPlaying, cover: Option<Option<Vec<u8>>>) {
        let _ = app.run_on_main_thread(move || {
            // SAFETY: on the main thread; every object passed is alive for the
            // call and the dictionary copies what it keeps.
            unsafe { apply(now, cover) }
        });
    }

    unsafe fn apply(now: NowPlaying, cover: Option<Option<Vec<u8>>>) {
        let center = MPNowPlayingInfoCenter::defaultCenter();
        if now.state == PlaybackState::Stopped || now.path.is_none() {
            ARTWORK.with(|artwork| artwork.borrow_mut().take());
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
            return;
        }
        if let Some(cover) = cover {
            let artwork = cover.and_then(|bytes| artwork(&bytes));
            ARTWORK.with(|slot| *slot.borrow_mut() = artwork);
        }

        let playing = now.state == PlaybackState::Playing;
        let title = NSString::from_str(&now.title);
        let artist = NSString::from_str(&now.artist);
        let album = NSString::from_str(&now.album);
        let duration = NSNumber::new_f64(now.duration_secs.max(0.0));
        let elapsed = NSNumber::new_f64(now.position_secs.max(0.0));
        // The system moves the elapsed time on by itself at this rate, so the
        // widget's clock runs between updates and stops while paused.
        let rate = NSNumber::new_f64(if playing { 1.0 } else { 0.0 });

        let mut keys: Vec<&NSString> = vec![
            MPMediaItemPropertyTitle,
            MPMediaItemPropertyPlaybackDuration,
            MPNowPlayingInfoPropertyElapsedPlaybackTime,
            MPNowPlayingInfoPropertyPlaybackRate,
        ];
        let mut values: Vec<&AnyObject> = vec![&title, &duration, &elapsed, &rate];
        if !now.artist.is_empty() {
            keys.push(MPMediaItemPropertyArtist);
            values.push(&artist);
        }
        if !now.album.is_empty() {
            keys.push(MPMediaItemPropertyAlbumTitle);
            values.push(&album);
        }
        let artwork = ARTWORK.with(|slot| slot.borrow().clone());
        if let Some(artwork) = &artwork {
            keys.push(MPMediaItemPropertyArtwork);
            values.push(artwork);
        }
        let info = NSDictionary::from_slices(&keys, &values);
        center.setNowPlayingInfo(Some(&info));
        center.setPlaybackState(if playing {
            MPNowPlayingPlaybackState::Playing
        } else {
            MPNowPlayingPlaybackState::Paused
        });
    }

    /// The system asks for the artwork at whatever size it is drawing; the one
    /// image is handed back each time and scaled by the system.
    unsafe fn artwork(bytes: &[u8]) -> Option<Retained<MPMediaItemArtwork>> {
        let data = NSData::with_bytes(bytes);
        let image = NSImage::initWithData(NSImage::alloc(), &data)?;
        let size: CGSize = image.size();
        let handler = RcBlock::new(move |_size: CGSize| NonNull::from(&*image));
        Some(MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            size,
            &handler,
        ))
    }
}

#[cfg(target_os = "linux")]
mod mpris {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Instant;

    use tauri::{AppHandle, Emitter, Manager};
    use zbus::names::BusName;
    use zbus::zvariant::{ObjectPath, Value};

    use super::{MediaControl, NowPlaying, PlaybackState, EVENT};

    /// The name players own on the session bus: `org.mpris.MediaPlayer2.` and
    /// the player's own part. One process per user, so no instance suffix.
    const BUS_NAME: &str = "org.mpris.MediaPlayer2.tagrex";
    const PATH: &str = "/org/mpris/MediaPlayer2";
    const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
    /// The track id the specification reserves for "nothing loaded".
    const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
    /// The `.desktop` file the Linux bundles install, without its extension.
    const DESKTOP_ENTRY: &str = "TagRex";

    /// Where a remote command goes: the frontend's event, or a test's channel.
    type Sink = Arc<dyn Fn(MediaControl) + Send + Sync>;

    static SERVICE: OnceLock<Service> = OnceLock::new();

    /// Export the player on the session bus. The connection is made off the
    /// main thread so a slow bus never holds up the window; with no bus at all
    /// the app runs on without a media session.
    pub fn install(app: &AppHandle) {
        let emitter = app.clone();
        let sink: Sink = Arc::new(move |control| {
            let _ = emitter.emit(EVENT, control);
        });
        let covers = app.path().app_cache_dir().ok().map(|dir| dir.join("mpris"));
        let _ =
            std::thread::Builder::new()
                .name("mpris".into())
                .spawn(move || match Service::start(BUS_NAME, sink, covers) {
                    Ok(service) => {
                        let _ = SERVICE.set(service);
                    }
                    Err(err) => eprintln!("media session: no D-Bus session, media keys off: {err}"),
                });
    }

    /// Hand `now` to MPRIS. `cover` is `None` to keep the current artwork,
    /// `Some` for a new track's (or its lack of one).
    pub fn publish(_app: &AppHandle, now: NowPlaying, cover: Option<Option<Vec<u8>>>) {
        if let Some(service) = SERVICE.get() {
            service.publish(now, cover);
        }
    }

    /// The exported player: the connection that owns the bus name, and the
    /// state its properties are read from.
    struct Service {
        connection: zbus::blocking::Connection,
        session: Arc<Mutex<Session>>,
        covers: Option<PathBuf>,
    }

    impl Service {
        /// Connect to the session bus, export both interfaces at [`PATH`] and
        /// take `name`. `covers` is the folder the artwork files go in.
        fn start(name: &str, sink: Sink, covers: Option<PathBuf>) -> zbus::Result<Self> {
            // Only this module writes there; a previous run's file is stale.
            if let Some(dir) = &covers {
                let _ = std::fs::remove_dir_all(dir);
            }
            let session = Arc::new(Mutex::new(Session::default()));
            let player = PlayerInterface {
                session: session.clone(),
                sink,
            };
            let connection = zbus::blocking::connection::Builder::session()?
                .serve_at(PATH, RootInterface)?
                .serve_at(PATH, player)?
                .name(name.to_owned())?
                .build()?;
            Ok(Self {
                connection,
                session,
                covers,
            })
        }

        /// Take the frontend's report and tell listeners what changed.
        fn publish(&self, now: NowPlaying, cover: Option<Option<Vec<u8>>>) {
            let (changed, seeked) = {
                let mut session = self.session.lock().unwrap();
                let change = session.apply(now, cover, self.covers.as_deref());
                let mut changed: HashMap<&str, Value> = HashMap::new();
                if change.status {
                    changed.insert("PlaybackStatus", Value::from(session.status()));
                }
                if change.metadata {
                    changed.insert("Metadata", Value::from(session.metadata()));
                }
                (changed, change.seeked)
            };
            if !changed.is_empty() {
                let _ = self.connection.emit_signal(
                    None::<BusName>,
                    PATH,
                    "org.freedesktop.DBus.Properties",
                    "PropertiesChanged",
                    &(PLAYER_IFACE, changed, Vec::<&str>::new()),
                );
            }
            // Position is never announced as a property change: listeners run
            // the clock themselves and only hear about a jump.
            if let Some(position) = seeked {
                let _ = self.connection.emit_signal(
                    None::<BusName>,
                    PATH,
                    PLAYER_IFACE,
                    "Seeked",
                    &position,
                );
            }
        }
    }

    /// The track listeners see, as the frontend last reported it.
    struct Track {
        id: u64,
        path: String,
        title: String,
        artist: String,
        album: String,
        duration_secs: f64,
    }

    /// What one report changed, for the signals that follow it.
    struct Change {
        status: bool,
        metadata: bool,
        /// The new position in microseconds, when it jumped rather than ran on.
        seeked: Option<i64>,
    }

    #[derive(Default)]
    struct Session {
        track: Option<Track>,
        playing: bool,
        /// The position at `at`; it runs on from there while playing.
        position_secs: f64,
        at: Option<Instant>,
        /// The last track id handed out.
        last_id: u64,
        /// The current artwork file and its `file://` URL.
        art: Option<(PathBuf, String)>,
        /// Numbers the artwork files, so a listener that caches by URL never
        /// shows the previous track's cover.
        art_no: u64,
    }

    impl Session {
        fn apply(
            &mut self,
            now: NowPlaying,
            cover: Option<Option<Vec<u8>>>,
            covers: Option<&Path>,
        ) -> Change {
            let status = self.status();
            let metadata = self.metadata();
            let expected = self.position_secs();
            let id = self.track.as_ref().map(|track| track.id);

            match now.path {
                Some(path) if now.state != PlaybackState::Stopped => {
                    let id = match &self.track {
                        Some(track) if track.path == path => track.id,
                        _ => {
                            self.last_id += 1;
                            self.last_id
                        }
                    };
                    self.track = Some(Track {
                        id,
                        path,
                        title: now.title,
                        artist: now.artist,
                        album: now.album,
                        duration_secs: now.duration_secs.max(0.0),
                    });
                    self.playing = now.state == PlaybackState::Playing;
                    self.position_secs = now.position_secs.max(0.0);
                    self.at = Some(Instant::now());
                    if let Some(cover) = cover {
                        self.set_art(cover, covers);
                    }
                }
                _ => {
                    self.track = None;
                    self.playing = false;
                    self.position_secs = 0.0;
                    self.at = None;
                    self.set_art(None, covers);
                }
            }

            let same_track = id.is_some() && id == self.track.as_ref().map(|track| track.id);
            Change {
                status: self.status() != status,
                metadata: self.metadata() != metadata,
                seeked: (same_track && (self.position_secs - expected).abs() > 1.0)
                    .then(|| micros(self.position_secs)),
            }
        }

        /// Replace the artwork file. Listeners load the cover from a URL, so
        /// the bytes go to a file of their own in the app's cache folder.
        fn set_art(&mut self, bytes: Option<Vec<u8>>, covers: Option<&Path>) {
            if let Some((file, _)) = self.art.take() {
                let _ = std::fs::remove_file(file);
            }
            let (Some(bytes), Some(dir)) = (bytes, covers) else {
                return;
            };
            self.art_no += 1;
            let file = dir.join(format!("cover-{}{}", self.art_no, extension(&bytes)));
            let written = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&file, bytes));
            if written.is_ok() {
                if let Ok(url) = tauri::Url::from_file_path(&file) {
                    self.art = Some((file, url.into()));
                }
            }
        }

        fn status(&self) -> &'static str {
            match (&self.track, self.playing) {
                (None, _) => "Stopped",
                (Some(_), true) => "Playing",
                (Some(_), false) => "Paused",
            }
        }

        fn position_secs(&self) -> f64 {
            let Some(track) = &self.track else {
                return 0.0;
            };
            let ran = match (self.playing, self.at) {
                (true, Some(at)) => at.elapsed().as_secs_f64(),
                _ => 0.0,
            };
            let position = self.position_secs + ran;
            if track.duration_secs > 0.0 {
                position.min(track.duration_secs)
            } else {
                position
            }
        }

        fn track_id(&self) -> String {
            match &self.track {
                Some(track) => format!("/com/tagrex/track/{}", track.id),
                None => NO_TRACK.to_owned(),
            }
        }

        fn metadata(&self) -> HashMap<String, Value<'static>> {
            let mut map = HashMap::new();
            let id = ObjectPath::try_from(self.track_id()).expect("a valid object path");
            map.insert("mpris:trackid".to_owned(), Value::from(id));
            let Some(track) = &self.track else {
                return map;
            };
            if track.duration_secs > 0.0 {
                let length = micros(track.duration_secs);
                map.insert("mpris:length".to_owned(), Value::from(length));
            }
            map.insert("xesam:title".to_owned(), Value::from(track.title.clone()));
            if !track.artist.is_empty() {
                let artist = vec![track.artist.clone()];
                map.insert("xesam:artist".to_owned(), Value::from(artist));
            }
            if !track.album.is_empty() {
                map.insert("xesam:album".to_owned(), Value::from(track.album.clone()));
            }
            if let Some((_, url)) = &self.art {
                map.insert("mpris:artUrl".to_owned(), Value::from(url.clone()));
            }
            map
        }
    }

    fn micros(secs: f64) -> i64 {
        (secs * 1_000_000.0).round() as i64
    }

    /// A file extension for the common cover formats, for a listener that goes
    /// by the name rather than the content.
    fn extension(bytes: &[u8]) -> &'static str {
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            ".jpg"
        } else if bytes.starts_with(b"\x89PNG") {
            ".png"
        } else {
            ""
        }
    }

    /// `org.mpris.MediaPlayer2`: who the player is. It cannot be raised or
    /// quit from outside.
    struct RootInterface;

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl RootInterface {
        fn raise(&self) {}

        fn quit(&self) {}

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_quit(&self) -> bool {
            false
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_raise(&self) -> bool {
            false
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn has_track_list(&self) -> bool {
            false
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn identity(&self) -> &str {
            "TagRex"
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn desktop_entry(&self) -> &str {
            DESKTOP_ENTRY
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn supported_uri_schemes(&self) -> Vec<String> {
            Vec::new()
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn supported_mime_types(&self) -> Vec<String> {
            Vec::new()
        }
    }

    /// `org.mpris.MediaPlayer2.Player`: the transport. Every method becomes
    /// the [`EVENT`] the macOS handlers send; the frontend decides what it
    /// means, as it does for a click.
    struct PlayerInterface {
        session: Arc<Mutex<Session>>,
        sink: Sink,
    }

    impl PlayerInterface {
        fn send(&self, action: &'static str, position_secs: Option<f64>) {
            (self.sink)(MediaControl {
                action,
                position_secs,
            });
        }
    }

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl PlayerInterface {
        fn play_pause(&self) {
            self.send("toggle", None);
        }

        fn play(&self) {
            self.send("play", None);
        }

        fn pause(&self) {
            self.send("pause", None);
        }

        fn stop(&self) {
            self.send("stop", None);
        }

        fn next(&self) {
            self.send("next", None);
        }

        fn previous(&self) {
            self.send("previous", None);
        }

        /// Move by `offset` microseconds. Past the end is the next track, as
        /// the specification asks; before the start is the start.
        fn seek(&self, offset: i64) {
            let (position, length) = {
                let session = self.session.lock().unwrap();
                match &session.track {
                    Some(track) => (session.position_secs(), track.duration_secs),
                    None => return,
                }
            };
            let target = position + offset as f64 / 1_000_000.0;
            if length > 0.0 && target > length {
                self.send("next", None);
            } else {
                self.send("seek", Some(target.max(0.0)));
            }
        }

        /// Go to `position` microseconds into `track_id`. Ignored for another
        /// track or a position outside this one, as the specification asks.
        fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
            let length = {
                let session = self.session.lock().unwrap();
                match &session.track {
                    Some(track) if track_id.as_str() == session.track_id() => track.duration_secs,
                    _ => return,
                }
            };
            if position < 0 || (length > 0.0 && position > micros(length)) {
                return;
            }
            self.send("seek", Some(position as f64 / 1_000_000.0));
        }

        fn open_uri(&self, uri: &str) -> zbus::fdo::Result<()> {
            let _ = uri;
            Err(zbus::fdo::Error::NotSupported(
                "TagRex plays the files in its own list".into(),
            ))
        }

        /// Declared for the introspection data; [`Service::publish`] sends it.
        #[zbus(signal)]
        async fn seeked(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            position: i64,
        ) -> zbus::Result<()>;

        #[zbus(property)]
        fn playback_status(&self) -> &'static str {
            self.session.lock().unwrap().status()
        }

        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, Value<'static>> {
            self.session.lock().unwrap().metadata()
        }

        #[zbus(property(emits_changed_signal = "false"))]
        fn position(&self) -> i64 {
            micros(self.session.lock().unwrap().position_secs())
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn rate(&self) -> f64 {
            1.0
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn minimum_rate(&self) -> f64 {
            1.0
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn maximum_rate(&self) -> f64 {
            1.0
        }

        /// The volume is the player bar's slider; it is not offered here.
        #[zbus(property(emits_changed_signal = "const"))]
        fn volume(&self) -> f64 {
            1.0
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_go_next(&self) -> bool {
            true
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_go_previous(&self) -> bool {
            true
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_play(&self) -> bool {
            true
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_pause(&self) -> bool {
            true
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_seek(&self) -> bool {
            true
        }

        #[zbus(property(emits_changed_signal = "const"))]
        fn can_control(&self) -> bool {
            true
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::mpsc;
        use std::time::Duration;

        use zbus::blocking::Connection;
        use zbus::zvariant::{OwnedObjectPath, OwnedValue};

        use super::*;

        const TIMEOUT: Duration = Duration::from_secs(5);
        const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F'];

        fn report(state: PlaybackState, path: Option<&str>, position_secs: f64) -> NowPlaying {
            NowPlaying {
                state,
                path: path.map(str::to_owned),
                title: "Title".into(),
                artist: "Artist".into(),
                album: "Album".into(),
                duration_secs: 200.0,
                position_secs,
            }
        }

        #[test]
        fn a_track_keeps_its_id_and_a_jump_is_a_seek() {
            let mut session = Session::default();
            let change =
                session.apply(report(PlaybackState::Playing, Some("/a"), 10.0), None, None);
            assert!(change.status && change.metadata && change.seeked.is_none());
            assert_eq!(session.track_id(), "/com/tagrex/track/1");

            // Pausing the same track: a new status, the same metadata.
            let change = session.apply(report(PlaybackState::Paused, Some("/a"), 10.0), None, None);
            assert!(change.status && !change.metadata && change.seeked.is_none());

            // A jump on the same track is announced as Seeked.
            let change = session.apply(report(PlaybackState::Paused, Some("/a"), 90.0), None, None);
            assert_eq!(change.seeked, Some(90_000_000));

            // Another track is another id and no seek.
            let change = session.apply(report(PlaybackState::Playing, Some("/b"), 0.0), None, None);
            assert!(change.status && change.metadata && change.seeked.is_none());
            assert_eq!(session.track_id(), "/com/tagrex/track/2");

            let change = session.apply(report(PlaybackState::Stopped, None, 0.0), None, None);
            assert!(change.status && change.metadata);
            assert_eq!(session.track_id(), NO_TRACK);
            assert_eq!(session.status(), "Stopped");
        }

        /// A service under a name of its own, so the bus tests run side by side.
        struct Fixture {
            service: Service,
            events: mpsc::Receiver<MediaControl>,
            client: Connection,
            name: String,
            covers: PathBuf,
        }

        fn fixture(tag: &str) -> Fixture {
            let pid = std::process::id();
            let name = format!("{BUS_NAME}.test_{tag}_{pid}");
            let covers = std::env::temp_dir().join(format!("tagrex-mpris-{tag}-{pid}"));
            let (sender, events) = mpsc::channel();
            let sink: Sink = Arc::new(move |control| {
                let _ = sender.send(control);
            });
            let service = Service::start(&name, sink, Some(covers.clone()))
                .expect("a session bus: run under dbus-run-session");
            let client = Connection::session().unwrap();
            Fixture {
                service,
                events,
                client,
                name,
                covers,
            }
        }

        impl Fixture {
            fn proxy(&self, interface: &'static str) -> zbus::blocking::Proxy<'_> {
                zbus::blocking::proxy::Builder::new(&self.client)
                    .destination(self.name.as_str())
                    .unwrap()
                    .path(PATH)
                    .unwrap()
                    .interface(interface)
                    .unwrap()
                    .cache_properties(zbus::proxy::CacheProperties::No)
                    .build()
                    .unwrap()
            }

            fn event(&self) -> MediaControl {
                self.events.recv_timeout(TIMEOUT).expect("an event")
            }

            fn metadata(&self) -> HashMap<String, OwnedValue> {
                self.proxy(PLAYER_IFACE).get_property("Metadata").unwrap()
            }

            /// The signals named `member` this service sends from now on.
            fn listen(&self, member: &'static str) -> mpsc::Receiver<zbus::Message> {
                let sender = self.service.connection.unique_name().unwrap().to_string();
                let (messages, received) = mpsc::channel();
                let (ready, subscribed) = mpsc::channel();
                std::thread::spawn(move || {
                    let connection = Connection::session().unwrap();
                    let rule = zbus::MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .sender(sender.as_str())
                        .unwrap()
                        .path(PATH)
                        .unwrap()
                        .member(member)
                        .unwrap()
                        .build();
                    let iterator =
                        zbus::blocking::MessageIterator::for_match_rule(rule, &connection, None)
                            .unwrap();
                    ready.send(()).unwrap();
                    for message in iterator.flatten() {
                        if messages.send(message).is_err() {
                            break;
                        }
                    }
                });
                subscribed.recv_timeout(TIMEOUT).unwrap();
                received
            }
        }

        fn string(value: &OwnedValue) -> String {
            String::try_from(value.try_clone().unwrap()).unwrap()
        }

        #[test]
        #[ignore = "needs a D-Bus session bus: run under dbus-run-session"]
        fn the_player_is_on_the_bus_with_what_the_frontend_reports() {
            let fx = fixture("report");
            let root = fx.proxy("org.mpris.MediaPlayer2");
            assert_eq!(root.get_property::<String>("Identity").unwrap(), "TagRex");
            assert_eq!(
                root.get_property::<String>("DesktopEntry").unwrap(),
                "TagRex"
            );
            let player = fx.proxy(PLAYER_IFACE);
            assert_eq!(
                player.get_property::<String>("PlaybackStatus").unwrap(),
                "Stopped"
            );
            for capability in [
                "CanPlay",
                "CanPause",
                "CanGoNext",
                "CanGoPrevious",
                "CanSeek",
            ] {
                assert!(
                    player.get_property::<bool>(capability).unwrap(),
                    "{capability}"
                );
            }
            let id = OwnedObjectPath::try_from(fx.metadata()["mpris:trackid"].try_clone().unwrap());
            assert_eq!(id.unwrap().as_str(), NO_TRACK);

            let changes = fx.listen("PropertiesChanged");
            fx.service.publish(
                report(PlaybackState::Playing, Some("/music/a.flac"), 10.0),
                Some(Some(JPEG.to_vec())),
            );
            let message = changes.recv_timeout(TIMEOUT).expect("PropertiesChanged");
            let (interface, changed, _): (String, HashMap<String, OwnedValue>, Vec<String>) =
                message.body().deserialize().unwrap();
            assert_eq!(interface, PLAYER_IFACE);
            assert_eq!(string(&changed["PlaybackStatus"]), "Playing");
            assert!(changed.contains_key("Metadata"));

            assert_eq!(
                player.get_property::<String>("PlaybackStatus").unwrap(),
                "Playing"
            );
            let metadata = fx.metadata();
            assert_eq!(string(&metadata["xesam:title"]), "Title");
            let artist = Vec::<String>::try_from(metadata["xesam:artist"].try_clone().unwrap());
            assert_eq!(artist.unwrap(), ["Artist"]);
            assert_eq!(string(&metadata["xesam:album"]), "Album");
            let length = i64::try_from(metadata["mpris:length"].try_clone().unwrap());
            assert_eq!(length.unwrap(), 200_000_000);
            let art = tauri::Url::parse(&string(&metadata["mpris:artUrl"])).unwrap();
            let art = art.to_file_path().unwrap();
            assert!(art.starts_with(&fx.covers) && art.extension().unwrap() == "jpg");
            assert_eq!(std::fs::read(&art).unwrap(), JPEG);
            let position = player.get_property::<i64>("Position").unwrap();
            assert!((10_000_000..12_000_000).contains(&position), "{position}");

            // A jump on the same track is a Seeked signal, not a property change.
            let seeks = fx.listen("Seeked");
            fx.service.publish(
                report(PlaybackState::Paused, Some("/music/a.flac"), 100.0),
                None,
            );
            let message = seeks.recv_timeout(TIMEOUT).expect("Seeked");
            assert_eq!(message.body().deserialize::<i64>().unwrap(), 100_000_000);
            assert_eq!(
                player.get_property::<String>("PlaybackStatus").unwrap(),
                "Paused"
            );
            assert_eq!(player.get_property::<i64>("Position").unwrap(), 100_000_000);

            // Stopping clears the track and its cover file.
            fx.service
                .publish(report(PlaybackState::Stopped, None, 0.0), Some(None));
            assert_eq!(
                player.get_property::<String>("PlaybackStatus").unwrap(),
                "Stopped"
            );
            assert!(!fx.metadata().contains_key("xesam:title"));
            assert!(!art.exists());
        }

        #[test]
        #[ignore = "needs a D-Bus session bus: run under dbus-run-session"]
        fn every_method_sends_the_frontend_its_event() {
            let fx = fixture("methods");
            let player = fx.proxy(PLAYER_IFACE);
            let actions = [
                ("PlayPause", "toggle"),
                ("Play", "play"),
                ("Pause", "pause"),
                ("Stop", "stop"),
                ("Next", "next"),
                ("Previous", "previous"),
            ];
            for (method, action) in actions {
                player.call_method(method, &()).unwrap();
                let event = fx.event();
                assert_eq!(
                    (event.action, event.position_secs),
                    (action, None),
                    "{method}"
                );
            }

            // Nothing to seek in while stopped.
            player.call_method("Seek", &(5_000_000i64)).unwrap();
            fx.service.publish(
                report(PlaybackState::Paused, Some("/music/a.flac"), 10.0),
                None,
            );

            player.call_method("Seek", &(5_000_000i64)).unwrap();
            let event = fx.event();
            assert_eq!((event.action, event.position_secs), ("seek", Some(15.0)));
            player.call_method("Seek", &(-60_000_000i64)).unwrap();
            assert_eq!(fx.event().position_secs, Some(0.0));
            // Past the end is the next track.
            player.call_method("Seek", &(500_000_000i64)).unwrap();
            assert_eq!(fx.event().action, "next");

            let id = OwnedObjectPath::try_from(fx.metadata()["mpris:trackid"].try_clone().unwrap())
                .unwrap();
            let elsewhere = ObjectPath::try_from("/com/tagrex/track/999").unwrap();
            // Another track's id, and a position past the end, are ignored.
            player
                .call_method("SetPosition", &(elsewhere, 30_000_000i64))
                .unwrap();
            player
                .call_method("SetPosition", &(id.as_ref(), 300_000_000i64))
                .unwrap();
            player
                .call_method("SetPosition", &(id.as_ref(), 30_000_000i64))
                .unwrap();
            let event = fx.event();
            assert_eq!((event.action, event.position_secs), ("seek", Some(30.0)));
            assert!(fx.events.try_recv().is_err());
        }
    }
}

#[cfg(windows)]
mod smtc {
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter, Manager};
    use windows::core::{factory, Interface, Ref, HSTRING};
    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::{
        MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
        SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    };
    use windows::Storage::Streams::{
        DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
    };
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
    use windows_future::IAsyncOperation;

    use super::{MediaControl, NowPlaying, PlaybackState, EVENT};

    /// The controls for the main window, once opened. `None` inside means they
    /// could not be — reported once, and not tried again on every update.
    static CONTROLS: Mutex<Option<Option<SystemMediaTransportControls>>> = Mutex::new(None);

    /// Nothing to set up at start: the controls belong to a window, so they are
    /// opened on the first update, when the window is certainly there.
    pub fn install(_app: &AppHandle) {}

    /// Hand `now` to the system media controls. `cover` is `None` to keep the
    /// current artwork, `Some` for a new track's (or its lack of one).
    pub fn publish(app: &AppHandle, now: NowPlaying, cover: Option<Option<Vec<u8>>>) {
        let mut slot = CONTROLS.lock().unwrap();
        let controls = slot.get_or_insert_with(|| match open(app) {
            Ok(controls) => Some(controls),
            Err(err) => {
                eprintln!("media session: no system media controls, media keys off: {err}");
                None
            }
        });
        if let Some(controls) = controls {
            if let Err(err) = apply(controls, now, cover) {
                eprintln!("media session: system media controls not updated: {err}");
            }
        }
    }

    /// The main window's controls, with the buttons on and their presses sent
    /// to the frontend.
    fn open(app: &AppHandle) -> windows::core::Result<SystemMediaTransportControls> {
        let window = app.get_webview_window("main").ok_or_else(|| {
            windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)
        })?;
        let hwnd = window
            .hwnd()
            .map_err(|_| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))?;
        let interop =
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        // SAFETY: a live window handle of this process, asked for the type the
        // call is documented to return.
        let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
        controls.SetIsEnabled(true)?;
        controls.SetIsPlayEnabled(true)?;
        controls.SetIsPauseEnabled(true)?;
        controls.SetIsStopEnabled(true)?;
        controls.SetIsNextEnabled(true)?;
        controls.SetIsPreviousEnabled(true)?;

        let emitter = app.clone();
        controls.ButtonPressed(&TypedEventHandler::new(
            move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
                let action = match args.ok()?.Button()? {
                    SystemMediaTransportControlsButton::Play => "play",
                    SystemMediaTransportControlsButton::Pause => "pause",
                    SystemMediaTransportControlsButton::Stop => "stop",
                    SystemMediaTransportControlsButton::Next => "next",
                    SystemMediaTransportControlsButton::Previous => "previous",
                    _ => return Ok(()),
                };
                let _ = emitter.emit(
                    EVENT,
                    MediaControl {
                        action,
                        position_secs: None,
                    },
                );
                Ok(())
            },
        ))?;

        // Dragging the position in the media flyout.
        let emitter = app.clone();
        controls.PlaybackPositionChangeRequested(&TypedEventHandler::new(
            move |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
                let position = args.ok()?.RequestedPlaybackPosition()?;
                let _ = emitter.emit(
                    EVENT,
                    MediaControl {
                        action: "seek",
                        position_secs: Some(position.Duration as f64 / TICKS_PER_SEC),
                    },
                );
                Ok(())
            },
        ))?;
        Ok(controls)
    }

    /// A `TimeSpan` counts 100-nanosecond ticks.
    const TICKS_PER_SEC: f64 = 10_000_000.0;

    fn ticks(secs: f64) -> TimeSpan {
        TimeSpan {
            Duration: (secs.max(0.0) * TICKS_PER_SEC).round() as i64,
        }
    }

    fn apply(
        controls: &SystemMediaTransportControls,
        now: NowPlaying,
        cover: Option<Option<Vec<u8>>>,
    ) -> windows::core::Result<()> {
        let updater = controls.DisplayUpdater()?;
        if now.state == PlaybackState::Stopped || now.path.is_none() {
            updater.ClearAll()?;
            updater.Update()?;
            return controls.SetPlaybackStatus(MediaPlaybackStatus::Stopped);
        }
        // A new track starts from a clean slate, so the last one's cover never
        // lingers on a track without one.
        if cover.is_some() {
            updater.ClearAll()?;
        }
        updater.SetType(MediaPlaybackType::Music)?;
        let music = updater.MusicProperties()?;
        music.SetTitle(&HSTRING::from(now.title.as_str()))?;
        music.SetArtist(&HSTRING::from(now.artist.as_str()))?;
        music.SetAlbumTitle(&HSTRING::from(now.album.as_str()))?;
        if let Some(Some(bytes)) = &cover {
            updater.SetThumbnail(&thumbnail(bytes)?)?;
        }
        updater.Update()?;
        controls.SetPlaybackStatus(if now.state == PlaybackState::Playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        })?;

        // The flyout's own seek bar.
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(ticks(0.0))?;
        timeline.SetEndTime(ticks(now.duration_secs))?;
        timeline.SetMinSeekTime(ticks(0.0))?;
        timeline.SetMaxSeekTime(ticks(now.duration_secs))?;
        timeline.SetPosition(ticks(now.position_secs))?;
        controls.UpdateTimelineProperties(&timeline)
    }

    /// The cover as a stream the controls can read, held in memory. Waiting on
    /// the store is a copy into memory, not I/O.
    fn thumbnail(bytes: &[u8]) -> windows::core::Result<RandomAccessStreamReference> {
        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(bytes)?;
        writer
            .StoreAsync()?
            .cast::<IAsyncOperation<u32>>()?
            .join()?;
        writer.DetachStream()?;
        stream.Seek(0)?;
        RandomAccessStreamReference::CreateFromStream(&stream)
    }
}
