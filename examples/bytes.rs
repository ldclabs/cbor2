use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Packet {
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw = vec![1u8, 2, 3, 4];
    let slice: &[u8] = &raw;

    // Serde's default for Vec<u8> and &[u8] is a sequence of integers.
    assert_eq!(hex::encode(cbor2::to_vec(&raw)?), "8401020304");
    assert_eq!(hex::encode(cbor2::to_vec(&slice)?), "8401020304");

    // Use serde_bytes when the wire type should be a CBOR byte string.
    let owned = serde_bytes::ByteBuf::from(raw.clone());
    let borrowed = serde_bytes::Bytes::new(&raw);
    assert_eq!(hex::encode(cbor2::to_vec(&owned)?), "4401020304");
    assert_eq!(hex::encode(cbor2::to_vec(&borrowed)?), "4401020304");

    let packet = Packet {
        payload: vec![0xde, 0xad, 0xbe, 0xef],
    };
    let encoded = cbor2::to_vec(&packet)?;
    assert_eq!(hex::encode(&encoded), "a1677061796c6f616444deadbeef");

    let decoded: Packet = cbor2::from_slice(&encoded)?;
    assert_eq!(decoded, packet);

    println!("{}", cbor2::diagnostic(&encoded[..])?);
    Ok(())
}
