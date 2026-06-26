/*!
This crate provides a full-featured implementation of [RFC
8949](https://www.rfc-editor.org/rfc/rfc8949) — the Concise Binary Object
Representation (CBOR) — built on [serde](https://serde.rs).

CBOR adopts and modestly builds on the *data model* used by JSON, except the
encoding is in binary form. Its primary goals include a balance of
implementation size, message size and extensibility.

Feature coverage includes ordinary serde encode/decode, RFC 8949 preferred
serialization, deterministic/canonical encoders, a dynamic [`Value`] type,
validated [`RawValue`] pass-through bytes, CBOR simple values, semantic tags,
bignums, COSE-style integer map keys, arrays and tags through `#[derive(Cbor)]`, CBOR
sequences, async item I/O through [`async_io`], diagnostic notation,
exact-one-item [`validate`], allocation-free serialization helpers and
`no_std` modes.

# Quick start

Use [`to_vec`]/[`to_writer`] to encode any [`serde::Serialize`] type and
[`from_slice`]/[`from_reader`] to decode any [`serde::Deserialize`] type.
`from_slice` can also borrow definite-length text and byte strings directly
from the input buffer:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Photo {
    title: String,
    pixels: (u32, u32),
    tags: Vec<String>,
}

let photo = Photo {
    title: "Sunrise".into(),
    pixels: (1920, 1080),
    tags: vec!["morning".into(), "gradient".into()],
};

let bytes = cbor2::to_vec(&photo).unwrap();
let back: Photo = cbor2::from_slice(&bytes).unwrap();
assert_eq!(photo, back);
```

`from_slice` and `from_reader` deserialize one leading CBOR item. Use
[`validate`] first when a byte buffer must contain exactly one item, or use
[`de::Deserializer::into_iter`] for a CBOR sequence.

# For code agents

Code agents should read the repository's
[`AGENTS.md`](https://github.com/ldclabs/cbor2/blob/main/AGENTS.md) first for
the compressed API-selection contract, then use the
[`agent cookbook`](https://github.com/ldclabs/cbor2/blob/main/docs/agent-cookbook.md)
and the `agent_patterns` example for copyable recipes.

# Command line tool

The workspace also publishes `cbor2-cli`, which installs the `cbor`
command for converting CBOR to and from JSON and for rendering diagnostic
notation:

```text
brew install ldclabs/tap/cbor2-cli   # Homebrew, installs cbor
cargo install cbor2-cli              # Cargo, installs cbor
```

# Byte strings and `serde_bytes`

Serde's default data model treats `Vec<u8>` and `&[u8]` as sequences, so
they serialize as CBOR arrays, not byte strings. Use
[`serde_bytes`](https://docs.rs/serde_bytes/latest/serde_bytes/) when the
wire type should be major type 2.

```rust
let bytes = vec![1u8, 2, 3, 4];

// Bare Vec<u8>: [1, 2, 3, 4]
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "8401020304");

// serde_bytes::ByteBuf: h'01020304'
let bytes = serde_bytes::ByteBuf::from(bytes);
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "4401020304");
```

For struct fields, use serde's field adapter:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Packet {
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
}

let packet = Packet { payload: vec![0xde, 0xad, 0xbe, 0xef] };
assert_eq!(
    hex::encode(cbor2::to_vec(&packet).unwrap()),
    "a1677061796c6f616444deadbeef"
);
```

When building dynamic data directly, [`Value::Bytes`] already represents a
CBOR byte string:

```rust
let value = cbor2::Value::Bytes(vec![0xde, 0xad]);
assert_eq!(hex::encode(cbor2::to_vec(&value).unwrap()), "42dead");
```

# Borrowed Slice Deserialization

```rust
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
struct Borrowed<'a> {
    #[serde(borrow)]
    label: &'a str,
    #[serde(borrow, with = "serde_bytes")]
    payload: &'a [u8],
}

let bytes = hex::decode("a2656c6162656c626869677061796c6f616442dead").unwrap();
let value: Borrowed<'_> = cbor2::from_slice(&bytes).unwrap();
assert_eq!(value, Borrowed { label: "hi", payload: &[0xde, 0xad] });
```

Indefinite-length strings are accepted for owned targets, but cannot be
borrowed because their logical body is split across segments.

# Dynamic values

When the shape of the data is not known in advance, decode into a
[`Value`], the CBOR equivalent of `serde_json::Value`. The [`cbor!`] macro
builds `Value`s with a JSON-like syntax:

```rust
use cbor2::{cbor, Simple, Value};

let value = cbor!({
    "code": 415,
    "message": null,
    "tags": ["legacy", 1.5],
    (Simple::new(59).unwrap()) => [Value::Bytes(vec![0xde, 0xad])],
}).unwrap();

let bytes = cbor2::to_vec(&value).unwrap();
let back: Value = cbor2::from_slice(&bytes).unwrap();
assert_eq!(value, back);

let simple: Simple = cbor2::from_slice(&[0xf8, 0x3b]).unwrap();
assert_eq!(simple, Simple::new(59).unwrap());
```

`Value::serialized` and `Value::deserialized` convert between `Value` and
any type implementing the serde traits.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Point {
    x: i64,
    y: i64,
}

let value = cbor2::Value::serialized(&Point { x: -2, y: 5 }).unwrap();
assert_eq!(value.to_string(), r#"{"x": -2, "y": 5}"#);

let point: Point = value.deserialized().unwrap();
assert_eq!(point, Point { x: -2, y: 5 });
```

# Raw values

A [`RawValue`] keeps one CBOR item as its raw encoded bytes — validated
for well-formedness, but never decoded. Serializing splices the bytes
into the stream untouched and deserializing captures them byte for byte,
which preserves the exact wire encoding for signature payloads,
pass-through items and deferred decoding:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Signed {
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
    payload: cbor2::RawValue,
}

let bytes = cbor2::to_vec(&Signed {
    signature: vec![0xde, 0xad],
    payload: cbor2::RawValue::serialized(&("untouched", 42)).unwrap(),
}).unwrap();

let signed: Signed = cbor2::from_slice(&bytes).unwrap();
// Verify `signed.signature` over `signed.payload.as_bytes()`, then:
let (text, n): (String, u8) = signed.payload.deserialized().unwrap();
assert_eq!((text.as_str(), n), ("untouched", 42));
```

`TryFrom` converts in both directions between `RawValue` and [`Value`]:
decoding one way, encoding the other.

# CBOR sequences

CBOR sequences (RFC 8742) are streams of adjacent complete CBOR items.
Write them by calling [`to_writer`] repeatedly, and read them with
[`de::Deserializer::into_iter`]:

```rust
let mut stream = Vec::new();
cbor2::to_writer(&"hello", &mut stream).unwrap();
cbor2::to_writer(&42u64, &mut stream).unwrap();

let items: Vec<cbor2::Value> = cbor2::de::Deserializer::from_reader(&stream[..])
    .into_iter()
    .collect::<Result<_, _>>()
    .unwrap();

assert_eq!(items, vec![cbor2::Value::from("hello"), cbor2::Value::from(42)]);
assert!(cbor2::validate(&stream[..]).is_err()); // not exactly one item
```

# Async Item I/O

Serde itself is synchronous. The [`async_io`] module handles the async
transport boundary by reading or writing one complete CBOR item; once an item
is buffered, use the regular serde API. For untrusted peers, use
`read_item_with_limit` or `read_value_with_limit` unless an outer transport
layer already enforces a message size limit:

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let value: cbor2::Value = cbor2::async_io::read_value_with_limit(reader, 1 << 20).await?;
# let _ = value;
# Ok(())
# }
```

Enable the `futures` or `tokio` feature to call the matching
`async_io::futures::*` or `async_io::tokio::*` helpers directly with
`futures_io::AsyncRead`/`AsyncWrite` or `tokio::io::AsyncRead`/`AsyncWrite`
types.

# Tags

CBOR data items can be wrapped in semantic [tags](tag) (RFC 8949 §3.4). The
wrapper types in the [`tag`] module capture and emit tags through serde:

```rust
use cbor2::tag::RequireExact;

// Tag 32: a URI.
type Uri = RequireExact<String, 32>;

let uri: Uri = RequireExact("https://example.com".into());
let bytes = cbor2::to_vec(&uri).unwrap();
assert_eq!(bytes[0], 0xd8); // tag(32)
```

# Integer Map Keys, Arrays and Tags (COSE)

Protocols like COSE (RFC 9052) key their maps with integers, encode several
messages as arrays and wrap messages in tags, which serde's data model cannot
express on named structs. With the `derive` feature, `#[derive(Cbor)]`
declares those details — a textual
`#[serde(rename = "1")]` stays a *text* key, so there is no ambiguity
between the two. The derive generates the `Serialize` and `Deserialize`
impls itself, so serde's derives must not be repeated alongside it:

```rust
# #[cfg(feature = "derive")] {
use cbor2::Cbor;

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 98)]
struct CoseSign {
    #[cbor(key = 1)]
    kty: u8,
    #[cbor(key = 3)]
    alg: i8,
}

let key = CoseSign { kty: 2, alg: -7 };
let bytes = cbor2::to_vec(&key).unwrap();
assert_eq!(hex::encode(&bytes), "d862a201020326"); // 98({1: 2, 3: -7})
assert_eq!(cbor2::from_slice::<CoseSign>(&bytes).unwrap(), key);
# }
```

The tag is transparent on decode, and the serde attributes (`alias`,
`default`, `skip`, `with`, ...) work as usual; map types like
`HashMap<String, _>` are unaffected. The declared keys, array shape and tag
stay inspectable at runtime through the [`Cbor`](trait@Cbor) trait, which the
derive implements alongside the serde traits.

The derive touches neither the field names nor the type name — the
protocol details ride along on a hidden shadow type (see
[`ser::STRUCT_MARKER`]) recognized only by this crate's serializers — so
the same type still serializes naturally everywhere else. JSON, for
example, just works, with the original field names and no tag:

```rust
# #[cfg(feature = "derive")] {
# use cbor2::Cbor;
# #[derive(Debug, PartialEq, Cbor)]
# #[cbor(tag = 98)]
# struct CoseSign {
#     #[cbor(key = 1)]
#     kty: u8,
#     #[cbor(key = 3)]
#     alg: i8,
# }
# let key = CoseSign { kty: 2, alg: -7 };
let json = serde_json::to_string(&key).unwrap();
assert_eq!(json, r#"{"kty":2,"alg":-7}"#);
assert_eq!(serde_json::from_str::<CoseSign>(&json).unwrap(), key);
# }
```

For named Rust structs whose CBOR wire shape is an array, add
`#[cbor(array)]`:

```rust
# #[cfg(feature = "derive")] {
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

let msg = Sign1 {
    protected: vec![0xa0],
    unprotected: 0,
    payload: vec![],
    signature: vec![0xff],
};

assert_eq!(hex::encode(cbor2::to_vec(&msg).unwrap()), "d28441a0004041ff");
assert!(Sign1::ARRAY);
# }
```

# Allocation-free helpers

Three helpers work without touching the heap: [`validate`] checks that an
input is exactly one well-formed CBOR item (including text UTF-8 validity),
[`serialized_size`] computes the exact encoded size of any serializable
value, and [`to_slice`] encodes into a caller-provided buffer.

```rust
let value = ("hello", vec![1u8, 2, 3]);
let bytes = cbor2::to_vec(&value).unwrap();

assert_eq!(cbor2::serialized_size(&value).unwrap(), bytes.len() as u64);
assert!(cbor2::validate(&bytes[..]).is_ok());
assert!(cbor2::validate(&bytes[..bytes.len() - 1]).is_err()); // truncated

let mut buffer = [0u8; 16];
assert_eq!(cbor2::to_slice(&value, &mut buffer).unwrap(), &bytes[..]);
```

# Crate features

* **`std`** *(default)* — implements the [`io`] traits for every
  `std::io::Read`/`std::io::Write`, adds [`async_io`] and adds the
  `HashMap` conversions. Implies `alloc`.
* **`alloc`** — everything that needs a heap, without `std`: [`Value`],
  [`to_vec`]/[`from_slice`]/[`from_reader`], [`RawValue`],
  [`diagnostic`]/[`diagnostic_pretty`], the deterministic encoders and
  the [`cbor!`] macro. Readers and writers
  are byte slices, `Vec<u8>`, or custom [`io`] trait implementations.
* **neither** — a `#![no_std]` core for constrained targets: streaming
  serialization with [`to_writer`]/[`to_slice`]/[`serialized_size`],
  [`validate`], the [`tag`] wrappers and the [`core`] header codec.
  Deserializing through serde requires `alloc`.
* **`derive`** — the `#[derive(Cbor)]` macro; works in all three modes
  (deserialization again requiring `alloc`).
* **`futures`** — adds `async_io::futures` adapters for
  `futures_io::AsyncRead`/`futures_io::AsyncWrite`. Implies `std`.
* **`tokio`** — adds `async_io::tokio` adapters for
  `tokio::io::AsyncRead`/`tokio::io::AsyncWrite`. Implies `std`.

# Concise Diagnostic Notation

The API names keep the two directions separate:

* [`to_cdn`] and [`to_cdn_pretty`] render CBOR bytes **to** CDN text.
* `cdn*` and [`from_cdn`] parse CDN text **to** CBOR bytes or serde values.
* The historical `diagnostic*` names remain available as compatibility
  aliases for the corresponding `to_cdn*` functions.

[`to_cdn`] renders raw CBOR as the compact human-readable text form
formalized by the IETF Concise Diagnostic Notation draft
(`draft-ietf-cbor-edn-literals`); [`to_cdn_pretty`] does the same with
two-space indentation. Both work on the wire and preserve what a [`Value`]
cannot represent: indefinite-length markers, `undefined`, and unassigned
simple values. [`cdn_to_vec`] parses CDN text into CBOR bytes, including
comments, byte-string literals, embedded CBOR sequences, encoding indicators,
tags, simple values and the mandatory CDN application extensions. `Value`
implements
[`Display`](std::fmt::Display) with the same compact notation, and
[`Debug`](std::fmt::Debug) pretty-prints it with indentation.
When integer-keyed maps also have a known string-key table such as
[`Cbor::KEYS`], [`to_cdn_pretty_with_key_comments`] can annotate the
pretty output with `// "field"` comments without changing the decoded
wire item.

```rust
let bytes = hex::decode("bf61610161629f0203ffff").unwrap();
assert_eq!(
    cbor2::to_cdn(&bytes[..]).unwrap(),
    r#"{_ "a": 1, "b": [_ 2, 3]}"#
);
assert_eq!(
    cbor2::to_cdn_pretty(&bytes[..]).unwrap(),
    "{_\n  \"a\": 1,\n  \"b\": [_\n    2,\n    3\n  ]\n}"
);

let keyed = hex::decode("a201626d6504182a").unwrap();
let keys = [("iss", 1), ("exp", 4)];
assert_eq!(
    cbor2::to_cdn_pretty_with_key_comments(&keyed[..], &keys).unwrap(),
    "{\n  1: \"me\", // \"iss\"\n  4: 42 // \"exp\"\n}"
);

let value = cbor2::cbor!({ "k": [1, -2.5, null] }).unwrap();
assert_eq!(value.to_string(), r#"{"k": [1, -2.5, null]}"#);

let cdn = r#"{ /kty/ 1: 4, /k/ -1: h'6684523a' }"#;
let bytes = cbor2::cdn_to_vec(cdn).unwrap();
assert_eq!(cbor2::to_cdn(&bytes[..]).unwrap(), "{1: 4, -1: h'6684523a'}");
```

# Low-level headers

The [`core`] module exposes the pull/push header codec for applications
that need to preserve wire structure such as indefinite-length strings:

```rust
use cbor2::core::{Decoder, Encoder, Header};

let mut bytes = Vec::new();
let mut enc = Encoder::from(&mut bytes);
enc.push(Header::Array(None)).unwrap();
enc.text("chunked").unwrap();
enc.bytes(&[0xde, 0xad]).unwrap();
enc.push(Header::Break).unwrap();

let mut dec = Decoder::from(&bytes[..]);
assert_eq!(dec.pull().unwrap(), Header::Array(None));

let Header::Text(len) = dec.pull().unwrap() else { unreachable!() };
let mut text = String::new();
dec.text_body(len, &mut text).unwrap();
assert_eq!(text, "chunked");

let Header::Bytes(len) = dec.pull().unwrap() else { unreachable!() };
let mut body = Vec::new();
dec.bytes_body(len, &mut body).unwrap();
assert_eq!(body, vec![0xde, 0xad]);
assert_eq!(dec.pull().unwrap(), Header::Break);
```

# Deterministic encoding

[`to_canonical_vec`]/[`to_canonical_writer`] produce output satisfying the
core deterministic encoding requirements of RFC 8949 §4.2.1: preferred
(smallest) serializations, definite lengths only, and map keys sorted in the
bytewise lexicographic order of their encodings. [`Value::canonicalize`]
applies the same normalization to a `Value` in place.

```rust
use std::collections::HashMap;

// HashMap iteration order is random, but the encoding is stable.
let map: HashMap<&str, i32> = [("z", 1), ("aa", 2), ("b", 3)].into();

let bytes = cbor2::to_canonical_vec(&map).unwrap();
assert_eq!(bytes, cbor2::to_canonical_vec(&map).unwrap());
assert_eq!(hex::encode(&bytes), "a3616203617a01626161 02".replace(' ', ""));
```

Many existing protocols instead use the older "Canonical CBOR" key order of
RFC 7049 §3.9 (kept as RFC 8949 §4.2.3), where shorter encoded keys sort
first. Pass [`KeyOrder::LengthFirst`] to the `*_with` variants for that:

```rust
use cbor2::KeyOrder;

let map: std::collections::HashMap<i64, bool> = [(100, true), (-1, false)].into();

// Bytewise (RFC 8949 §4.2.1): 100 (0x1864) sorts before -1 (0x20).
let core = cbor2::to_canonical_vec(&map).unwrap();
assert_eq!(hex::encode(&core), "a2 1864f5 20f4".replace(' ', ""));

// Length-first (RFC 7049 §3.9): -1 sorts before 100.
let legacy = cbor2::to_canonical_vec_with(&map, KeyOrder::LengthFirst).unwrap();
assert_eq!(hex::encode(&legacy), "a2 20f4 1864f5".replace(' ', ""));
```

# Design decisions

This implementation is wire-compatible with
[`ciborium`](https://docs.rs/ciborium), whose design it follows:

* **Numbers are always encoded in their smallest lossless form**, as
  deterministic encoding (RFC 8949 §4.2.1) requires. Integer width in Rust
  is treated as an in-memory detail, not a wire property: `1u64` encodes as
  one byte, and that byte happily decodes into a `u128` or an `i8`.
* **`u128`/`i128` values outside the 64-bit range** are encoded as bignums
  (tags 2 and 3), and bignums small enough to fit are accepted for any
  integer type.
* **Maps are represented as `Vec<(Value, Value)>`** in [`Value`], preserving
  wire order and arbitrary (even duplicate) keys.
* **Be liberal in what you accept**: decoding handles indefinite-length
  items, segmented strings, half-width floats, leading zeros in bignums and
  unknown tags in most positions, even though encoding never produces most
  of those forms.
* **Deeply nested input fails with
  [`RecursionLimitExceeded`](de::Error::RecursionLimitExceeded)** instead of
  exhausting the stack; see [`de::Deserializer::with_recursion_limit`].

# History

This crate descends from `cbor` by Andrew Gallant, whose 0.4 and earlier
releases were built on the long-deprecated `rustc-serialize` framework and
predate both serde 1.0 and RFC 8949. Version 0.5 was a from-scratch rewrite
published under the `cbor2` name — the original crates.io name stays with
the legacy release — and 1.0 stabilizes it; none of the old API survives.
*/

#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(all(feature = "alloc", feature = "std"))]
pub mod async_io;
#[cfg(feature = "alloc")]
mod cdn;
pub mod core;
pub mod de;
#[cfg(feature = "alloc")]
mod diag;
pub mod io;
#[cfg(feature = "alloc")]
mod raw;
pub mod ser;
pub mod simple;
pub mod tag;
#[cfg(feature = "alloc")]
pub mod value;

#[cfg(feature = "alloc")]
pub use crate::cdn::{cdn_sequence_to_vec, cdn_to_vec, from_cdn};
#[doc(inline)]
pub use crate::de::validate;
#[cfg(feature = "alloc")]
#[doc(inline)]
pub use crate::de::{from_reader, from_slice};
#[cfg(feature = "alloc")]
pub use crate::diag::{
    diagnostic, diagnostic_pretty, diagnostic_pretty_with_key_comments, to_cdn, to_cdn_pretty,
    to_cdn_pretty_with_key_comments,
};
#[cfg(feature = "alloc")]
pub use crate::raw::RawValue;
#[doc(inline)]
pub use crate::ser::{serialized_size, to_slice, to_writer};
#[cfg(feature = "alloc")]
#[doc(inline)]
pub use crate::ser::{
    to_canonical_vec, to_canonical_vec_with, to_canonical_writer, to_canonical_writer_with, to_vec,
};
pub use crate::simple::Simple;
#[cfg(feature = "alloc")]
#[doc(inline)]
pub use crate::value::{KeyOrder, Value};

// Internal items that the `cbor!` macro expansion needs to reach through
// `$crate`. Not public API.
#[cfg(feature = "alloc")]
#[doc(hidden)]
pub mod __private {
    use alloc::string::String;

    use crate::value::{self, Integer, Value};

    pub use alloc::vec;

    fn flatten_error(msg: &'static str) -> value::Error {
        value::Error::Custom(String::from(msg))
    }

    fn unwrap_tags(mut value: Value) -> Value {
        while let Value::Tag(.., inner) = value {
            value = *inner;
        }
        value
    }

    fn key_text(mut key: &Value) -> Option<&str> {
        loop {
            match key {
                Value::Tag(.., inner) => key = inner,
                Value::Text(text) => return Some(text),
                _ => return None,
            }
        }
    }

    fn key_integer(mut key: &Value) -> Option<i128> {
        loop {
            match key {
                Value::Tag(.., inner) => key = inner,
                Value::Integer(int) => return Some(i128::from(*int)),
                _ => return None,
            }
        }
    }

    fn integer_for_field(keys: &[(&'static str, i128)], name: &str) -> Option<Integer> {
        keys.iter()
            .find_map(|(field, key)| (*field == name).then_some(*key))
            .and_then(|key| Integer::try_from(key).ok())
    }

    fn field_for_integer(keys: &[(&'static str, i128)], key: i128) -> Option<&'static str> {
        keys.iter()
            .find_map(|(field, field_key)| (*field_key == key).then_some(*field))
    }

    pub fn __cbor2_flatten_serialize(
        value: Value,
        tag: Option<u64>,
        keys: &[(&'static str, i128)],
    ) -> Result<Value, value::Error> {
        let Value::Map(mut entries) = unwrap_tags(value) else {
            return Err(flatten_error(
                "#[serde(flatten)] with #[derive(Cbor)] must serialize as a map",
            ));
        };

        for (key, _) in &mut entries {
            let mapped = key_text(key).and_then(|name| integer_for_field(keys, name));
            if let Some(integer) = mapped {
                *key = Value::Integer(integer);
            }
        }

        let value = Value::Map(entries);
        Ok(match tag {
            Some(tag) => Value::Tag(tag, value.into()),
            None => value,
        })
    }

    pub fn __cbor2_flatten_deserialize(
        value: Value,
        keys: &[(&'static str, i128)],
    ) -> Result<Value, value::Error> {
        let Value::Map(mut entries) = unwrap_tags(value) else {
            return Err(flatten_error(
                "#[serde(flatten)] with #[derive(Cbor)] must deserialize from a map",
            ));
        };

        for (key, _) in &mut entries {
            let mapped = key_integer(key).and_then(|key| field_for_integer(keys, key));
            if let Some(field) = mapped {
                *key = Value::Text(String::from(field));
            }
        }

        Ok(Value::Map(entries))
    }

    pub fn __cbor2_flatten_deserialize_value<'de, T: serde::Deserialize<'de>>(
        value: &Value,
    ) -> Result<T, value::Error> {
        value.__cbor2_deserialized_with_integer_identifiers()
    }
}
/// Derives [`serde::Serialize`] and [`serde::Deserialize`] with CBOR
/// protocol details: integer map keys, field-order arrays and a CBOR tag
/// (COSE, RFC 9052).
///
/// Annotate fields with `#[cbor(key = <integer>)]`, named structs with
/// `#[cbor(array)]`, and the container with `#[cbor(tag = <integer>)]`.
/// Do **not** also derive serde's
/// `Serialize`/`Deserialize` — this macro generates both impls. Field
/// names and the type name stay untouched, so the same type still
/// serializes naturally to JSON and other formats. See the [crate-level
/// documentation](crate#integer-map-keys-arrays-and-tags-cose) for examples.
///
/// The declared protocol details are also exposed for runtime inspection
/// through the [`Cbor`](trait@Cbor) trait, which this macro implements.
#[cfg(feature = "derive")]
pub use cbor2_derive::Cbor;

/// The CBOR protocol details a `#[derive(Cbor)]` type
/// declares: its integer map keys, array shape and tag.
///
/// The derive implements this trait alongside `Serialize` and
/// `Deserialize`, so the `#[cbor(...)]` attributes stay inspectable at
/// runtime — for building protocol documentation, validating foreign
/// input against the declared keys, or driving generic code off the tag.
///
/// ```rust
/// # #[cfg(feature = "derive")] {
/// use cbor2::Cbor; // one import: the derive macro and this trait
///
/// #[derive(Cbor)]
/// #[cbor(tag = 98)]
/// struct CoseSign {
///     #[cbor(key = 1)]
///     kty: u8,
///     #[cbor(key = 3)]
///     alg: i8,
///     comment: String, // no key: stays a text key on the wire
/// }
///
/// assert_eq!(CoseSign::KEYS, &[("kty", 1), ("alg", 3)]);
/// assert_eq!(CoseSign::TAG, Some(98));
///
/// let key = CoseSign { kty: 2, alg: -7, comment: "".into() };
/// assert_eq!(key.keys()["kty"], 1);
/// # }
/// ```
pub trait Cbor {
    /// The `serde field name → integer map key` pairs declared with
    /// `#[cbor(key = <integer>)]`, in declaration order.
    ///
    /// Names are the *serde* names, so a `#[serde(rename = ...)]` carries
    /// over. Fields without a key attribute are not listed — they keep
    /// their textual keys on the wire. For an enum, the table merges the
    /// keyed fields of every variant.
    const KEYS: &'static [(&'static str, i128)];

    /// The CBOR tag declared with `#[cbor(tag = <integer>)]`, if any.
    const TAG: Option<u64>;

    /// Whether a named struct is encoded as a CBOR array in field order.
    ///
    /// This is `true` for `#[cbor(array)]`. Tuple structs are already arrays
    /// in serde's data model and do not need this marker.
    const ARRAY: bool = false;

    /// The [`KEYS`](Self::KEYS) table collected into a map.
    #[cfg(feature = "alloc")]
    fn keys(&self) -> alloc::collections::BTreeMap<alloc::string::String, i128> {
        Self::KEYS
            .iter()
            .map(|&(name, key)| (alloc::string::String::from(name), key))
            .collect()
    }
}

/// Builds a [`Value`] from JSON-like syntax.
///
/// Maps use `:` between keys and values, exactly like `serde_json::json!`;
/// any expression implementing [`serde::Serialize`] can be inlined,
/// including nested `cbor!` maps and arrays. Going beyond JSON, map keys
/// may be any CBOR value — integers included — and `null` is the CBOR
/// null. The macro returns `Result<Value, value::Error>`.
///
/// ```rust
/// use cbor2::cbor;
///
/// let value = cbor!({
///     "code": 415,
///     "message": null,
///     "continue": false,
///     "extra": { "numbers": [8.2341e+4, 0.251425] },
///     1: "an integer key",
/// }).unwrap();
/// ```
///
/// The ciborium-style `=>` separator is accepted as well, and is handy
/// when a key expression itself contains a colon (alternatively,
/// parenthesize the key):
///
/// ```rust
/// use cbor2::cbor;
///
/// const ALG: i8 = 1;
///
/// let value = cbor!({ ALG => -7, (i8::MAX) : 0 }).unwrap();
/// ```
#[cfg(feature = "alloc")]
#[macro_export]
macro_rules! cbor {
    //////////// arrays ////////////

    // Done, with or without a trailing comma.
    (@array [$($elems:expr,)*]) => {
        $crate::value::Value::Array($crate::__private::vec![$($elems,)*])
    };
    (@array [$($elems:expr),*]) => {
        $crate::value::Value::Array($crate::__private::vec![$($elems),*])
    };

    // Next element is an array.
    (@array [$($elems:expr,)*] [$($array:tt)*] $($rest:tt)*) => {
        $crate::cbor!(@array [$($elems,)* $crate::cbor!(@array [] $($array)*)] $($rest)*)
    };

    // Next element is a map.
    (@array [$($elems:expr,)*] {$($map:tt)*} $($rest:tt)*) => {
        $crate::cbor!(@array [$($elems,)* $crate::cbor!(@map [] () ($($map)*) ($($map)*))] $($rest)*)
    };

    // Next element is an expression followed by a comma.
    (@array [$($elems:expr,)*] $next:expr, $($rest:tt)*) => {
        $crate::cbor!(@array [$($elems,)* $crate::cbor!(@leaf $next),] $($rest)*)
    };

    // Last element is an expression with no trailing comma.
    (@array [$($elems:expr,)*] $last:expr) => {
        $crate::cbor!(@array [$($elems,)* $crate::cbor!(@leaf $last)])
    };

    // Comma after the most recent element.
    (@array [$($elems:expr),*] , $($rest:tt)*) => {
        $crate::cbor!(@array [$($elems,)*] $($rest)*)
    };

    // Unexpected token after the most recent element.
    (@array [$($elems:expr),*] $unexpected:tt $($rest:tt)*) => {
        $crate::cbor_unexpected!($unexpected)
    };

    //////////// maps ////////////
    //
    // The state is `[finished (key, value) pairs] (tokens of the key
    // being munched) (remaining input) (copy of the remaining input,
    // for error reporting)`. Keys are munched one token at a time
    // because an `expr` fragment cannot be followed by `:`.

    // Done.
    (@map [$($pairs:expr,)*] () () ()) => {
        $crate::value::Value::Map($crate::__private::vec![$($pairs,)*])
    };

    // Insert the current entry followed by a trailing comma.
    (@map [$($pairs:expr,)*] [$($key:tt)+] ($value:expr) , $($rest:tt)*) => {
        $crate::cbor!(@map [$($pairs,)* ($crate::cbor!(@key $($key)+), $value),] () ($($rest)*) ($($rest)*))
    };

    // Current entry followed by an unexpected token.
    (@map [$($pairs:expr,)*] [$($key:tt)+] ($value:expr) $unexpected:tt $($rest:tt)*) => {
        $crate::cbor_unexpected!($unexpected)
    };

    // Insert the last entry without a trailing comma.
    (@map [$($pairs:expr,)*] [$($key:tt)+] ($value:expr)) => {
        $crate::value::Value::Map($crate::__private::vec![$($pairs,)* ($crate::cbor!(@key $($key)+), $value)])
    };

    // Next value is an array.
    (@map [$($pairs:expr,)*] ($($key:tt)+) (: [$($array:tt)*] $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@array [] $($array)*)) $($rest)*)
    };
    (@map [$($pairs:expr,)*] ($($key:tt)+) (=> [$($array:tt)*] $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@array [] $($array)*)) $($rest)*)
    };

    // Next value is a map.
    (@map [$($pairs:expr,)*] ($($key:tt)+) (: {$($map:tt)*} $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@map [] () ($($map)*) ($($map)*))) $($rest)*)
    };
    (@map [$($pairs:expr,)*] ($($key:tt)+) (=> {$($map:tt)*} $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@map [] () ($($map)*) ($($map)*))) $($rest)*)
    };

    // Next value is an expression followed by a comma.
    (@map [$($pairs:expr,)*] ($($key:tt)+) (: $value:expr , $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@leaf $value)) , $($rest)*)
    };
    (@map [$($pairs:expr,)*] ($($key:tt)+) (=> $value:expr , $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@leaf $value)) , $($rest)*)
    };

    // Last value is an expression with no trailing comma.
    (@map [$($pairs:expr,)*] ($($key:tt)+) (: $value:expr) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@leaf $value)))
    };
    (@map [$($pairs:expr,)*] ($($key:tt)+) (=> $value:expr) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] [$($key)+] ($crate::cbor!(@leaf $value)))
    };

    // Missing value for the last entry: "unexpected end of macro
    // invocation".
    (@map [$($pairs:expr,)*] ($($key:tt)+) (:) $copy:tt) => {
        $crate::cbor!()
    };
    (@map [$($pairs:expr,)*] ($($key:tt)+) (=>) $copy:tt) => {
        $crate::cbor!()
    };

    // Missing separator and value for the last entry.
    (@map [$($pairs:expr,)*] ($($key:tt)+) () $copy:tt) => {
        $crate::cbor!()
    };

    // Misplaced separator: no key came before it. "No rules expected
    // the token `:`/`=>`".
    (@map [$($pairs:expr,)*] () (: $($rest:tt)*) ($sep:tt $($copy:tt)*)) => {
        $crate::cbor_unexpected!($sep)
    };
    (@map [$($pairs:expr,)*] () (=> $($rest:tt)*) ($sep:tt $($copy:tt)*)) => {
        $crate::cbor_unexpected!($sep)
    };

    // A comma inside a key. "No rules expected the token `,`".
    (@map [$($pairs:expr,)*] ($($key:tt)*) (, $($rest:tt)*) ($comma:tt $($copy:tt)*)) => {
        $crate::cbor_unexpected!($comma)
    };

    // A fully parenthesized key — for key expressions containing `:`.
    (@map [$($pairs:expr,)*] () (($key:expr) : $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] ($key) (: $($rest)*) ($copy))
    };
    (@map [$($pairs:expr,)*] () (($key:expr) => $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] ($key) (=> $($rest)*) ($copy))
    };

    // Refuse to absorb a separator into the key expression.
    (@map [$($pairs:expr,)*] ($($key:tt)*) (: $($unexpected:tt)+) $copy:tt) => {
        $crate::cbor_expect_expr_comma!($($unexpected)+)
    };
    (@map [$($pairs:expr,)*] ($($key:tt)*) (=> $($unexpected:tt)+) $copy:tt) => {
        $crate::cbor_expect_expr_comma!($($unexpected)+)
    };

    // Munch a token into the current key.
    (@map [$($pairs:expr,)*] ($($key:tt)*) ($tt:tt $($rest:tt)*) $copy:tt) => {
        $crate::cbor!(@map [$($pairs,)*] ($($key)* $tt) ($($rest)*) ($copy))
    };

    //////////// keys and leaves ////////////

    // A nested map or array as the key.
    (@key {$($map:tt)*}) => {
        $crate::cbor!(@map [] () ($($map)*) ($($map)*))
    };
    (@key [$($array:tt)*]) => {
        $crate::cbor!(@array [] $($array)*)
    };
    (@key $key:expr) => {
        $crate::cbor!(@leaf $key)
    };

    // Any serializable expression; `null` is the CBOR null.
    (@leaf $val:expr) => {{
        #[allow(unused_imports)]
        use $crate::value::Value::Null as null;
        $crate::value::Value::serialized(&$val)?
    }};

    //////////// entry points ////////////

    ({ $($map:tt)* }) => {
        (|| {
            ::core::result::Result::<_, $crate::value::Error>::Ok(
                $crate::cbor!(@map [] () ($($map)*) ($($map)*)),
            )
        })()
    };

    ([ $($array:tt)* ]) => {
        (|| {
            ::core::result::Result::<_, $crate::value::Error>::Ok(
                $crate::cbor!(@array [] $($array)*),
            )
        })()
    };

    ($val:expr) => {{
        #[allow(unused_imports)]
        use $crate::value::Value::Null as null;
        $crate::value::Value::serialized(&$val)
    }};
}

// Produces a "no rules expected the token ..." error at the offending
// token. Not public API.
#[macro_export]
#[doc(hidden)]
macro_rules! cbor_unexpected {
    () => {};
}

// Produces an "expected expression followed by `,`"-shaped error at the
// offending tokens. Not public API.
#[macro_export]
#[doc(hidden)]
macro_rules! cbor_expect_expr_comma {
    ($e:expr , $($tt:tt)*) => {};
}
