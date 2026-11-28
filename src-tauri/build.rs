fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // `tauri_build` only embeds the Common Controls v6 manifest into the main binary.
        // Test binaries also link `comctl32.dll` (`TaskDialogIndirect` via `rfd`), so embed
        // the v6 dependency manifest into test targets to avoid 0xc0000139 on launch.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        // Binaries already embed a manifest via `tauri_build`'s `resource.lib`; disable the
        // extra `/MANIFEST:EMBED` on bins so `cvtres` doesn't fail with CVT1100.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    }

    tauri_build::try_build(tauri_build::Attributes::new().plugin(
        "torrent",
        tauri_build::InlinedPlugin::new().commands(&[
            "search_torrent_releases",
            "inspect_torrent_release",
            "resolve_torrent_album",
            "cancel_torrent_inspection",
            "list_torrent_indexers",
            "set_torrent_indexer_enabled",
            "test_torrent_indexer",
            "open_rutracker_login",
            "get_rutracker_status",
            "sign_in_rutracker",
            "sign_out_rutracker",
            "is_rutracker_enabled",
        ]),
    ))
    .expect("failed to build Tauri application metadata");
}
