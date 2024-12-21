use std::net::SocketAddrV4;

use message::{Message, MessageTag};
use rand::Rng;
use response::{Request, Response};
use tokio::{io::AsyncWriteExt, net::TcpStream};

pub struct HandShake<'a> {
    pub length: u8,
    pub bittorrent: [u8; 19],
    pub reserved: [u8; 8],
    pub info_hash: &'a [u8; 20],
    pub peer_id: &'a [u8; 20],
}

impl<'a> HandShake<'a> {
    pub fn new(info_hash: &'a [u8; 20], peer_id: &'a [u8; 20]) -> Self {
        Self {
            length: 19,
            bittorrent: *b"BitTorrent protocol",
            reserved: [0; 8],
            info_hash,
            peer_id,
        }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(49 + 20 + 20);
        buffer.push(self.length);
        buffer.extend_from_slice(&self.bittorrent);
        buffer.extend_from_slice(&self.reserved);
        buffer.extend_from_slice(self.info_hash);
        buffer.extend_from_slice(self.peer_id);
        buffer
    }
}

#[derive(Debug)]
pub struct Peer {
    pub addr: SocketAddrV4,
    pub stream: TcpStream,
    pub sent_interested: bool,
    pub pieces: Vec<i32>,
}

impl Peer {
    pub async fn new(addr: SocketAddrV4, info_hash: &[u8; 20]) -> anyhow::Result<Peer> {
        let mut stream = TcpStream::connect(addr).await?;
        let mut rng = rand::thread_rng();
        let peer_id: [u8; 20] = rng.gen();
        let handshake = HandShake::new(info_hash, &peer_id);
        stream.write_all(&handshake.to_bytes()).await?;

        // Decode only the Bitfield message
        let message = Message::decode(&mut stream, MessageTag::Bitfield).await?;
        let mut pieces = Vec::new();
        let mut piece_count = 0;
        for chunk in message.payload {
            let bin = format!("{:b}", chunk);
            for c in bin.chars() {
                if c == '1' {
                    pieces.push(piece_count);
                }
                piece_count += 1;
            }
        }
        Ok(Self {
            addr,
            stream,
            sent_interested: false,
            pieces,
        })
    }

    pub async fn download_piece(
        &mut self,
        piece_idx: usize,
        plength: usize,
    ) -> anyhow::Result<Vec<u8>> {
        const BLOCK_SIZE: usize = 1 << 14;
        let mut stream = &mut self.stream;
        let mut downloaded_piece = vec![0u8; plength];
        let mut bytes_downloaded = 0;

        if !self.sent_interested {
            Message::encode(&mut stream, MessageTag::Interested, &[]).await?;
            Message::decode(&mut stream, MessageTag::Unchoke).await?;
            self.sent_interested = true;
        }

        while bytes_downloaded < plength {
            let block_offset = bytes_downloaded;
            let block_length = (plength - bytes_downloaded).min(BLOCK_SIZE);

            // Create a request for the next block
            let request = Request::new(piece_idx as u32, block_offset as u32, block_length as u32);
            let payload = request.encode();

            Message::encode(&mut stream, MessageTag::Request, &payload).await?;

            let message = Message::decode(&mut stream, MessageTag::Piece).await?;
            let response = Response::decode(&message)?;
            let data = response.data;
            if response.idx as usize == piece_idx && response.offset as usize == block_offset {
                downloaded_piece[block_offset..block_offset + data.len()].copy_from_slice(&data);
                bytes_downloaded += data.len();
            }
        }

        Ok(downloaded_piece)
    }
}

pub mod message {
    use std::time::Duration;

    use anyhow::bail;
    use tokio::{
        io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
        time::Instant,
    };

    #[derive(Debug, PartialEq, Eq)]
    pub enum MessageTag {
        Choke = 0,
        Unchoke = 1,
        Interested = 2,
        NotInterested = 3,
        Have = 4,
        Bitfield = 5,
        Request = 6,
        Piece = 7,
        Cancel = 8,
    }
    impl MessageTag {
        pub fn from(idx: usize) -> anyhow::Result<Self> {
            match idx {
                0 => Ok(Self::Choke),
                1 => Ok(Self::Unchoke),
                2 => Ok(Self::Interested),
                3 => Ok(Self::NotInterested),
                4 => Ok(Self::Have),
                5 => Ok(Self::Bitfield),
                6 => Ok(Self::Request),
                7 => Ok(Self::Piece),
                8 => Ok(Self::Cancel),
                _ => anyhow::bail!("Not available"),
            }
        }
    }
    #[derive(Debug)]
    pub struct Message {
        pub tag: MessageTag,
        pub payload: Vec<u8>,
    }
    impl Message {
        pub async fn encode<W>(w: &mut W, tag: MessageTag, payload: &[u8]) -> anyhow::Result<()>
        where
            W: AsyncWrite + Unpin,
        {
            let len_buf = (payload.len() + 1) as u32;

            w.write_u32(len_buf).await?;

            w.write_u8(tag as u8).await?;

            w.write_all(payload).await?;

            Ok(())
        }
        pub async fn decode<R>(stream: &mut R, tag: MessageTag) -> anyhow::Result<Self>
        where
            R: AsyncRead + Unpin,
        {
            let tick = Instant::now();
            loop {
                if tick.elapsed() > Duration::from_secs(5) {
                    break;
                }
                let length = stream.read_u32().await?;
                if length == 0 || length > 18000 {
                    continue;
                }

                let mut buffer = vec![0u8; length as usize];
                stream.read_exact(&mut buffer).await?;
                if let Ok(tag) = MessageTag::from(buffer[0].into()) {
                    let payload = buffer[1..].to_vec();
                    return Ok(Self { tag, payload });
                }
            }
            bail!("Failed to receive message of tag : {:?}", tag)
        }
    }
}
pub mod response {
    use super::message::Message;

    pub struct Request {
        piece_idx: u32,
        block_offset: u32,
        block_length: u32,
    }
    impl Request {
        pub fn new(piece_idx: u32, block_offset: u32, block_length: u32) -> Self {
            Self {
                piece_idx,
                block_offset,
                block_length,
            }
        }
        pub fn encode(&self) -> Vec<u8> {
            let mut buffer = Vec::with_capacity(12);
            buffer.extend_from_slice(&(self.piece_idx).to_be_bytes());
            buffer.extend_from_slice(&(self.block_offset).to_be_bytes());
            buffer.extend_from_slice(&(self.block_length).to_be_bytes());
            buffer
        }
    }
    pub struct Response {
        pub idx: u32,
        pub offset: u32,
        pub data: Vec<u8>,
    }
    impl Response {
        pub fn decode(message: &Message) -> anyhow::Result<Self> {
            let idx = u32::from_be_bytes(message.payload[0..4].try_into()?);
            let offset = u32::from_be_bytes(message.payload[4..8].try_into()?);
            let data = message.payload[8..].to_vec();
            Ok(Self { idx, offset, data })
        }
    }
}
