fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = Vec::new();

    for item in [
        cbor2::cbor!({ "type" => "start" })?,
        cbor2::cbor!({ "type" => "chunk", "n" => 1 })?,
        cbor2::cbor!({ "type" => "done" })?,
    ] {
        cbor2::to_writer(&item, &mut stream)?;
    }

    let decoded: Vec<cbor2::Value> = cbor2::de::Deserializer::from_reader(&stream[..])
        .into_iter()
        .collect::<Result<_, _>>()?;

    assert_eq!(decoded.len(), 3);
    assert!(cbor2::validate(&stream[..]).is_err());

    for item in decoded {
        println!("{item}");
    }

    Ok(())
}
