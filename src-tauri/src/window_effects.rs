//! Native blur behind a see-through window ("Background Blur" in Settings).
//!
//! Windows: acrylic through the window accent API, which keeps the blur while the window is not
//! focused. A window without decorations shows it as flat black unless the DWM frame is extended
//! over the whole client area, so that is done together with the effect.
//! macOS: vibrancy (needs `macOSPrivateApi`, which the transparent window needs anyway).
//! Other platforms have no window effects in Tauri; the setting is hidden there.

use tauri::{Runtime, WebviewWindow};

/// Whether this platform can blur what is behind the window.
#[tauri::command]
pub fn window_blur_supported() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

/// Turns the blur behind the calling window on or off. `tint` is the theme's background colour
/// (RGB); Windows mixes it into the acrylic, other platforms ignore it.
#[tauri::command]
pub fn set_window_blur<R: Runtime>(
    window: WebviewWindow<R>,
    enabled: bool,
    tint: [u8; 3],
) -> Result<(), String> {
    platform::apply(&window, enabled, tint)
}

#[cfg(target_os = "windows")]
mod platform {
    use std::ffi::c_void;
    use tauri::{Runtime, WebviewWindow};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::UI::Controls::MARGINS;

    const ACCENT_DISABLED: u32 = 0;
    const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    /// Draw the accent over the whole window, not only inside its borders.
    const ACCENT_FLAG_ALL_BORDERS: u32 = 2;
    /// Alpha of the acrylic tint. Acrylic ignores a fully transparent tint; the tint colour still
    /// sets how dark the blur is (black makes it almost black), so it gets the theme's background.
    const ACRYLIC_TINT_ALPHA: u32 = 1;
    const WCA_ACCENT_POLICY: u32 = 19;

    #[repr(C)]
    struct AccentPolicy {
        accent_state: u32,
        accent_flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }

    #[repr(C)]
    struct WindowCompositionAttribData {
        attrib: u32,
        data: *mut c_void,
        size: usize,
    }

    type SetWindowCompositionAttribute =
        unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> i32;

    pub fn apply<R: Runtime>(
        window: &WebviewWindow<R>,
        enabled: bool,
        tint: [u8; 3],
    ) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| format!("window handle: {e}"))?.0 as HWND;
        // The legacy accent API, not the Windows 11 system backdrop (`Effect::Acrylic`): the
        // backdrop turns into a flat grey fill whenever the window loses focus, the accent stays.
        set_accent(hwnd, accent_policy(enabled, tint))?;
        extend_frame(hwnd, frame_margin(enabled))
    }

    /// (state, flags, ABGR tint) for the window accent.
    pub(super) fn accent_policy(enabled: bool, [r, g, b]: [u8; 3]) -> (u32, u32, u32) {
        if enabled {
            let tint = (ACRYLIC_TINT_ALPHA << 24)
                | (u32::from(b) << 16)
                | (u32::from(g) << 8)
                | u32::from(r);
            (
                ACCENT_ENABLE_ACRYLICBLURBEHIND,
                ACCENT_FLAG_ALL_BORDERS,
                tint,
            )
        } else {
            (ACCENT_DISABLED, 0, 0)
        }
    }

    /// -1 extends the frame over the whole client area ("sheet of glass"); 0 restores the default.
    /// Without it a window with no decorations draws the accent as flat black.
    pub(super) fn frame_margin(enabled: bool) -> i32 {
        if enabled {
            -1
        } else {
            0
        }
    }

    fn set_accent(hwnd: HWND, (state, flags, tint): (u32, u32, u32)) -> Result<(), String> {
        // SAFETY: user32 is always loaded in a GUI process; the name is a NUL-terminated literal.
        let proc = unsafe {
            let user32 = GetModuleHandleA(c"user32.dll".as_ptr().cast());
            if user32.is_null() {
                return Err("user32.dll is not loaded".into());
            }
            GetProcAddress(user32, c"SetWindowCompositionAttribute".as_ptr().cast())
        };
        let Some(proc) = proc else {
            return Err("SetWindowCompositionAttribute is not available".into());
        };
        // SAFETY: the exported function has this signature (Windows 7 and later).
        let set: SetWindowCompositionAttribute = unsafe { std::mem::transmute(proc) };
        let mut policy = AccentPolicy {
            accent_state: state,
            accent_flags: flags,
            gradient_color: tint,
            animation_id: 0,
        };
        let mut data = WindowCompositionAttribData {
            attrib: WCA_ACCENT_POLICY,
            data: (&mut policy as *mut AccentPolicy).cast(),
            size: std::mem::size_of::<AccentPolicy>(),
        };
        // SAFETY: `hwnd` is this process's live window; `data` and `policy` outlive the call.
        if unsafe { set(hwnd, &mut data) } == 0 {
            return Err("SetWindowCompositionAttribute failed".into());
        }
        Ok(())
    }

    fn extend_frame(hwnd: HWND, margin: i32) -> Result<(), String> {
        let margins = MARGINS {
            cxLeftWidth: margin,
            cxRightWidth: margin,
            cyTopHeight: margin,
            cyBottomHeight: margin,
        };
        // SAFETY: `hwnd` is the live handle of this process's window and `margins` outlives the call.
        let hr = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) };
        if hr < 0 {
            return Err(format!("DwmExtendFrameIntoClientArea failed: 0x{hr:08X}"));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use tauri::window::{Effect, EffectState, EffectsBuilder};
    use tauri::{Runtime, WebviewWindow};

    pub fn apply<R: Runtime>(
        window: &WebviewWindow<R>,
        enabled: bool,
        _tint: [u8; 3],
    ) -> Result<(), String> {
        let result = if enabled {
            window.set_effects(
                EffectsBuilder::new()
                    .effect(Effect::UnderWindowBackground)
                    .state(EffectState::FollowsWindowActiveState)
                    .build(),
            )
        } else {
            window.set_effects(None)
        };
        result.map_err(|e| format!("set window effects: {e}"))
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use tauri::{Runtime, WebviewWindow};

    pub fn apply<R: Runtime>(
        _window: &WebviewWindow<R>,
        enabled: bool,
        _tint: [u8; 3],
    ) -> Result<(), String> {
        if enabled {
            Err("window blur is not supported on this platform".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_is_supported_only_on_windows_and_macos() {
        assert_eq!(
            window_blur_supported(),
            cfg!(any(target_os = "windows", target_os = "macos"))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn frame_covers_the_whole_window_only_while_blurred() {
        assert_eq!(platform::frame_margin(true), -1);
        assert_eq!(platform::frame_margin(false), 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn acrylic_tint_is_the_theme_colour_in_abgr_with_visible_alpha() {
        let (state, _, tint) = platform::accent_policy(true, [0x0F, 0x10, 0x15]);
        assert_eq!(state, 4);
        assert_eq!(tint, 0x0115_100F);
        assert_ne!(tint >> 24, 0, "acrylic ignores a fully transparent tint");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn turning_blur_off_clears_the_accent() {
        assert_eq!(
            platform::accent_policy(false, [0x0F, 0x10, 0x15]),
            (0, 0, 0)
        );
    }
}
