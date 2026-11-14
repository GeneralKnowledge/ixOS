//! What two Relay devices say to each other, and the pairing code.
//!
//! A connection goes: the dialer sends an [`Intro`] in clear (who it says it
//! is, and whether it is pairing), both run the Noise `XXpsk3` handshake
//! from `forge_devproto` keyed by the pair's secret, they swap [`Msg::Hello`]
//! and then the dialer offers items one at a time. A wrong secret fails the
//! handshake, so nothing after the intro is readable without it.

use serde::{Deserialize, Serialize};

use crate::model::RelayError;

/// Identifies the intro, so reaching some other service says so.
pub const MAGIC: &str = "forge-relay";
/// Bump on any change an older peer would misread.
pub const PROTOCOL: u16 = 1;
/// Bytes of a file in one chunk: well inside a Noise message.
pub const CHUNK: usize = 32 * 1024;
/// The pairing code's scheme. Also the QR code's text.
const SCHEME: &str = "forgerelay://pair";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Keyed by the one-time secret in an invitation.
    Pair,
    /// Keyed by the secret stored when the two paired.
    Session,
}

/// The one cleartext message: which secret the listener should use.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Intro {
    pub magic: String,
    pub protocol: u16,
    pub from: String,
    pub purpose: Purpose,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Offered {
    Note {
        text: String,
    },
    File {
        name: String,
        size: u64,
        sha256: String,
    },
}

/// Everything after the handshake, encrypted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Hello {
        device_id: String,
        name: String,
    },
    Offer {
        item: Offered,
    },
    Accept,
    Decline {
        reason: String,
    },
    Chunk {
        #[serde(with = "serde_bytes_vec")]
        data: Vec<u8>,
    },
    /// The whole file has been sent.
    Done,
    /// The receiver checked and kept it (`ok`), or didn't.
    Received {
        ok: bool,
        reason: Option<String>,
    },
    Bye,
}

/// CBOR byte strings rather than arrays of numbers for chunks.
mod serde_bytes_vec {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(data)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Vec<u8>, A::Error> {
                let mut out = Vec::new();
                while let Some(b) = seq.next_element()? {
                    out.push(b);
                }
                Ok(out)
            }
        }
        d.deserialize_byte_buf(V)
    }
}

/// What a pairing code carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub device_id: String,
    pub name: String,
    pub secret: [u8; 32],
    /// Where the inviting device listens ("host:port"), best first.
    pub addresses: Vec<String>,
}

impl Code {
    /// `forgerelay://pair?v=1&id=…&name=…&key=…&at=…`
    pub fn encode(&self) -> String {
        format!(
            "{SCHEME}?v={PROTOCOL}&id={}&name={}&key={}&at={}",
            self.device_id,
            percent_encode(&self.name),
            hex(&self.secret),
            percent_encode(&self.addresses.join(",")),
        )
    }

    pub fn parse(code: &str) -> Result<Self, RelayError> {
        let query = code
            .trim()
            .strip_prefix(SCHEME)
            .and_then(|rest| rest.strip_prefix('?'))
            .ok_or(RelayError::BadCode)?;
        let mut version = None;
        let (mut id, mut name, mut key, mut at) = (None, None, None, None);
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').ok_or(RelayError::BadCode)?;
            let v = percent_decode(v).ok_or(RelayError::BadCode)?;
            match k {
                "v" => version = v.parse::<u16>().ok(),
                "id" => id = Some(v),
                "name" => name = Some(v),
                "key" => key = Some(v),
                "at" => at = Some(v),
                _ => {} // Added by a later version; ignored.
            }
        }
        if version != Some(PROTOCOL) {
            return Err(RelayError::BadCode);
        }
        let device_id = id
            .filter(|id| is_device_id(id))
            .ok_or(RelayError::BadCode)?;
        let secret: [u8; 32] = key
            .and_then(|k| unhex(&k))
            .and_then(|k| k.try_into().ok())
            .ok_or(RelayError::BadCode)?;
        let addresses: Vec<String> = at
            .unwrap_or_default()
            .split(',')
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect();
        if addresses.is_empty() {
            return Err(RelayError::BadCode);
        }
        Ok(Self {
            device_id,
            name: name.unwrap_or_default(),
            secret,
            addresses,
        })
    }
}

pub fn is_device_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let byte = u8::from_str_radix(text.get(i + 1..i + 3)?, 16).ok()?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code() -> Code {
        Code {
            device_id: "0123456789abcdef0123456789abcdef".into(),
            name: "Reuben's iPhone & co".into(),
            secret: [7; 32],
            addresses: vec!["192.168.1.20:47900".into(), "[fe80::1]:47900".into()],
        }
    }

    #[test]
    fn codes_round_trip() {
        let text = code().encode();
        assert!(text.starts_with("forgerelay://pair?v=1&"));
        assert!(!text.contains(' '));
        assert_eq!(Code::parse(&text).unwrap(), code());
        // Later versions may add fields.
        assert_eq!(Code::parse(&format!("{text}&new=1")).unwrap(), code());
    }

    #[test]
    fn bad_codes_are_refused() {
        let good = code().encode();
        for bad in [
            "https://example.com".to_string(),
            good.replace("v=1", "v=2"),
            good.replace("key=07", "key=zz"),
            good.replace("id=0123", "id=XY23"),
            good.replace("&at=", "&gone="),
        ] {
            assert_eq!(Code::parse(&bad), Err(RelayError::BadCode), "{bad}");
        }
    }

    #[test]
    fn chunks_are_cbor_byte_strings() {
        let msg = Msg::Chunk {
            data: vec![0xAB; 1000],
        };
        let bytes = forge_devproto::encode(&msg);
        assert!(bytes.len() < 1020, "{}", bytes.len());
        assert_eq!(forge_devproto::decode::<Msg>(&bytes).unwrap(), msg);
    }
}
