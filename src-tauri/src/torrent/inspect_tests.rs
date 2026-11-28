use super::*;

fn multifile(files: &[(u64, &[&[u8]])]) -> Vec<u8> {
    let mut torrent = b"d4:infod5:filesl".to_vec();
    for (length, path) in files {
        torrent.extend(format!("d6:lengthi{length}e4:pathl").as_bytes());
        for part in *path {
            torrent.extend(format!("{}:", part.len()).as_bytes());
            torrent.extend(*part);
        }
        torrent.extend(b"ee");
    }
    torrent.extend(b"e4:name7:Release12:piece lengthi16384e6:pieces20:abcdefghijklmnopqrstee");
    torrent
}

#[test]
fn sorting_tracks_keeps_their_original_file_indexes_and_unknown_container_codecs() {
    let torrent = multifile(&[
        (15, &[b"front.jpg"]),
        (50, &[b"CD 2", b"01 - Song.M4A"]),
        (50, &[b"CD 1", b"01 - Song.FLAC"]),
        (30, &[b"CD 1", b"1999.flac"]),
    ]);
    let release = parse_metainfo(&torrent, None).unwrap();
    assert_eq!(release.name, "Release");
    assert_eq!(
        release.layout,
        super::super::release::ReleaseLayout::MultiDiscAlbum
    );
    assert_eq!(release.cover_art[0].file_index, 0);
    assert_eq!(
        release
            .tracks
            .iter()
            .map(|track| track.file_index)
            .collect::<Vec<_>>(),
        vec![2, 3, 1],
    );
    assert_eq!(release.tracks[0].track_number, Some(1));
    assert_eq!(release.tracks[1].title, "1999");
    assert_eq!(release.tracks[1].track_number, None);
    assert_eq!(release.tracks[2].codec, "M4A");
}

#[test]
fn rejects_paths_that_can_escape_or_alias_windows_files_and_keeps_lossy_names_distinct() {
    for part in [
        b"..".as_slice(),
        b"a/b",
        b"\\host",
        b"C:",
        b"file:name.flac",
        b"con.flac",
        b"foo.",
    ] {
        let torrent = multifile(&[(10, &[part, b"valid.flac"])]);
        assert!(parse_metainfo(&torrent, None).is_err(), "accepted {part:?}");
    }
    let torrent = multifile(&[(10, &[b"\xff.flac"]), (10, &[b"\xfe.flac"])]);
    let release = parse_metainfo(&torrent, None).unwrap();
    assert_ne!(release.tracks[0].file_path, release.tracks[1].file_path);
    assert_eq!(release.tracks[0].title, release.tracks[1].title);
    let duplicate = multifile(&[(10, &[b"Song.flac"]), (10, &[b"Song.flac"])]);
    assert!(parse_metainfo(&duplicate, None).is_err());
}

#[test]
fn rejects_bad_piece_layouts_untrusted_lengths_and_unverified_hashes() {
    let mut torrent = multifile(&[(15, &[b"Album.CUE"]), (50, &[b"Album.FLAC"])]);
    let release = parse_metainfo(&torrent, None).unwrap();
    assert_eq!(
        release.layout,
        super::super::release::ReleaseLayout::CueImageAlbum
    );
    assert_eq!(release.tracks.len(), 1);
    assert_eq!(release.cue_sheets[0].file_index, 0);
    assert!(parse_metainfo(&torrent, Some("0123456789012345678901234567890123456789")).is_err());
    torrent.pop();
    assert!(parse_metainfo(&torrent, None).is_err());
    assert!(parse_metainfo(b"d4:infod99999999999999999999999999999999999999:ee", None).is_err());
    let deep = [b"d1:x".as_slice(), &[b'l'; 80], &[b'e'; 81]].concat();
    assert!(parse_metainfo(&deep, None).is_err());
    let torrent = b"d4:infod6:lengthi10e4:name9:Song.flac12:piece lengthi0e6:pieces0:ee";
    assert!(parse_metainfo(torrent, None).is_err());
}

#[test]
fn rejects_private_routes_and_mixed_dns_answers_before_connecting() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "100.64.0.1",
        "169.254.169.254",
        "192.0.0.9",
        "198.19.0.1",
        "224.0.0.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "2001:db8::1",
        "2002:7f00:1::",
    ] {
        assert!(!is_public_address(address.parse().unwrap()), "{address}");
    }
    assert!(is_public_address("8.8.8.8".parse().unwrap()));
    assert!(is_public_address("2606:4700::1111".parse().unwrap()));
    assert!(validate_resolved(&[
        "8.8.8.8:443".parse().unwrap(),
        "127.0.0.1:443".parse().unwrap(),
    ])
    .is_err());
    assert!(parse_http_url("https://user:token@archive.org/download/x/x_archive.torrent").is_err());
    assert!(parse_http_url("https://archive.org/download/x/x_archive.torrent").is_ok());
    assert!(parse_http_url("file:///C:/music.torrent").is_err());
}

#[test]
fn magnet_hash_accepts_equal_hex_and_base32_rejecting_conflicting_or_unbounded_selection() {
    let expected = "0101010101010101010101010101010101010101";
    let magnet = format!(
        "magnet:?xt=urn:btih:{expected}&xt=urn:btih:aeaqcaibaeaqcaibaeaqcaibaeaqcaib&so=0-4294967295"
    );
    assert_eq!(magnet_info_hash(&magnet).unwrap(), expected);
    assert!(magnet_info_hash(&format!(
        "{magnet}&xt=urn:btih:0202020202020202020202020202020202020202"
    ))
    .is_err());
    assert!(
        magnet_info_hash("magnet:?xt=urn:btih:0000000000000000000000000000000000000000").is_err()
    );
    let input = parse_magnet(&format!(
        "{magnet}&x.pe=127.0.0.1:6881&x.pe=[::1]:6881&x.pe=8.8.8.8:0&x.pe=8.8.8.8:6881"
    ))
    .unwrap();
    assert_eq!(input.peers, ["8.8.8.8:6881".parse::<SocketAddr>().unwrap()]);
}

#[tokio::test]
async fn peer_inspection_exchanges_metadata_only_and_closes_the_socket() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const INFO: &[u8] =
        b"d6:lengthi10e4:name9:Song.flac12:piece lengthi16384e6:pieces20:abcdefghijklmnopqrste";
    const HASH: &str = "542f9566de344a062855a45950546002e7a9e649";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut handshake = [0u8; 68];
        socket.read_exact(&mut handshake).await.unwrap();
        assert_eq!(&handshake[..20], b"\x13BitTorrent protocol");
        assert_eq!(handshake[25] & 16, 16);
        socket.write_all(&handshake).await.unwrap();

        let mut request = vec![0; socket.read_u32().await.unwrap() as usize];
        socket.read_exact(&mut request).await.unwrap();
        assert_eq!(request, b"\x14\x00d1:md11:ut_metadatai1eee");
        let response = format!(
            "\x14\x00d1:md11:ut_metadatai7ee13:metadata_sizei{}ee",
            INFO.len()
        );
        socket.write_u32(response.len() as u32).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();

        let mut request = vec![0; socket.read_u32().await.unwrap() as usize];
        socket.read_exact(&mut request).await.unwrap();
        assert_eq!(request, b"\x14\x07d8:msg_typei0e5:piecei0ee");
        let response = format!(
            "\x14\x01d8:msg_typei1e5:piecei0e10:total_sizei{}ee",
            INFO.len()
        );
        socket
            .write_u32((response.len() + INFO.len()) as u32)
            .await
            .unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.write_all(INFO).await.unwrap();
        assert_eq!(socket.read(&mut [0]).await.unwrap(), 0);
    });
    let release = super::peer::from_peer(addr, HASH).await.unwrap();
    assert_eq!(release.info_hash, HASH);
    assert_eq!(release.tracks[0].file_index, 0);
    assert_eq!(release.tracks[0].file_path, "Song.flac");
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn dropping_an_inspection_closes_pending_peer_sockets() {
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(super::peer::try_peers(
        vec![address],
        "542f9566de344a062855a45950546002e7a9e649",
    ));
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    stream.read_exact(&mut [0; 68]).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut [0]))
        .await
        .unwrap();
    assert_eq!(read.unwrap(), 0);
}

#[tokio::test]
#[ignore = "queries the public torrent metadata cache"]
async fn inspects_the_cache_operators_public_ubuntu_example_by_magnet() {
    let release = inspect("magnet:?xt=urn:btih:B415C913643E5FF49FE37D304BBB5E6E11AD5101")
        .await
        .unwrap();
    assert_eq!(
        release.info_hash,
        "b415c913643e5ff49fe37d304bbb5e6e11ad5101"
    );
    assert_eq!(release.layout, ReleaseLayout::NonAudio);
    assert!(release.tracks.is_empty());
    assert_eq!(release.total_file_count, 1);
}

#[test]
fn cached_torrent_bytes_matches_magnet_hash_and_torrent_url() {
    let sample_hash = "0123456789abcdef0123456789abcdef01234567";
    let payload = b"d8:announce19:http://tracker/test4:infod4:name4:testee".to_vec();
    cache_torrent_bytes(sample_hash, &payload);

    let from_magnet = cached_torrent_bytes(&format!(
        "magnet:?xt=urn:btih:{}&dn=test",
        sample_hash.to_ascii_uppercase()
    ));
    assert_eq!(from_magnet.as_deref(), Some(payload.as_slice()));

    let url = "https://archive.org/download/test/test_archive.torrent";
    cache_torrent_bytes(url, &payload);
    assert_eq!(cached_torrent_bytes(url).as_deref(), Some(payload.as_slice()));
}
