use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Photo {
    title: String,
    pixels: (u32, u32),
    tags: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let photo = Photo {
        title: "Sunrise".into(),
        pixels: (1920, 1080),
        tags: vec!["morning".into(), "gradient".into()],
    };

    let mut bytes = Vec::new();
    cbor2::to_writer(&photo, &mut bytes)?;
    cbor2::validate(&bytes[..])?;

    let back: Photo = cbor2::from_slice(&bytes)?;
    assert_eq!(photo, back);

    println!("{}", cbor2::diagnostic(&bytes[..])?);
    Ok(())
}
