use anyhow::{anyhow, Result};
use rdkafka::message::ToBytes;
use std::error::Error;
use std::fmt::Display;
use std::str::FromStr;

use common::{DevicesInfo, SystemInfo};
use serde::{de, ser::SerializeTuple, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, bincode::Encode, bincode::Decode)]
pub struct ClientId(pub [u8; 16]);

impl ToBytes for ClientId {
    fn to_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl FromStr for ClientId {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim_start_matches("0x");
        let bytes: [u8; 16] = hex::decode(s)?
            .try_into()
            .map_err(|_| anyhow!("Invalid client ID length"))?;
        Ok(ClientId(bytes))
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(&self.0))
    }
}

use sqlx::{
    postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo},
    Encode, Type,
};

impl Type<sqlx::Postgres> for ClientId {
    fn type_info() -> PgTypeInfo {
        <[u8; 16] as Type<sqlx::Postgres>>::type_info()
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        <[u8; 16] as Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl Encode<'_, sqlx::Postgres> for ClientId {
    fn encode_by_ref(
        &self,
        buf: &mut PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, Box<dyn Error + Send + Sync>> {
        //TODO: use ? operator to propagate possible errors
        Ok(<[u8; 16] as Encode<sqlx::Postgres>>::encode(self.0, buf)?)
    }

    fn size_hint(&self) -> usize {
        16 // 16 bytes
    }
}

// if need to support array type, can add this implementation
impl PgHasArrayType for ClientId {
    fn array_type_info() -> PgTypeInfo {
        <[u8; 16] as PgHasArrayType>::array_type_info()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProxyConnId(pub [u8; 16]);

impl FromStr for ProxyConnId {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let bytes: [u8; 16] = hex::decode(s)?
            .try_into()
            .map_err(|_| anyhow!("Invalid client ID length"))?;
        Ok(ProxyConnId(bytes))
    }
}

impl Display for ProxyConnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl serde::Serialize for ClientId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            // For human-readable formats, serialize as a hex string
            serializer.serialize_str(&hex::encode(self.0))
        } else {
            // For binary formats, serialize as a byte array
            let mut seq = serializer.serialize_tuple(16)?;
            for byte in &self.0 {
                seq.serialize_element(byte)?;
            }
            seq.end()
        }
    }
}

impl<'de> serde::Deserialize<'de> for ClientId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            // For human-readable formats, deserialize from a hex string
            let s = String::deserialize(deserializer)?;
            let s = s.trim_start_matches("0x");
            let bytes: [u8; 16] = hex::decode(s)
                .map_err(de::Error::custom)?
                .try_into()
                .map_err(|_| de::Error::custom("Invalid client ID length"))?;
            Ok(ClientId(bytes))
        } else {
            // For binary formats, deserialize from a byte array
            struct ClientIdVisitor;

            impl<'de> de::Visitor<'de> for ClientIdVisitor {
                type Value = [u8; 16];

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a 16-byte array")
                }

                fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                where
                    A: de::SeqAccess<'de>,
                {
                    let mut bytes = [0u8; 16];
                    for i in 0..16 {
                        bytes[i] = seq
                            .next_element()?
                            .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                    }
                    Ok(bytes)
                }
            }

            let bytes = deserializer.deserialize_tuple(16, ClientIdVisitor)?;
            Ok(ClientId(bytes))
        }
    }
}

#[derive(Debug, bincode::Encode, bincode::Decode)]
pub struct HeartbeatMessage {
    // #[serde(deserialize_with = "deserialize_client_id")]
    pub client_id: ClientId,
    pub system_info: SystemInfo,
    pub device_memtotal_gb: u32,
    pub device_count: u32,
    pub total_tflops: u32,
    pub devices_info: Vec<DevicesInfo>,
}

#[allow(dead_code)]
fn deserialize_client_id<'de, D>(deserializer: D) -> Result<ClientId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    ClientId::from_str(&s).map_err(serde::de::Error::custom)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestIDAndClientIDMessage {
    pub client_id: [u8; 16],
    pub request_id: [u8; 16],
}

#[test]
fn test_edit_client() {
    // Use a valid 32-character hex string (16 bytes)
    let client_id = ClientId::from_str("1234567890abcdef1234567890abcdef").unwrap();
    let client_id2 = ClientId::from_str("1234567890abcdef1234567890abcdee").unwrap();
    assert_ne!(client_id, client_id2);
    println!("client_id: {}", client_id);
    println!("client_id2: {}", client_id2);
    // Test that the same hex string produces the same ClientId

    let client_id3 = "0x1234567890abcdef1234567890abcdef"
        .parse::<ClientId>()
        .unwrap();
    println!("client_id3: {:?}", client_id3);
    println!("client_id3: {}", client_id3.to_string());
    assert_eq!(client_id, client_id3);

    //is bad  memory size client_id3_bytes
    let client_id3_bytes: [u8; 32] = "1234567890abcdef1234567890abcdef"
        .as_bytes()
        .try_into()
        .unwrap();
    println!("client_id3_bytes: {:?}", client_id3_bytes);
    assert!(client_id3_bytes.len() > client_id3.0.len());
}
