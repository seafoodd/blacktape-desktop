use blacktape_desktop_lib::download::bandcamp::parse_album;

#[tokio::test]
#[ignore]
async fn test_real_album_parsing() {
    let url = "https://aphextwin.bandcamp.com/album/syro";

    match parse_album(url).await {
        Ok(album) => {
            let tracks = album.tracks;

            assert!(
                !tracks.is_empty(),
                "Track list should not be empty for a valid album"
            );

            let first_track = &tracks[0];
            assert!(
                first_track
                    .url
                    .starts_with("https://aphextwin.bandcamp.com/track/"),
                "Track URL format is unexpected: {}",
                first_track.url
            );
            assert!(
                first_track.file_name.starts_with("01. "),
                "Track filename padding format is unexpected: {}",
                first_track.file_name
            );

            println!("\n--- Verified Live Bandcamp Parsing ---");
            println!("Album: {}", url);
            println!("Successfully parsed {} tracks.", tracks.len());
            println!(
                "First track output: {} -> {}",
                first_track.file_name, first_track.url
            );
            println!("---------------------------------------\n");
        }
        Err(e) => panic!("Failed to parse live Bandcamp album: {}", e),
    }
}
