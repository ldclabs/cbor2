use std::collections::BTreeMap;

#[derive(Debug, PartialEq, wire::Cbor)]
#[cbor(tag = 42)]
struct Header<T> {
    #[cbor(key = 1)]
    id: T,
    #[serde(flatten)]
    extra: BTreeMap<String, wire::Value>,
}

fn main() {
    let value = Header {
        id: 7u8,
        extra: [("time".into(), wire::Value::Tag(1, Box::new(7.into())))].into(),
    };
    let bytes = wire::to_vec(&value).unwrap();
    assert_eq!(wire::from_slice::<Header<u8>>(&bytes).unwrap(), value);
}
