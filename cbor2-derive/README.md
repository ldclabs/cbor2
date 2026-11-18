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

| Need               | Built in                                                                                                        |
| ------------------ | --------------------------------------------------------------------------------------------------------------- |
| Integer map keys   | `#[cbor(key = 1)]` writes a real CBOR integer key, not the text key `"1"`.                                      |
| Field-order arrays | `#[cbor(array)]` encodes a named struct as a compact CBOR array while keeping Rust field names.                 |
| Semantic tags      | `#[cbor(tag = 18)]` wraps the encoded item in a CBOR tag and accepts tagged or untagged input on decode.        |
| COSE ergonomics    | Compact RFC 9052 structures can be declared directly on Rust structs and tuple structs.                         |
| JSON compatibility | Field names and the type name stay untouched, so `serde_json` still uses the natural names and no CBOR tag.     |
| Runtime metadata   | The generated `cbor2::Cbor` impl exposes `T::KEYS`, `T::TAG`, `T::ARRAY` and `value.keys()`.                    |
| Serde attributes   | Field-level attributes such as `default`, `skip`, `alias` and `with = "serde_bytes"` continue to work.          |
| Flattened maps     | `#[serde(flatten)]` on map-shaped structs can carry extension fields, including COSE-style integer/text labels. |

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

For map-shaped protocols with extension claims, such as CWT:

```rust
use std::collections::BTreeMap;

use cbor2::{Cbor, Value};

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 61)]
struct Claims {
    #[cbor(key = 1)]
    #[serde(rename = "iss")]
    issuer: String,
    #[serde(flatten, default)]
    extra: BTreeMap<String, Value>,
}
```

The declared fields still use their integer CBOR keys and the flattened map
keeps ordinary text keys. Flattened maps whose key type serializes as integers
or strings, such as a COSE `Label` / `CoseMap`, preserve those integer keys on
CBOR round trips. `#[serde(flatten)]` is supported for named map structs; do not
combine it with `#[cbor(array)]`.

The macro generates `serde::Serialize`, `serde::Deserialize` and
`cbor2::Cbor`. Do not also derive serde's `Serialize` or `Deserialize` on the
same type; those impls would conflict.

See the main [`cbor2` README](https://github.com/ldclabs/cbor2#integer-map-keys-and-tags-cose-with-derivecbor)
for complete COSE examples.

The derive needs only the `cbor2` dependency with `derive` enabled; serde is
resolved through cbor2. Container `default` and recursive `Self` types are
supported. Positional arrays reject conditional and one-directional skips;
use `Option` placeholders or symmetric `#[serde(skip)]` instead.

Flattened structs preserve the exact encoding of declared `RawValue` fields
and support borrowed fields when decoding from a slice. Extension values can
carry tags and simple values. Encoding rejects an extension label that
collides with a declared field name, alias, or registered integer label. The binary flatten
adapter uses cbor2's raw-item protocol; other binary serializers are not
supported. Human-readable formats keep the ordinary serde representation.

Flattening an inner struct uses serde's field names; it does not inherit the
inner type's `Cbor::KEYS`, tag or array shape. Declare registered keys on the
outer struct and use a flattened map for extension labels. For example,
flattening an `Inner { #[cbor(key = 1)] value: u8 }` field writes `"value"`,
not integer `1`.

An enum's key table is shared by its variants. Repeated serde field names
must consistently declare the same integer key. Enums with integer keys
cannot contain `#[serde(untagged)]` variants. On structs, `#[serde(tag =
"...")]` cannot be combined with CBOR keys, tags or array shape.

## License

Licensed under the MIT License.

Cargo dependency aliases work with the derive, for example `#[derive(wire::Cbor)]`
when the dependency is named `wire`.
