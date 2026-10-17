//! The system media session (#218): the Mac's media keys, the Now Playing
//! widget and headphone controls drive the preview player.
//!
//! The app registers with the OS as a media player rather than grabbing the
//! keys: macOS routes Play/Next/Previous to whichever app is "now playing",
//! so no Accessibility permission is needed and another player keeps its keys
//! while it is the one playing. That routing only works while the app says
//! what it is playing, which is why the frontend reports every change of
//! track and state through [`update`].
//!
//! A remote command reaches the frontend as the [`EVENT`] event and presses
//! the same transport buttons a click would. Other platforms compile the
//! no-op versions below.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use tagrex_player::Player;

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

/// No system media session off macOS yet.
#[cfg(not(target_os = "macos"))]
pub fn install(_app: &AppHandle) {}

#[cfg(not(target_os = "macos"))]
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
