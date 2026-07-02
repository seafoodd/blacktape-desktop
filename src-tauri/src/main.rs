// Prevents additional console window on Windows in release, DO NOT REMOVE!!
// #![cfg_attr(not(debug_assertions), windows_subsystem = "console")]

#[cfg(target_os = "windows")]
mod win_hide {
    use std::ffi::c_void;

    extern "system" {
        fn GetConsoleWindow() -> *mut c_void;
        fn ShowWindow(hWnd: *mut c_void, nCmdShow: i32) -> i32;
    }

    const SW_HIDE: i32 = 0;

    pub unsafe fn hide_console() {
        let window = GetConsoleWindow();
        if !window.is_null() {
            ShowWindow(window, SW_HIDE);
        }
    }
}

fn main() {
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    unsafe {
        win_hide::hide_console();
    }

    // Launch your music player
    blacktape_desktop_lib::run();
}
