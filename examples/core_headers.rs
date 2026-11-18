use cbor2::core::{Decoder, Encoder, Header};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = [0; 32];
    let mut remaining = buffer.as_mut_slice();
    let mut enc = Encoder::from(&mut remaining);

    enc.push(Header::Map(Some(2)))?;
    enc.text("status")?;
    enc.text("ok")?;
    enc.text("data")?;
    enc.push(Header::Bytes(None))?;
    enc.bytes(&[0xde, 0xad])?;
    enc.bytes(&[0xbe, 0xef])?;
    enc.push(Header::Break)?;
    enc.flush()?;

    let unused = remaining.len();
    let bytes = &buffer[..buffer.len() - unused];
    #[cfg(feature = "alloc")]
    println!("{}", cbor2::diagnostic(bytes)?);

    let mut dec = Decoder::from(bytes);
    assert_eq!(dec.pull()?, Header::Map(Some(2)));
    for expected in ["status", "ok", "data"] {
        assert_eq!(dec.pull()?, Header::Text(Some(expected.len())));
        let mut text = [0; 6];
        dec.read_exact(&mut text[..expected.len()])?;
        assert_eq!(&text[..expected.len()], expected.as_bytes());
    }

    assert_eq!(dec.pull()?, Header::Bytes(None));
    let mut body = [0; 4];
    for chunk in body.chunks_mut(2) {
        assert_eq!(dec.pull()?, Header::Bytes(Some(chunk.len())));
        dec.read_exact(chunk)?;
    }
    assert_eq!(dec.pull()?, Header::Break);
    assert_eq!(body, [0xde, 0xad, 0xbe, 0xef]);
    Ok(())
}
