use peers::Peers;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize)]
pub struct TrackerRequest {
    pub peer_id: String,
    pub port: u16,
    pub uploaded: usize,
    pub downloaded: usize,
    pub left: usize,
    pub compact: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponse {
    pub interval: usize,
    pub peers: Peers,
}

mod peers {
    use std::net::{Ipv4Addr, SocketAddrV4};

    use serde::{de::Visitor, Deserialize, Serialize};
    #[derive(Debug, Clone)]
    pub struct Peers(pub Vec<SocketAddrV4>);

    struct PeersVisitor;

    impl<'de> Visitor<'de> for PeersVisitor {
        type Value = Peers;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter
                .write_str("expecting 6 bytes, the first 4 are the ip and the last 2 are the port")
        }
        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v.len() % 6 != 0 {
                return Err(E::custom(format!("Length is : {}", v.len())));
            }
            Ok(Peers(
                v.chunks_exact(6)
                    .map(|chunk| {
                        SocketAddrV4::new(
                            Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]),
                            u16::from_be_bytes(chunk[4..].try_into().expect("Can't panic")),
                        )
                    })
                    .collect(),
            ))
        }
    }

    impl<'de> Deserialize<'de> for Peers {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_bytes(PeersVisitor)
        }
    }
    impl Serialize for Peers {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut single_slice = Vec::with_capacity(self.0.len() * 6);
            for peer in &self.0 {
                single_slice.extend(peer.ip().octets());
                single_slice.extend(peer.port().to_be_bytes());
            }
            serializer.serialize_bytes(&single_slice)
        }
    }
}
