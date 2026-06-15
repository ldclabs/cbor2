use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Packet {
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Borrowed<'a> {
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow, with = "serde_bytes")]
    body: &'a [u8],
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Signed {
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
    payload: cbor2::RawValue,
}

fn exact_item() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = cbor2::to_vec(&("ok", 7u8))?;
    cbor2::validate(&bytes[..])?;

    let decoded: (String, u8) = cbor2::from_slice(&bytes)?;
    assert_eq!(decoded, ("ok".to_string(), 7));

    let mut with_trailing = bytes.clone();
    with_trailing.push(0);
    assert!(cbor2::validate(&with_trailing[..]).is_err());

    let leading: (String, u8) = cbor2::from_slice(&with_trailing)?;
    assert_eq!(leading, decoded);
    Ok(())
}

fn byte_strings() -> Result<(), Box<dyn std::error::Error>> {
    let raw = vec![1u8, 2, 3, 4];
    assert_eq!(hex::encode(cbor2::to_vec(&raw)?), "8401020304");

    let packet = Packet {
        payload: raw.clone(),
    };
    assert_eq!(
        hex::encode(cbor2::to_vec(&packet)?),
        "a1677061796c6f61644401020304"
    );

    let borrowed = serde_bytes::Bytes::new(&raw);
    assert_eq!(hex::encode(cbor2::to_vec(&borrowed)?), "4401020304");
    Ok(())
}

fn borrowed_from_slice() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = hex::decode("a2646e616d656361706964626f647942cafe")?;
    let value: Borrowed<'_> = cbor2::from_slice(&bytes)?;
    assert_eq!(value.name, "api");
    assert_eq!(value.body, &[0xca, 0xfe]);
    Ok(())
}

fn raw_value_for_signatures() -> Result<(), Box<dyn std::error::Error>> {
    let signed = Signed {
        signature: vec![0xde, 0xad],
        payload: cbor2::RawValue::serialized(&("keep", 1u8))?,
    };

    let bytes = cbor2::to_vec(&signed)?;
    let decoded: Signed = cbor2::from_slice(&bytes)?;
    assert_eq!(decoded.signature, signed.signature);
    assert_eq!(decoded.payload.as_bytes(), signed.payload.as_bytes());

    let payload: (String, u8) = decoded.payload.deserialized()?;
    assert_eq!(payload, ("keep".to_string(), 1));
    Ok(())
}

fn cbor_sequence() -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = Vec::new();
    cbor2::to_writer(&"start", &mut stream)?;
    cbor2::to_writer(&42u8, &mut stream)?;

    let items: Vec<cbor2::Value> = cbor2::de::Deserializer::from_reader(&stream[..])
        .into_iter()
        .collect::<Result<_, _>>()?;

    assert_eq!(
        items,
        vec![cbor2::Value::from("start"), cbor2::Value::from(42)]
    );
    assert!(cbor2::validate(&stream[..]).is_err());
    Ok(())
}

fn canonical_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let map: std::collections::HashMap<&str, u8> = [("z", 1), ("aa", 2), ("b", 3)].into();
    let first = cbor2::to_canonical_vec(&map)?;
    let second = cbor2::to_canonical_vec(&map)?;
    assert_eq!(first, second);
    assert_eq!(hex::encode(&first), "a3616203617a0162616102");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    exact_item()?;
    byte_strings()?;
    borrowed_from_slice()?;
    raw_value_for_signatures()?;
    cbor_sequence()?;
    canonical_bytes()?;

    println!("agent patterns ok");
    Ok(())
}
