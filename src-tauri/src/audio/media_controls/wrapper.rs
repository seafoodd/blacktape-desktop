//! The operating system's media controls (Windows SMTC, MPRIS on Linux, Now Playing on macOS).
//!
//! They are a convenience: when the platform refuses them (no session bus, no window handle yet) the player works
//! without them, and every later call is a no-op. Errors are logged, never raised.

use std::time::Duration;

use souvlaki::MediaControlEvent;
use souvlaki::MediaControls as SouvlakiMediaControls;
use souvlaki::MediaMetadata;
use souvlaki::MediaPlayback;
use souvlaki::MediaPosition;
use souvlaki::PlatformConfig;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::WebviewWindow;

pub struct MediaControls {
    controls: Option<SouvlakiMediaControls>,
}

impl MediaControls {
    pub fn new(window: &WebviewWindow, app_handle: AppHandle) -> Self {
        match Self::create(window, app_handle) {
            Ok(controls) => Self { controls: Some(controls) },
            Err(e) => {
                eprintln!("[media controls] unavailable, continuing without them: {e}");
                Self { controls: None }
            }
        }
    }

    fn create(window: &WebviewWindow, app_handle: AppHandle) -> Result<SouvlakiMediaControls, String> {
        #[cfg(not(target_os = "windows"))]
        let hwnd = {
            let _ = window;
            None
        };
        #[cfg(target_os = "windows")]
        let hwnd = Some(window.hwnd().map_err(|e| format!("no window handle: {e}"))?.0);

        let config = PlatformConfig {
            dbus_name: "blacktape",
            display_name: "Blacktape Desktop",
            hwnd,
        };
        let mut controls = SouvlakiMediaControls::new(config).map_err(|e| format!("{e:?}"))?;
        controls
            .attach(move |event: MediaControlEvent| {
                let name = match event {
                    MediaControlEvent::Play => "media-resume",
                    MediaControlEvent::Pause => "media-pause",
                    MediaControlEvent::Stop => "media-stop",
                    MediaControlEvent::Next => "media-next",
                    MediaControlEvent::Previous => "media-previous",
                    MediaControlEvent::Toggle => "media-toggle",
                    _ => return,
                };
                let _ = app_handle.emit(name, ());
            })
            .map_err(|e| format!("{e:?}"))?;
        controls.set_playback(MediaPlayback::Stopped).map_err(|e| format!("{e:?}"))?;
        Ok(controls)
    }

    pub fn update_metadata(&mut self, metadata: MediaMetadata) {
        if let Some(controls) = self.controls.as_mut() {
            if let Err(e) = controls.set_metadata(metadata) {
                eprintln!("[media controls] cannot set metadata: {e:?}");
            }
        }
    }

    pub fn set_playing(&mut self, position: Duration) {
        self.set_playback(MediaPlayback::Playing { progress: Some(MediaPosition(position)) });
    }

    pub fn set_paused(&mut self, position: Duration) {
        self.set_playback(MediaPlayback::Paused { progress: Some(MediaPosition(position)) });
    }

    pub fn set_stopped(&mut self) {
        self.set_playback(MediaPlayback::Stopped);
    }

    fn set_playback(&mut self, playback: MediaPlayback) {
        if let Some(controls) = self.controls.as_mut() {
            if let Err(e) = controls.set_playback(playback) {
                eprintln!("[media controls] cannot set the playback state: {e:?}");
            }
        }
    }
}
