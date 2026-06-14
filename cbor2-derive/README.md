# cbor2-derive

Derive support for protocol-shaped CBOR with
[`cbor2`](https://crates.io/crates/cbor2).

English | [简体中文](README.zh-CN.md)

Most users should not depend on this crate directly. Enable the `derive`
feature on `cbor2` instead:

```toml
[dependencies]
cbor2 = { version = "1", features = ["derive"] }
serde_bytes = "0.11" # only needed for binary fields like the example below
```

## Why cbor2-derive

`serde` derives are excellent for the common data model, but some CBOR
protocols need wire details that serde's attributes cannot express directly:
integer map keys, field-order arrays, semantic tags and COSE-style compact
structures.
`#[derive(cbor2::Cbor)]` generates serde impls for that shape.

| Need               | Built in                                                                                                    |
| ------------------ | ----------------------------------------------------------------------------------------------------------- |
| Integer map keys   | `#[cbor(key = 1)]` writes a real CBOR integer key, not the text key `"1"`.                                  |
| Field-order arrays | `#[cbor(array)]` encodes a named struct as a compact CBOR array while keeping Rust field names.             |
| Semantic tags      | `#[cbor(tag = 18)]` wraps the encoded item in a CBOR tag and requires it on decode.                         |
| COSE ergonomics    | Compact RFC 9052 structures can be declared directly on Rust structs and tuple structs.                     |
| JSON compatibility | Field names and the type name stay untouched, so `serde_json` still uses the natural names and no CBOR tag. |
| Runtime metadata   | The generated `cbor2::Cbor` impl exposes `T::KEYS`, `T::TAG`, `T::ARRAY` and `value.keys()`.                |
| Serde attributes   | Field-level attributes such as `default`, `skip`, `alias` and `with = "serde_bytes"` continue to work.      |

## Example

```rust
use cbor2::Cbor;

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 18)]
struct CoseHeader {
    #[cbor(key = 1)]
    alg: i8,
    #[cbor(key = 4)]
    #[serde(with = "serde_bytes")]
    kid: Vec<u8>,
}

assert_eq!(CoseHeader::KEYS, &[("alg", 1), ("kid", 4)]);
assert_eq!(CoseHeader::TAG, Some(18));
```

For a COSE array-shaped message with named Rust fields:

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

assert!(Sign1::ARRAY);
```

The macro generates `serde::Serialize`, `serde::Deserialize` and
`cbor2::Cbor`. Do not also derive serde's `Serialize` or `Deserialize` on the
same type; those impls would conflict.

See the main [`cbor2` README](https://github.com/ldclabs/cbor2#integer-map-keys-and-tags-cose-with-derivecbor)
for complete COSE examples.

## License

Dual-licensed under MIT or the [UNLICENSE](http://unlicense.org).
