use std::sync::atomic::AtomicUsize;

use anyhow::bail;
use sha1::{Digest, Sha1};

use crate::{peer::Peer, torrent::Torrent};

pub struct Client<'a> {
    peers: Vec<Peer>,
    file: File<'a>,
    data: Data,
}
pub struct File<'a> {
    file_name: &'a str,
    total_size: usize,
    downloaded: AtomicUsize,
}

pub struct Data {
    piece_count: usize,
    piece_hashes: Vec<String>,
    plength: usize,
}

impl<'a> Client<'a> {
    pub async fn new(torrent: &'a Torrent) -> anyhow::Result<Self> {
        let peer_addrs = torrent.peers().await?;
        let info_hash = torrent.info_hash()?;
        let total_size = torrent.length();

        let mut peers = Vec::new();
        for addr in peer_addrs {
            let peer = Peer::new(addr, &info_hash).await?;
            peers.push(peer);
        }
        let file = File {
            file_name: &torrent.info.name,
            total_size,
            downloaded: AtomicUsize::new(0),
        };
        let data = {
            let hashes = torrent.hashes()?;
            Data {
                piece_count: hashes.len(),
                piece_hashes: hashes,
                plength: torrent.info.plength,
            }
        };
        Ok(Self { peers, file, data })
    }
    pub async fn download_file(&mut self) -> anyhow::Result<Vec<u8>> {
        let piece_count = self.data.piece_count;
        let mut buffer: Vec<u8> = Vec::new();
        for idx in 0..piece_count {
            let plength = if idx == piece_count - 1 {
                self.file.total_size - self.data.plength * (piece_count as u64 - 1) as usize
            } else {
                self.data.plength
            };
            let peer = self
                .peers
                .iter_mut()
                .find(|peer| peer.pieces.contains(&(idx as i32)));
            if let Some(peer) = peer {
                let slice = peer.download_piece(idx, plength).await?;
                let piece_hash = {
                    let mut hasher = Sha1::new();
                    hasher.update(&slice);
                    hex::encode(hasher.finalize())
                };
                assert!(self.data.piece_hashes.contains(&piece_hash));
                buffer.extend(&slice);
            } else {
                bail!("peers don't have this piece :{}", idx);
            }
        }
        Ok(buffer)
    }
}
