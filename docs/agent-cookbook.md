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
| Preserve CBOR simple values | `cbor2::Simple` or `Value::Simple` |
| Render CBOR as CDN text | `cbor2::to_cdn(bytes)` / `to_cdn_pretty(bytes)` |
| Encode Concise Diagnostic Notation | `cbor2::cdn_to_vec(cdn)` |
| Deserialize Concise Diagnostic Notation | `cbor2::from_cdn::<T>(cdn)` |
| Produce deterministic bytes | `cbor2::to_canonical_vec(&value)` |
| Pretty-print integer-keyed maps with names | `cbor2::to_cdn_pretty_with_key_comments(bytes, T::KEYS)` |
| Read/write one typed value async | `cbor2::async_io::read_value` / `write_value` |
| Read from an untrusted async stream | `cbor2::async_io::read_value_with_limit` / `read_item_with_limit` |
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

## Encode CDN Fixtures

Use `cdn_to_vec` when a test vector is clearer as Concise Diagnostic
Notation than as JSON. CDN can express integer map keys, byte strings, tags,
simple values, comments, encoding indicators and application extensions
directly. Use `bytes` to reinterpret text as byte strings, `same` to assert
that multiple literals describe the same CBOR item, and `float` to spell raw
IEEE 754 binary16/32/64 payloads.

```rust
let bytes = cbor2::cdn_to_vec(r#"{ /kty/ 1: 4, /k/ -1: h'6684523a' }"#).unwrap();
assert_eq!(hex::encode(bytes), "a2010420446684523a");

let payload = cbor2::cdn_to_vec(r#"bytes<<"sig:", h'deadbeef'>>"#).unwrap();
assert_eq!(hex::encode(payload), "487369673adeadbeef");

let float = cbor2::cdn_to_vec(r#"same<<float'47110815', 0x1.22102ap+15>>"#).unwrap();
assert_eq!(hex::encode(float), "fa47110815");

let value: cbor2::Value = cbor2::from_cdn(r#"{1: [2, 3]}"#).unwrap();
assert_eq!(value.to_string(), "{1: [2, 3]}");
```

For terminal workflows, prefer copyable hex:

```bash
printf "{1: h'dead'}" | cbor encode --cdn --hex
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

## Preserve CBOR Simple Values

Use `Simple` when a protocol registers a CBOR simple value outside serde's
built-in bool/null shapes. `Value::Simple` can also appear as a map key.

```rust
use cbor2::{cbor, Simple, Value};

let redacted_claim_keys = Simple::new(59).unwrap(); // SD-CWT #7.59
let claims = cbor!({
    (redacted_claim_keys) => [Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef])],
})
.unwrap();

let bytes = cbor2::to_vec(&claims).unwrap();
assert_eq!(cbor2::from_slice::<Simple>(&[0xf8, 0x3b]).unwrap(), redacted_claim_keys);
assert_eq!(cbor2::from_slice::<Value>(&bytes).unwrap(), claims);
```

Common mistake: representing a simple value as the integer `59`. CBOR
`simple(59)` is major type 7 (`f8 3b`), not unsigned integer 59 (`18 3b`).

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

A `#[cbor(tag = N)]` tag is written on encode and transparent on decode. When
a protocol travels both tagged and untagged (CWT, many COSE messages), one type
decodes either form instead of defining a second "bare" struct:

```rust
use cbor2::Cbor;

#[derive(Debug, PartialEq, Default, Cbor)]
#[cbor(tag = 61)]
struct Claims {
    #[cbor(key = 1)]
    #[serde(rename = "iss")]
    issuer: String,
    #[serde(flatten, default)]
    extra: std::collections::BTreeMap<String, cbor2::Value>,
}

// Encodes with tag 61; decodes whether or not the tag is present.
let bytes = cbor2::to_canonical_vec(&Claims {
    issuer: "me".into(),
    ..Default::default()
})
.unwrap();
assert_eq!(cbor2::from_slice::<Claims>(&bytes).unwrap().issuer, "me");
assert_eq!(cbor2::from_slice::<Claims>(&bytes[2..]).unwrap().issuer, "me");
```

For map-shaped protocols such as CWT, `#[serde(flatten)]` can carry
application/private fields that are outside the typed registered subset. The
declared fields still use their integer CBOR keys. Flattened text keys remain
text keys, and flattened map key types that serialize as integers or strings
such as a COSE `Label` / `CoseMap` preserve their integer labels on CBOR
round trips.

When documenting or debugging an integer-keyed protocol map, keep the raw
integer keys in diagnostic notation and pass the derived key table to add
string-key comments:

```rust
use cbor2::Cbor;

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 61)]
struct Claims {
    #[cbor(key = 1)]
    #[serde(rename = "iss")]
    issuer: String,
    #[cbor(key = 4)]
    #[serde(rename = "exp")]
    expiration: u64,
}

let bytes = cbor2::to_canonical_vec(&Claims {
    issuer: "me".into(),
    expiration: 1444064944,
})
.unwrap();

let diag = cbor2::to_cdn_pretty_with_key_comments(&bytes[..], Claims::KEYS).unwrap();
assert_eq!(
    diag,
    "61({\n  1: \"me\", // \"iss\"\n  4: 1444064944 // \"exp\"\n})"
);
```

Common mistakes:

- Do not also write `#[derive(Serialize, Deserialize)]`; `Cbor` generates
  those impls.
- Do not use `#[serde(rename = "1")]` for integer keys; that creates a text
  key. Use `#[cbor(key = 1)]`.
- Do not replace integer keys with text keys just to make diagnostics readable;
  use `to_cdn_pretty_with_key_comments` with `T::KEYS`.
- Do not combine `#[cbor(array)]` with per-field integer keys or
  `#[serde(flatten)]`; flatten is for map-shaped structs.

## Use Async Transports

Serde itself is synchronous. `async_io::read_value`/`write_value` frame one
complete CBOR item on an async stream and (de)serialize it in a single call:

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<cbor2::Value, cbor2::de::Error> {
let value: cbor2::Value = cbor2::async_io::read_value(reader).await?;
# Ok(value)
# }
```

For untrusted peers, use a bounded reader unless an outer protocol already
enforces a message size:

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<cbor2::Value, cbor2::de::Error> {
let value: cbor2::Value = cbor2::async_io::read_value_with_limit(reader, 1 << 20).await?;
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

## Use The CLI Safely

When an agent needs to inspect or generate CBOR from a shell, use the `cbor`
binary from `cbor2-cli`:

```bash
cbor a1616101
cbor decode bf616101ff
cbor decode --json a1616101
echo '{"a":1}' | cbor encode --json --hex
printf "{1: h'dead'}" | cbor encode --cdn --hex
cbor validate a1616101
```

Prefer `cbor encode --hex` in transcripts, tests and docs because it prints
copyable lowercase hex instead of raw binary stdout. Use raw `cbor encode` only
when the next command expects CBOR bytes on stdin. Add `--json` for strict JSON
input, or `--diag`/`--cdn` for CDN input. `cbor validate` prints `valid` and
exits 0 for one or more complete CBOR items; malformed data exits 1 and
command-line usage errors exit 2.

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
