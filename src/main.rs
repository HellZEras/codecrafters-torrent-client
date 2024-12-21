use std::{fs::File, io::Write};

use client::Client;
use torrent::Torrent;
mod client;
mod peer;
mod torrent;
mod tracker;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let buff = std::fs::read("sample.torrent")?;
    let torrent: Torrent = serde_bencode::from_bytes(&buff)?;
    let mut client = Client::new(&torrent).await?;
    let buffer = client.download_file().await?;
    let mut file = File::create(torrent.info.name)?;
    file.write_all(&buffer)?;
    Ok(())
}
