use std::time::Duration;

use souvlaki::Error;
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
    controls: SouvlakiMediaControls,
}

impl MediaControls {
    pub fn new(window: &WebviewWindow, app_handle: AppHandle) -> Self {
        #[cfg(not(target_os = "windows"))]
        let hwnd = None;

        #[cfg(target_os = "windows")]
        let hwnd = Some(window.hwnd().expect("Failed to receive HWND").0);
        let config = PlatformConfig {
            dbus_name: "blacktape",
            display_name: "Blacktape Desktop",
            hwnd,
        };
        let mut controls =
            SouvlakiMediaControls::new(config).expect("Failed to create MediaControls");

        let app_handle_clone = app_handle.clone();
        controls
            .attach(move |event: MediaControlEvent| match event {
                MediaControlEvent::Play => {
                    let _ = app_handle_clone.emit("media-resume", ());
                }
                MediaControlEvent::Pause => {
                    let _ = app_handle_clone.emit("media-pause", ());
                }
                MediaControlEvent::Stop => {
                    let _ = app_handle_clone.emit("media-stop", ());
                }
                MediaControlEvent::Next => {
                    let _ = app_handle_clone.emit("media-next", ());
                }
                MediaControlEvent::Previous => {
                    let _ = app_handle_clone.emit("media-previous", ());
                }
                MediaControlEvent::Toggle => {
                    let _ = app_handle_clone.emit("media-toggle", ());
                }
                _ => {}
            })
            .unwrap();

        controls
            .set_playback(MediaPlayback::Stopped)
            .expect("Failed to set playback");

        Self { controls }
    }

    pub fn update_metadata(&mut self, metadata: MediaMetadata) {
        self.controls
            .set_metadata(metadata)
            .expect("Failed to set metadata");
    }

    pub fn play(&mut self) -> Result<(), Error> {
        self.controls.set_playback(MediaPlayback::Playing {
            progress: Some(MediaPosition(Duration::ZERO)),
        })
    }

    pub fn pause(&mut self) -> Result<(), Error> {
        self.controls.set_playback(MediaPlayback::Paused {
            progress: Some(MediaPosition(Duration::ZERO)),
        })
    }

    pub fn stop(&mut self) -> Result<(), Error> {
        self.controls.set_playback(MediaPlayback::Stopped)
    }
}
