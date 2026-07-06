use std::fs;
use tempfile::tempdir;

use blacktape_desktop_lib::scan::{old_scan_music_dir, scan_music_dir};

#[tokio::test]
async fn test_parallel_vs_sequential_scanner() {
    let tmp = tempdir().unwrap();
    let music_dir = tmp.path().join("Music");
    let covers_dir = tmp.path().join("Covers");

    fs::create_dir_all(&music_dir).unwrap();

    let song1_path = music_dir.join("Bohemian Rhapsody.mp3");
    let song2_path = music_dir.join("Stairway To Heaven.flac");

    let mut id3_dummy = b"ID3\x03\x00\x00\x00\x00\x00\x00".to_vec();
    id3_dummy.resize(1024, 0);
    fs::write(&song1_path, id3_dummy).unwrap();

    let mut flac_dummy = b"fLaC".to_vec();
    flac_dummy.resize(1024, 0);
    fs::write(&song2_path, flac_dummy).unwrap();

    let parallel_results = scan_music_dir(music_dir.to_string_lossy().to_string(), &covers_dir);

    if covers_dir.exists() {
        fs::remove_dir_all(&covers_dir).unwrap();
    }
    let sequential_results =
        old_scan_music_dir(music_dir.to_string_lossy().to_string(), &covers_dir);

    assert_eq!(
        parallel_results.len(),
        sequential_results.len(),
        "Both scanners must find the exact same number of files"
    );

    let mut p_songs = parallel_results;
    let mut s_songs = sequential_results;
    p_songs.sort_by_key(|s| s.path.clone());
    s_songs.sort_by_key(|s| s.path.clone());

    for (p_song, s_song) in p_songs.iter().zip(s_songs.iter()) {
        assert_eq!(p_song.path, s_song.path);

        assert_eq!(
            p_song.title, s_song.title,
            "Mismatch found on title parsing rules!"
        );
    }
}
