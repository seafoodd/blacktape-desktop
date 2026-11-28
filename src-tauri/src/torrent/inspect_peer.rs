use super::{bencode_prefix, parse_metainfo, InspectedRelease, MAX_METADATA_BYTES};
use librqbit::dht::Id20;
use serde::Deserialize;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinSet;

pub(super) const MAX_EXPLICIT_PEERS: usize = 4;
const PEER_TIMEOUT: Duration = Duration::from_secs(8);
const METADATA_PIECE_BYTES: usize = 16 * 1024;
const MAX_MESSAGES: usize = MAX_METADATA_BYTES.div_ceil(METADATA_PIECE_BYTES) * 2 + 32;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const REQUEST_WINDOW: usize = 8;
const EXTENDED_MESSAGE: u8 = 20;
const LOCAL_EXTENSION_ID: u8 = 1;
const PROTOCOL_PREFIX: &[u8] = b"\x13BitTorrent protocol";
const EXTENSION_SUPPORT_FLAG: u8 = 0x10;

#[derive(Deserialize)]
struct ExtensionHandshake {
    m: ExtensionIds,
    metadata_size: usize,
}

#[derive(Deserialize)]
struct ExtensionIds {
    ut_metadata: u8,
}

#[derive(Deserialize)]
struct MetadataHeader {
    msg_type: u8,
    piece: usize,
    total_size: Option<usize>,
}

pub(super) async fn try_peers(peers: Vec<SocketAddr>, hash: &str) -> Option<InspectedRelease> {
    let mut tasks = JoinSet::new();
    for peer in peers {
        let hash = hash.to_owned();
        tasks.spawn(async move { from_peer(peer, &hash).await });
    }
    while let Some(outcome) = tasks.join_next().await {
        if let Ok(Ok(release)) = outcome {
            return Some(release);
        }
    }
    None
}

pub(super) async fn from_peer(address: SocketAddr, hash: &str) -> Result<InspectedRelease, String> {
    let id: Id20 = hash
        .parse()
        .map_err(|_| "Invalid BitTorrent v1 info hash.")?;
    let bytes = tokio::time::timeout(PEER_TIMEOUT, fetch_info(address, id))
        .await
        .map_err(|_| "Torrent metadata peer timed out.")??;
    parse_metainfo(&bytes, Some(hash))
}

async fn fetch_info(address: SocketAddr, hash: Id20) -> Result<Vec<u8>, &'static str> {
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|_| "Could not connect to the torrent metadata peer.")?;
    let mut handshake = [0; 68];
    handshake[..20].copy_from_slice(PROTOCOL_PREFIX);
    handshake[25] = EXTENSION_SUPPORT_FLAG;
    handshake[28..48].copy_from_slice(&hash.0);
    handshake[48..52].copy_from_slice(b"-BT-");
    handshake[52..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    stream
        .write_all(&handshake)
        .await
        .map_err(|_| "Could not open the torrent peer handshake.")?;
    stream
        .read_exact(&mut handshake)
        .await
        .map_err(|_| "Could not read the torrent peer handshake.")?;
    if &handshake[..20] != PROTOCOL_PREFIX
        || handshake[25] & EXTENSION_SUPPORT_FLAG == 0
        || handshake[28..48] != hash.0
    {
        return Err("Torrent peer does not support metadata exchange for this info hash.");
    }
    write_extension(&mut stream, 0, b"d1:md11:ut_metadatai1eee")
        .await
        .map_err(|_| "Could not send the torrent peer extension handshake.")?;

    let mut remaining_messages = MAX_MESSAGES;
    let extension = loop {
        let msg = read_message(&mut stream, &mut remaining_messages)
            .await
            .map_err(|_| "Could not read the torrent peer extension handshake.")?;
        let [EXTENDED_MESSAGE, 0, payload @ ..] = msg.as_slice() else {
            continue;
        };
        if bencode_prefix(payload).is_none_or(|rest| !rest.is_empty()) {
            return Err("Torrent peer extension handshake is malformed.");
        }
        break librqbit_bencode::from_bytes::<ExtensionHandshake>(payload)
            .map_err(|_| "Torrent peer extension handshake does not describe metadata.")?;
    };
    let peer_extension_id = extension.m.ut_metadata;
    if peer_extension_id == 0 || !(1..=MAX_METADATA_BYTES).contains(&extension.metadata_size) {
        return Err("Torrent peer advertised unsupported metadata.");
    }
    let metadata_size = extension.metadata_size;
    let piece_count = metadata_size.div_ceil(METADATA_PIECE_BYTES);
    let mut bytes = vec![0; metadata_size];
    let mut received = vec![false; piece_count];
    let mut next_request = piece_count.min(REQUEST_WINDOW);
    for piece in 0..next_request {
        request_metadata(&mut stream, peer_extension_id, piece)
            .await
            .map_err(|_| "Could not request torrent metadata from the peer.")?;
    }
    let mut completed = 0;
    while completed < piece_count {
        let msg = read_message(&mut stream, &mut remaining_messages)
            .await
            .map_err(|_| "Could not read the torrent metadata response.")?;
        let [EXTENDED_MESSAGE, LOCAL_EXTENSION_ID, payload @ ..] = msg.as_slice() else {
            continue;
        };
        let data =
            bencode_prefix(payload).ok_or("Torrent peer sent a malformed metadata response.")?;
        let header: MetadataHeader =
            librqbit_bencode::from_bytes(&payload[..payload.len() - data.len()])
                .map_err(|_| "Torrent peer metadata response is missing required fields.")?;
        match header.msg_type {
            1 => {}
            2 => return Err("Torrent peer rejected the metadata request."),
            _ => continue,
        }
        let piece = header.piece;
        if header.total_size != Some(metadata_size) || piece >= next_request || received[piece] {
            return Err("Torrent peer sent an unexpected metadata piece.");
        }
        let start = piece * METADATA_PIECE_BYTES;
        let end = (start + METADATA_PIECE_BYTES).min(metadata_size);
        if data.len() != end - start {
            return Err("Torrent peer sent a metadata piece with an unexpected length.");
        }
        bytes[start..end].copy_from_slice(data);
        received[piece] = true;
        completed += 1;
        if next_request < piece_count {
            request_metadata(&mut stream, peer_extension_id, next_request)
                .await
                .map_err(|_| "Could not request torrent metadata from the peer.")?;
            next_request += 1;
        }
    }
    // The peer sends only the info dictionary. Keep its bytes intact for librqbit's hash check.
    let mut torrent = Vec::with_capacity(bytes.len() + 8);
    torrent.extend_from_slice(b"d4:info");
    torrent.extend_from_slice(&bytes);
    torrent.push(b'e');
    Ok(torrent)
}

async fn request_metadata(stream: &mut TcpStream, extension: u8, index: usize) -> Result<(), ()> {
    let payload = format!("d8:msg_typei0e5:piecei{index}ee");
    write_extension(stream, extension, payload.as_bytes()).await
}

async fn write_extension(stream: &mut TcpStream, extension: u8, payload: &[u8]) -> Result<(), ()> {
    let length: u32 = (payload.len() + 2).try_into().map_err(|_| ())?;
    let mut message = Vec::with_capacity(payload.len() + 6);
    message.extend_from_slice(&length.to_be_bytes());
    message.extend_from_slice(&[EXTENDED_MESSAGE, extension]);
    message.extend_from_slice(payload);
    stream.write_all(&message).await.map_err(|_| ())
}

async fn read_message(
    stream: &mut TcpStream,
    remaining_messages: &mut usize,
) -> Result<Vec<u8>, ()> {
    *remaining_messages = remaining_messages.checked_sub(1).ok_or(())?;
    let length = stream.read_u32().await.map_err(|_| ())? as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(());
    }
    let mut message = vec![0; length];
    stream.read_exact(&mut message).await.map_err(|_| ())?;
    Ok(message)
}
