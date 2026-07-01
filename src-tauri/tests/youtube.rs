use blacktape_desktop_lib::download::youtube::parse_album;

#[tokio::test]
#[ignore]
async fn test_real_album_parsing() {
    let urls = [
        "https://music.youtube.com/browse/MPREb_5TFkAq5mYDo", // The Doors - The Doors
        "https://music.youtube.com/browse/MPREb_6KTedIfvZMt", // The Powers That B - Death Grips
        "MPREb_QGz4un0op6F",                                  // plastic death - glass beach
    ];

    for url in urls {
        match parse_album(url).await {
            Ok(album) => {
                let tracks = album.tracks;

                assert!(
                    !tracks.is_empty(),
                    "Track list should not be empty for a valid album: {}",
                    url
                );

                let first_track = &tracks[0];

                println!("\n--- Verified Live YouTube Parsing ---");
                println!("Album: {}", url);
                println!("Successfully parsed {} tracks.", tracks.len());
                println!(
                    "First track output: {} -> {}",
                    first_track.file_name, first_track.url
                );
                println!("---------------------------------------\n");
            }
            Err(e) => panic!("Failed to parse live YouTube album ({}): {}", url, e),
        }
    }

    // Wrong browse_url
    assert!(
        parse_album("https://music.youtube.com/browse/Mqxbb_QFz4Gn0Hp6F")
            .await
            .is_err(),
        "Should fail: Invalid URL mistakenly parsed successfully"
    );

    // Wrong browse_id
    assert!(
        parse_album("Fqgqj_QDz58n08p6A").await.is_err(),
        "Should fail: Invalid browse_id mistakenly parsed successfully"
    );

    // Large playlist (The Full Bull of Heaven Discography (Chronologically) - playlist by Quinny)
    match parse_album("https://music.youtube.com/playlist?list=PLgeTf6bdBvoJJUr11oJVFnPd3XHNkt9tX")
        .await
    {
        Ok(_) => panic!("Should fail: Only browse urls are supported"),
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("playlist") || err_msg.contains("unsupported"),
                "Expected an unsupported URL error, but got a different error: {}",
                err_msg
            );
        }
    }
}
