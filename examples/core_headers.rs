use cbor2::core::{Decoder, Encoder, Header};

fn pull_text<R: std::io::Read>(dec: &mut Decoder<R>) -> Result<String, Box<dyn std::error::Error>> {
    let Header::Text(len) = dec.pull()? else {
        return Err("expected text header".into());
    };

    let mut out = String::new();
    dec.text_body(len, &mut out)?;
    Ok(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    let mut enc = Encoder::from(&mut bytes);

    enc.push(Header::Map(Some(2)))?;
    enc.text("status")?;
    enc.text("ok")?;
    enc.text("data")?;
    enc.push(Header::Bytes(None))?;
    enc.bytes(&[0xde, 0xad])?;
    enc.bytes(&[0xbe, 0xef])?;
    enc.push(Header::Break)?;
    enc.flush()?;

    println!("{}", cbor2::diagnostic(&bytes[..])?);

    let mut dec = Decoder::from(&bytes[..]);
    assert_eq!(dec.pull()?, Header::Map(Some(2)));

    assert_eq!(pull_text(&mut dec)?, "status");
    assert_eq!(pull_text(&mut dec)?, "ok");
    assert_eq!(pull_text(&mut dec)?, "data");

    let Header::Bytes(len) = dec.pull()? else {
        return Err("expected bytes header".into());
    };
    let mut body = Vec::new();
    dec.bytes_body(len, &mut body)?;
    assert_eq!(body, vec![0xde, 0xad, 0xbe, 0xef]);

    Ok(())
}
