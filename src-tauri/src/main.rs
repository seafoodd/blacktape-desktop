#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(target_os = "windows", not(debug_assertions)))]
mod win_hide {
    use std::ffi::c_void;

    extern "system" {
        fn AllocConsole() -> i32;
        fn GetConsoleWindow() -> *mut c_void;
        fn ShowWindow(hWnd: *mut c_void, nCmdShow: i32) -> i32;
        fn SetWindowPos(
            hWnd: *mut c_void,
            hWndInsertAfter: *mut c_void,
            X: i32,
            Y: i32,
            cx: i32,
            cy: i32,
            uFlags: u32,
        ) -> i32;
    }

    const SW_HIDE: i32 = 0;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;

    pub unsafe fn hide_console() {
        AllocConsole();

        let window = GetConsoleWindow();
        if !window.is_null() {
            // try to hide the console window
            ShowWindow(window, SW_HIDE);

            // fallback: move it far off-screen
            SetWindowPos(
                window,
                std::ptr::null_mut(),
                -32000,
                -32000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
        }
    }
}

fn main() {
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    unsafe {
        win_hide::hide_console();
    }

    blacktape_desktop_lib::run();
}
