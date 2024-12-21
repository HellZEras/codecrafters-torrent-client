use std::net::SocketAddrV4;

use anyhow::Context;
use hashes::Hashes;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::tracker::{TrackerRequest, TrackerResponse};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Torrent {
    pub announce: String,
    pub info: Info,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Info {
    pub name: String,
    #[serde(rename = "piece length")]
    pub plength: usize,
    pub pieces: Hashes,
    #[serde(flatten)]
    pub keys: Keys,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Keys {
    SingleFile { length: usize },
    MultiFile { files: Vec<File> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct File {
    length: usize,
    path: Vec<String>,
}

impl Torrent {
    pub fn info_hash(&self) -> anyhow::Result<[u8; 20]> {
        let info = &self.info;
        let ser = serde_bencode::to_bytes(info)?;
        let mut hasher = Sha1::new();
        hasher.update(&ser);
        let result = hasher
            .finalize()
            .as_slice()
            .try_into()
            .context("SHA1 hash should be 20 bytes long")?;

        Ok(result)
    }
    pub fn hashes(&self) -> anyhow::Result<Vec<String>> {
        let pieces = &self.info.pieces.0;
        Ok(pieces.iter().map(hex::encode).collect())
    }
    pub fn length(&self) -> usize {
        let keys = &self.info.keys;
        match keys {
            Keys::SingleFile { length } => *length,
            Keys::MultiFile { files } => files.iter().map(|file| file.length).sum(),
        }
    }
    pub async fn peers(&self) -> anyhow::Result<Vec<SocketAddrV4>> {
        let info_hash = self.info_hash()?;
        let info_hash = urlencode(&info_hash);

        let data = TrackerRequest {
            peer_id: String::from("66196841112650955225"),
            port: 6681,
            uploaded: 0,
            downloaded: 0,
            left: self.length(),
            compact: 1,
        };
        let url_params = serde_urlencoded::to_string(&data).context("Params")?;
        let url = format!(
            "{}?{}&info_hash={}",
            &self.announce, &url_params, &info_hash
        );
        let response = reqwest::get(url).await.context("Query tracker")?;
        let response = response.bytes().await.context("Fetch tracker response")?;
        let response: TrackerResponse =
            serde_bencode::from_bytes(&response).context("Parsing response")?;

        Ok(response.peers.0)
    }
}

fn urlencode(t: &[u8; 20]) -> String {
    let mut encoded = String::with_capacity(3 * t.len());
    for &byte in t {
        encoded.push('%');
        encoded.push_str(&hex::encode([byte]));
    }
    encoded
}

mod hashes {
    use serde::{de::Visitor, Deserialize, Serialize};
    #[derive(Debug, Clone)]
    pub struct Hashes(pub Vec<[u8; 20]>);

    struct HashesVisitor;

    impl<'de> Visitor<'de> for HashesVisitor {
        type Value = Hashes;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("expecting the array to be of length 20")
        }
        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v.len() % 20 != 0 {
                return Err(E::custom(format!("Length is : {}", v.len())));
            }
            Ok(Hashes(
                v.chunks_exact(20)
                    .map(|chunk| chunk.try_into().expect("Can't happen"))
                    .collect(),
            ))
        }
    }

    impl<'de> Deserialize<'de> for Hashes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_bytes(HashesVisitor)
        }
    }
    impl Serialize for Hashes {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let single_slice = self.0.concat();
            serializer.serialize_bytes(&single_slice)
        }
    }
}
