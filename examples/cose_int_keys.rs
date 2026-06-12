use serde::{Deserialize, Serialize};

#[cbor2::int_keys]
#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct ProtectedHeader {
    #[cbor(key = 1)]
    alg: i8,

    #[cbor(key = 4)]
    #[serde(with = "serde_bytes")]
    kid: Vec<u8>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let header = ProtectedHeader {
        alg: -7,
        kid: b"kid".to_vec(),
    };

    let bytes = cbor2::to_canonical_vec(&header)?;
    assert_eq!(hex::encode(&bytes), "a2012604436b6964");

    let back: ProtectedHeader = cbor2::from_slice(&bytes)?;
    assert_eq!(back, header);

    println!("{}", cbor2::diagnostic(&bytes[..])?);
    Ok(())
}
