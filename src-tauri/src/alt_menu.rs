//! Windows: releasing Alt on its own (or F10) puts a window into menu mode, where the mouse belongs to the menu
//! until a key or a click ends it. The app has no menu bar, so nothing shows: the page just stops getting mouse
//! moves, and whatever was under the pointer stays hovered. Every window of the app refuses that mode.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("alt-menu")
        .on_window_ready(|window| {
            #[cfg(target_os = "windows")]
            platform::block_menu_mode(&window);
            #[cfg(not(target_os = "windows"))]
            let _ = window;
        })
        .build()
}

#[cfg(target_os = "windows")]
mod platform {
    use tauri::{Runtime, Window};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SC_KEYMENU, WM_NCDESTROY, WM_SYSCOMMAND};

    /// Id of the subclass below; any value unique to this module.
    const SUBCLASS_ID: usize = 0x0B1A_A171;

    pub fn block_menu_mode<R: Runtime>(window: &Window<R>) {
        let Ok(handle) = window.hwnd() else {
            tracing::warn!("no window handle: Alt can freeze the mouse in {}", window.label());
            return;
        };
        let raw = handle.0 as isize;
        // A subclass must be installed from the window's own thread.
        let queued = window.run_on_main_thread(move || {
            // SAFETY: `raw` is this process's live window; the procedure is a plain function.
            unsafe { SetWindowSubclass(raw as HWND, Some(subclass_proc), SUBCLASS_ID, 0) };
        });
        if let Err(e) = queued {
            tracing::warn!("Alt can freeze the mouse in {}: {e}", window.label());
        }
    }

    /// Whether the message starts menu mode from Alt or F10 alone. lParam is the character typed together
    /// with Alt, so Alt+Space still opens the window's system menu.
    pub(super) fn starts_menu_mode(msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        // Windows uses the low four bits of wParam itself.
        msg == WM_SYSCOMMAND && wparam & 0xFFF0 == SC_KEYMENU as WPARAM && lparam == 0
    }

    pub(super) unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if starts_menu_mode(msg, wparam, lparam) {
            return 0;
        }
        if msg == WM_NCDESTROY {
            // SAFETY: the window is still alive here, and a subclass has to be removed before it is gone.
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
        }
        // SAFETY: Windows calls this with the live handle of the subclassed window.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::platform::{starts_menu_mode, subclass_proc};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SC_CLOSE, SC_KEYMENU, WM_SYSCOMMAND, WM_SYSKEYUP,
    };

    #[test]
    fn alt_or_f10_alone_starts_menu_mode() {
        assert!(starts_menu_mode(WM_SYSCOMMAND, SC_KEYMENU as usize, 0));
    }

    #[test]
    fn keymenu_with_low_wparam_bits_starts_menu_mode() {
        assert!(starts_menu_mode(WM_SYSCOMMAND, (SC_KEYMENU | 0xF) as usize, 0));
    }

    #[test]
    fn alt_space_keeps_system_menu() {
        assert!(!starts_menu_mode(WM_SYSCOMMAND, SC_KEYMENU as usize, ' ' as isize));
    }

    #[test]
    fn other_system_command_does_not_start_menu_mode() {
        assert!(!starts_menu_mode(WM_SYSCOMMAND, SC_CLOSE as usize, 0));
    }

    #[test]
    fn other_message_does_not_start_menu_mode() {
        assert!(!starts_menu_mode(WM_SYSKEYUP, SC_KEYMENU as usize, 0));
    }

    #[test]
    fn subclass_proc_swallows_alt_menu_message() {
        // SAFETY: a message that starts menu mode is answered without touching the window handle.
        let result = unsafe {
            subclass_proc(std::ptr::null_mut(), WM_SYSCOMMAND, SC_KEYMENU as usize, 0, 0, 0)
        };
        assert_eq!(result, 0);
    }
}
