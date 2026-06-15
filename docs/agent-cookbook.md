# cbor2 Agent Cookbook

This cookbook is written for code agents. Each recipe starts with the intent,
then gives the correct `cbor2` shape and the mistake to avoid.

## Pick the Right API

| Intent | Correct call |
| --- | --- |
| Serialize to bytes | `cbor2::to_vec(&value)` |
| Serialize to a writer | `cbor2::to_writer(&value, writer)` |
| Deserialize from bytes | `cbor2::from_slice::<T>(bytes)` |
| Deserialize from `Read` | `cbor2::from_reader::<T, _>(reader)` |
| Validate exactly one item | `cbor2::validate(bytes)` |
| Read adjacent CBOR items | `cbor2::de::Deserializer::from_reader(reader).into_iter()` |
| Preserve one raw item | `cbor2::RawValue` |
| Work with unknown data | `cbor2::Value` or `cbor2::cbor!` |
| Produce deterministic bytes | `cbor2::to_canonical_vec(&value)` |
| Read/write one typed value async | `cbor2::async_io::read_value` / `write_value` |
| Declare COSE-like structs | `#[derive(cbor2::Cbor)]` (feature `derive`) |

## Decode a Buffer That Must Contain One Item

Use `validate` as the exact-item gate. `from_slice` decodes the first item and
does not enforce exhaustion.

```rust
let bytes = cbor2::to_vec(&("ok", 7u8)).unwrap();
cbor2::validate(&bytes[..]).unwrap();

let value: (String, u8) = cbor2::from_slice(&bytes).unwrap();
assert_eq!(value, ("ok".to_string(), 7));
```

Common mistake:

```rust
// Wrong if trailing bytes must be rejected:
let _value: cbor2::Value = cbor2::from_slice(bytes).unwrap();
```

## Decode a CBOR Sequence

CBOR sequences are adjacent complete items. Use the deserializer iterator.

```rust
let mut stream = Vec::new();
cbor2::to_writer(&"start", &mut stream).unwrap();
cbor2::to_writer(&42u8, &mut stream).unwrap();

let items: Vec<cbor2::Value> = cbor2::de::Deserializer::from_reader(&stream[..])
    .into_iter()
    .collect::<Result<_, _>>()
    .unwrap();
assert_eq!(items.len(), 2);
assert!(cbor2::validate(&stream[..]).is_err());
```

## Borrow Text and Bytes From Input

Borrowing only works from `from_slice`, and only for definite-length text or
byte strings.

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct Borrowed<'a> {
    #[serde(borrow)]
    name: &'a str,
    #[serde(borrow, with = "serde_bytes")]
    body: &'a [u8],
}

let bytes = hex::decode("a2646e616d656361706964626f647942cafe").unwrap();
let value: Borrowed<'_> = cbor2::from_slice(&bytes).unwrap();
assert_eq!(value.name, "api");
assert_eq!(value.body, &[0xca, 0xfe]);
```

Common mistake:

```rust
// Wrong for borrowed output:
// let value: Borrowed<'_> = cbor2::from_reader(&bytes[..]).unwrap();
```

## Encode CBOR Byte Strings

Serde treats `Vec<u8>` and `&[u8]` as sequences. Use `serde_bytes` for CBOR
major type 2 byte strings.

```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Packet {
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
}

let packet = Packet { payload: vec![1, 2, 3, 4] };
assert_eq!(hex::encode(cbor2::to_vec(&packet).unwrap()), "a1677061796c6f61644401020304");
```

Common mistake:

```rust
let raw = vec![1u8, 2, 3, 4];
assert_eq!(hex::encode(cbor2::to_vec(&raw).unwrap()), "8401020304"); // array
```

## Preserve Signature Payload Bytes

Use `RawValue` when signatures, hashes, pass-through, or deferred decoding need
the original encoded bytes.

```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Signed {
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
    payload: cbor2::RawValue,
}

let signed = Signed {
    signature: vec![0xde, 0xad],
    payload: cbor2::RawValue::serialized(&("keep", 1u8)).unwrap(),
};

let bytes = cbor2::to_vec(&signed).unwrap();
let decoded: Signed = cbor2::from_slice(&bytes).unwrap();
assert_eq!(decoded.payload.as_bytes(), signed.payload.as_bytes());
```

Common mistake: decoding a signed payload into a typed struct and then
re-encoding it before verification. That can change map order, integer width,
or definite/indefinite structure.

## Generate Deterministic Bytes

Use canonical encoding for signatures, hashes, reproducible fixtures, and
protocols that require stable map ordering.

```rust
use std::collections::HashMap;

let map: HashMap<&str, u8> = [("z", 1), ("aa", 2), ("b", 3)].into();
let a = cbor2::to_canonical_vec(&map).unwrap();
let b = cbor2::to_canonical_vec(&map).unwrap();
assert_eq!(a, b);
```

Use `to_canonical_vec_with(..., cbor2::KeyOrder::LengthFirst)` only for
protocols that explicitly use the older RFC 7049 length-first key order.

## Declare COSE-Style Wire Shapes

Enable the `derive` feature and derive `cbor2::Cbor`, not serde's derives.

```rust
use cbor2::Cbor;

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 18, array)]
struct Sign1 {
    #[serde(with = "serde_bytes")]
    protected: Vec<u8>,
    unprotected: u8,
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
}
```

Common mistakes:

- Do not also write `#[derive(Serialize, Deserialize)]`; `Cbor` generates
  those impls.
- Do not use `#[serde(rename = "1")]` for integer keys; that creates a text
  key. Use `#[cbor(key = 1)]`.
- Do not combine `#[cbor(array)]` with per-field integer keys.

## Use Async Transports

Serde itself is synchronous. `async_io::read_value`/`write_value` frame one
complete CBOR item on an async stream and (de)serialize it in a single call:

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<cbor2::Value, cbor2::de::Error> {
let value: cbor2::Value = cbor2::async_io::read_value(reader).await?;
# Ok(value)
# }
```

Drop to `read_item` (then `from_slice`) only when the caller must borrow from or
inspect the raw item bytes:

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::read_item(reader).await?;
let value: cbor2::Value = cbor2::from_slice(&item)?;
# let _ = value;
# Ok(())
# }
```

The bare `async_io::AsyncRead`/`AsyncWrite` traits have no impls of their own.
Enable `futures` or `tokio` and call `async_io::futures::*` / `async_io::tokio::*`
to drive real `futures_io` / `tokio::io` streams.

## Migrate From ciborium or serde_cbor

Use direct replacements for ordinary serde use:

```rust
let bytes = cbor2::to_vec(&value)?;
let value: T = cbor2::from_slice(&bytes)?;
cbor2::to_writer(&value, writer)?;
let value: T = cbor2::from_reader(reader)?;
```

Check every binary payload. If old code relied on a type accepting byte
buffers through a specific serde visitor, use `serde_bytes`, a compatibility
helper, or decode through `cbor2::Value` and then convert intentionally.
