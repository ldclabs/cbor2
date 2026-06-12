//! Exhaustive tests for `Value` accessors, conversions and `Integer`.

use cbor2::value::Integer;
use cbor2::Value;

#[test]
fn accessors_match_their_variant() {
    let mut v = Value::Integer(7.into());
    assert!(v.is_integer());
    assert_eq!(v.as_integer(), Some(7.into()));
    assert_eq!(v.clone().into_integer(), Ok(7.into()));
    assert!(!v.is_bytes() && !v.is_float() && !v.is_text());
    assert!(!v.is_bool() && !v.is_null() && !v.is_tag());
    assert!(!v.is_array() && !v.is_map());
    assert!(v.as_bytes().is_none() && v.as_bytes_mut().is_none());
    assert!(v.as_float().is_none() && v.as_text().is_none());
    assert!(v.as_text_mut().is_none() && v.as_bool().is_none());
    assert!(v.as_tag().is_none() && v.as_tag_mut().is_none());
    assert!(v.as_array().is_none() && v.as_array_mut().is_none());
    assert!(v.as_map().is_none() && v.as_map_mut().is_none());

    let mut v = Value::Bytes(vec![1]);
    assert!(v.is_bytes());
    assert_eq!(v.as_bytes(), Some(&vec![1]));
    v.as_bytes_mut().unwrap().push(2);
    assert_eq!(v.clone().into_bytes(), Ok(vec![1, 2]));
    assert!(v.as_integer().is_none() && !v.is_integer());

    let v = Value::Float(1.5);
    assert!(v.is_float());
    assert_eq!(v.as_float(), Some(1.5));
    assert_eq!(v.clone().into_float(), Ok(1.5));

    let mut v = Value::Text("a".into());
    assert!(v.is_text());
    assert_eq!(v.as_text(), Some("a"));
    v.as_text_mut().unwrap().push('b');
    assert_eq!(v.clone().into_text(), Ok("ab".into()));

    let v = Value::Bool(true);
    assert!(v.is_bool());
    assert_eq!(v.as_bool(), Some(true));
    assert_eq!(v.clone().into_bool(), Ok(true));

    assert!(Value::Null.is_null());
    assert!(!Value::Bool(false).is_null());

    let mut v = Value::Tag(7, Box::new(Value::Null));
    assert!(v.is_tag());
    assert_eq!(v.as_tag(), Some((7, &Value::Null)));
    {
        let (tag, inner) = v.as_tag_mut().unwrap();
        *tag = 8;
        *inner = Value::Bool(true);
    }
    assert_eq!(v.clone().into_tag(), Ok((8, Box::new(Value::Bool(true)))));

    let mut v = Value::Array(vec![Value::Null]);
    assert!(v.is_array());
    assert_eq!(v.as_array().unwrap().len(), 1);
    v.as_array_mut().unwrap().clear();
    assert_eq!(v.clone().into_array(), Ok(vec![]));

    let mut v = Value::Map(vec![(Value::Null, Value::Null)]);
    assert!(v.is_map());
    assert_eq!(v.as_map().unwrap().len(), 1);
    v.as_map_mut().unwrap().clear();
    assert_eq!(v.clone().into_map(), Ok(vec![]));
}

#[test]
fn into_conversions_return_self_on_mismatch() {
    let v = Value::Null;
    assert_eq!(v.clone().into_integer(), Err(Value::Null));
    assert_eq!(v.clone().into_bytes(), Err(Value::Null));
    assert_eq!(v.clone().into_float(), Err(Value::Null));
    assert_eq!(v.clone().into_text(), Err(Value::Null));
    assert_eq!(v.clone().into_bool(), Err(Value::Null));
    assert_eq!(v.clone().into_tag(), Err(Value::Null));
    assert_eq!(v.clone().into_array(), Err(Value::Null));
    assert_eq!(v.into_map(), Err(Value::Null));
}

#[test]
fn from_impls() {
    assert_eq!(Value::from(Integer::from(3)), Value::Integer(3.into()));
    assert_eq!(Value::from(3u8), Value::Integer(3.into()));
    assert_eq!(Value::from(3u16), Value::Integer(3.into()));
    assert_eq!(Value::from(3u32), Value::Integer(3.into()));
    assert_eq!(Value::from(3u64), Value::Integer(3.into()));
    assert_eq!(Value::from(-3i8), Value::Integer((-3).into()));
    assert_eq!(Value::from(-3i16), Value::Integer((-3).into()));
    assert_eq!(Value::from(-3i32), Value::Integer((-3).into()));
    assert_eq!(Value::from(-3i64), Value::Integer((-3).into()));

    assert_eq!(Value::from(vec![1u8]), Value::Bytes(vec![1]));
    assert_eq!(Value::from(&b"ab"[..]), Value::Bytes(b"ab".to_vec()));

    assert_eq!(Value::from(0.5f64), Value::Float(0.5));
    assert_eq!(Value::from(0.5f32), Value::Float(0.5));

    assert_eq!(Value::from(String::from("x")), Value::Text("x".into()));
    assert_eq!(Value::from("x"), Value::Text("x".into()));
    assert_eq!(Value::from('水'), Value::Text("水".into()));

    assert_eq!(Value::from(true), Value::Bool(true));

    let items = [Value::Null];
    assert_eq!(Value::from(&items[..]), Value::Array(vec![Value::Null]));
    assert_eq!(
        Value::from(vec![Value::Null]),
        Value::Array(vec![Value::Null])
    );

    let pairs = [(Value::Null, Value::Null)];
    assert_eq!(
        Value::from(&pairs[..]),
        Value::Map(vec![(Value::Null, Value::Null)])
    );
    assert_eq!(
        Value::from(pairs.to_vec()),
        Value::Map(vec![(Value::Null, Value::Null)])
    );
}

#[test]
fn from_128_bit_boundaries() {
    // In range: plain integers.
    assert_eq!(
        Value::from(u64::MAX as u128),
        Value::Integer(u64::MAX.into())
    );
    assert_eq!(
        Value::from(-(u64::MAX as i128) - 1),
        Value::Integer(Integer::try_from(-(u64::MAX as i128) - 1).unwrap())
    );

    // Out of range: bignum tags with stripped payloads.
    let v = Value::from(u64::MAX as u128 + 1);
    assert_eq!(v.as_tag().unwrap().0, 2);
    assert_eq!(v.as_tag().unwrap().1.as_bytes().unwrap().len(), 9);

    let v = Value::from(-(u64::MAX as i128) - 2);
    assert_eq!(v.as_tag().unwrap().0, 3);
    assert_eq!(v.as_tag().unwrap().1.as_bytes().unwrap().len(), 9);
}

#[test]
fn integer_conversions() {
    // From all primitive widths...
    assert_eq!(i128::from(Integer::from(3u8)), 3);
    assert_eq!(i128::from(Integer::from(3u16)), 3);
    assert_eq!(i128::from(Integer::from(3u32)), 3);
    assert_eq!(i128::from(Integer::from(3u64)), 3);
    assert_eq!(i128::from(Integer::from(-3i8)), -3);
    assert_eq!(i128::from(Integer::from(-3i16)), -3);
    assert_eq!(i128::from(Integer::from(-3i32)), -3);
    assert_eq!(i128::from(Integer::from(-3i64)), -3);
    assert_eq!(i128::from(Integer::from(3usize)), 3);
    assert_eq!(i128::from(Integer::from(-3isize)), -3);

    // ...and back, with range checks.
    let big = Integer::from(u64::MAX);
    assert_eq!(u64::try_from(big), Ok(u64::MAX));
    assert!(u8::try_from(big).is_err());
    assert!(u16::try_from(big).is_err());
    assert!(u32::try_from(big).is_err());
    assert!(i8::try_from(big).is_err());
    assert!(i16::try_from(big).is_err());
    assert!(i32::try_from(big).is_err());
    assert!(i64::try_from(big).is_err());
    assert!(usize::try_from(Integer::from(1u8)).is_ok());
    assert!(isize::try_from(Integer::from(1u8)).is_ok());

    assert_eq!(u8::try_from(Integer::from(7u8)), Ok(7));
    assert_eq!(u16::try_from(Integer::from(7u8)), Ok(7));
    assert_eq!(u32::try_from(Integer::from(7u8)), Ok(7));
    assert_eq!(i8::try_from(Integer::from(-7i8)), Ok(-7));
    assert_eq!(i16::try_from(Integer::from(-7i8)), Ok(-7));
    assert_eq!(i32::try_from(Integer::from(-7i8)), Ok(-7));
    assert_eq!(i64::try_from(Integer::from(-7i8)), Ok(-7));

    // 128-bit constructors enforce the CBOR integer range.
    assert!(Integer::try_from(-2i128.pow(64)).is_ok());
    assert!(Integer::try_from(-2i128.pow(64) - 1).is_err());
    assert!(Integer::try_from(2u128.pow(64) - 1).is_ok());
    assert!(Integer::try_from(2u128.pow(64)).is_err());
    assert!(Integer::try_from(2i128.pow(64)).is_err());

    // 128-bit extractors.
    assert_eq!(
        u128::try_from(Integer::from(u64::MAX)),
        Ok(u64::MAX as u128)
    );
    assert!(u128::try_from(Integer::from(-1i8)).is_err());
    assert_eq!(
        i128::from(Integer::try_from(-2i128.pow(64)).unwrap()),
        -2i128.pow(64)
    );
}

#[test]
fn std_collection_conversions() {
    use std::collections::{BTreeMap, HashMap};

    // HashMap/BTreeMap -> Value (entry order follows the map's iteration
    // order; sort for a stable assertion).
    let map: HashMap<&str, u64> = [("b", 2), ("a", 1)].into();
    let mut value = Value::from(map);
    value
        .as_map_mut()
        .unwrap()
        .sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    assert_eq!(value, cbor2::cbor!({ "a": 1, "b": 2 }).unwrap());

    let map: BTreeMap<Integer, Vec<u8>> = [(Integer::from(1), b"kid".to_vec())].into();
    assert_eq!(
        Value::from(map),
        cbor2::cbor!({ 1: Value::Bytes(b"kid".to_vec()) }).unwrap()
    );

    // Value -> HashMap/BTreeMap, with typed or Value elements.
    let value = cbor2::cbor!({ "a": 1, "b": 2 }).unwrap();
    let typed: HashMap<String, u64> = value.clone().try_into().unwrap();
    assert_eq!(typed["a"], 1);
    let dynamic: HashMap<String, Value> = value.clone().try_into().unwrap();
    assert_eq!(dynamic["b"], Value::from(2));
    let sorted: BTreeMap<String, u64> = value.clone().try_into().unwrap();
    assert_eq!(sorted.keys().collect::<Vec<_>>(), ["a", "b"]);

    // COSE-style integer keys.
    let header = cbor2::cbor!({ 1: -7, 4: Value::Bytes(b"kid".to_vec()) }).unwrap();
    let map: BTreeMap<Integer, Value> = header.try_into().unwrap();
    assert_eq!(map[&Integer::from(1)], Value::from(-7));

    // Failures carry serde-style messages.
    let msg = HashMap::<String, u64>::try_from(Value::from(1))
        .unwrap_err()
        .to_string();
    assert!(msg.contains("invalid type"), "{msg}");
    let msg = HashMap::<String, u64>::try_from(cbor2::cbor!({ 1: 1 }).unwrap())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("invalid map key"), "{msg}");
    let msg = HashMap::<String, u64>::try_from(cbor2::cbor!({ "a": -1 }).unwrap())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("invalid map value"), "{msg}");

    // Later duplicate keys overwrite earlier ones, as on the wire.
    let dup = Value::Map(vec![
        (Value::from("a"), Value::from(1)),
        (Value::from("a"), Value::from(2)),
    ]);
    let map: HashMap<String, u64> = dup.try_into().unwrap();
    assert_eq!(map["a"], 2);
}

#[test]
fn std_scalar_conversions() {
    // From: Option, byte arrays, usize/isize, Cow, iterators.
    assert_eq!(Value::from(Some(7)), Value::from(7));
    assert_eq!(Value::from(None::<i32>), Value::Null);
    assert_eq!(Value::from(*b"ab"), Value::Bytes(vec![0x61, 0x62]));
    assert_eq!(Value::from(b"ab"), Value::Bytes(vec![0x61, 0x62]));
    assert_eq!(Value::from(7usize), Value::from(7));
    assert_eq!(Value::from(-7isize), Value::from(-7));
    assert_eq!(Value::from(std::borrow::Cow::from("hi")), Value::from("hi"));
    assert_eq!((1..=2).collect::<Value>(), cbor2::cbor!([1, 2]).unwrap());

    // TryFrom: each variant moves out...
    assert_eq!(String::try_from(Value::from("hi")).unwrap(), "hi");
    assert_eq!(Vec::<u8>::try_from(Value::from(b"ab")).unwrap(), b"ab");
    assert!(bool::try_from(Value::from(true)).unwrap());
    assert_eq!(f64::try_from(Value::from(1.5)).unwrap(), 1.5);
    assert_eq!(char::try_from(Value::from('x')).unwrap(), 'x');
    assert_eq!(Integer::try_from(Value::from(7)).unwrap(), Integer::from(7));
    assert_eq!(
        Vec::<Value>::try_from(cbor2::cbor!([1]).unwrap()).unwrap(),
        vec![Value::from(1)]
    );
    assert_eq!(
        Vec::<(Value, Value)>::try_from(cbor2::cbor!({ "a": 1 }).unwrap()).unwrap(),
        vec![(Value::from("a"), Value::from(1))]
    );

    // ... integers are range-checked ...
    assert_eq!(u8::try_from(Value::from(255)).unwrap(), 255);
    assert_eq!(i64::try_from(Value::from(-1)).unwrap(), -1);
    assert_eq!(usize::try_from(Value::from(7)).unwrap(), 7);
    let msg = u8::try_from(Value::from(256)).unwrap_err().to_string();
    assert!(msg.contains("invalid value: integer `256`"), "{msg}");
    let msg = u64::try_from(Value::from("x")).unwrap_err().to_string();
    assert!(msg.contains("invalid type"), "{msg}");

    // ... and the 128-bit forms round-trip through bignum tags.
    assert_eq!(u128::try_from(Value::from(u128::MAX)).unwrap(), u128::MAX);
    assert_eq!(i128::try_from(Value::from(i128::MIN)).unwrap(), i128::MIN);

    // Type mismatches keep serde-style messages.
    let msg = String::try_from(Value::from(1)).unwrap_err().to_string();
    assert!(msg.contains("invalid type"), "{msg}");
    let msg = char::try_from(Value::from("ab")).unwrap_err().to_string();
    assert!(msg.contains("single-character"), "{msg}");
}
