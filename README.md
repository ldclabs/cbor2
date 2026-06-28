# cbor2

Full-featured [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949) CBOR for
Rust: async item I/O, serde round trips, canonical/deterministic encoding,
`Value`/`RawValue`, CBOR simple values, COSE-style integer map keys, semantic tags,
diagnostic notation, `no_std`, and a separately available well-formedness check.

[![CI](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml/badge.svg)](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cbor2.svg)](https://crates.io/crates/cbor2)
[![docs.rs](https://docs.rs/cbor2/badge.svg)](https://docs.rs/cbor2)

English | [简体中文](README.zh-CN.md)

`cbor2` is for applications that need a complete CBOR toolkit, not just a
basic serializer. It works with ordinary `serde::Serialize`/`Deserialize`
types, preserves protocol details when the wire shape matters, and scales
from `std` services down to constrained `no_std` targets.

## Why cbor2

| Need                     | Built in                                                                                                                 |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| Serde encode/decode      | `to_vec`, `to_writer`, borrowing `from_slice`, `from_reader` and direct support for derived serde types.                 |
| Stable protocol bytes    | RFC 8949 preferred serialization plus deterministic/canonical encoders and selectable map key ordering.                  |
| Protocol CBOR            | Simple values, semantic tags, bignums, integer map keys, field-order arrays and COSE-style tags with `#[derive(cbor2::Cbor)]`. |
| Dynamic or unknown data  | `Value`, the `cbor!` macro and `RawValue` for validated pass-through bytes.                                              |
| Safe input handling      | Exact-one-item well-formedness check, CBOR sequence iteration, recursion limits and guarded allocation sizes.            |
| Async boundaries         | `async_io` reads or writes one complete CBOR item without pretending serde itself is async.                              |
| Debugging and inspection | RFC 8949 diagnostic notation, pretty diagnostics and the companion `cbor` CLI.                                           |
| Embedded targets         | `no_std + alloc` for the full heap-backed API, or no allocation for serialization, well-formedness checks and the core header codec. |

Licensed under the MIT License.

## Comparison with other CBOR crates

The [`cbor2-bench`](cbor2-bench/README.md) workspace measures cbor2 against
`ciborium 0.2`, `serde_cbor 0.11`, `cbor4ii 1.2` and `minicbor 2.2` on both
features and speed. It is a *detached* workspace, so none of those crates enter
this library's dependency graph, CI or MSRV.

### Feature comparison

| capability                             | cbor2 | ciborium | serde_cbor | cbor4ii | minicbor |
| -------------------------------------- | :---: | :------: | :--------: | :-----: | :------: |
| serde-native `Serialize`/`Deserialize` |   ✅   |    ✅     |     ✅      |    ✅    |    ❌¹    |
| `no_std` + `alloc`                     |   ✅   |    ✅     |     ✅      |    ✅    |    ✅     |
| zero-alloc encode (fixed buffer)       |   ✅   |    ✅     |     ✅      |   ✅⁵    |    ✅     |
| typed decode without `alloc`           |  ❌²   |    ❌     |     ❌      |   ❌²    |    ✅     |
| borrow `&str`/`&[u8]` from the input   |   ✅   |    ❌     |     ✅      |    ✅    |    ✅     |
| deterministic / canonical encoding³    |   ✅   |    ❌     |     ❌      |    ❌    |    ❌     |
| dynamic `Value` type                   |   ✅   |    ✅     |     ✅      |    ✅    |    ❌     |
| raw pass-through value (`RawValue`)    |   ✅   |    ❌     |     ❌      |   ✅⁶    |    ❌     |
| semantic tags                          |   ✅   |    ✅     |     ✅      |    ✅    |    ✅     |
| integer map keys for structs (COSE)    |   ✅   |    ❌     |     ❌      |    ❌    |    ✅     |
| diagnostic notation (RFC 8949 §8)      |   ✅   |    ❌     |     ❌      |    ❌    |    ✅     |
| async item I/O (futures / tokio)       |   ✅   |    ❌     |     ❌      |    ❌    |    ❌     |
| validate / exact size without decoding |   ✅   |    ❌     |     ❌      |    ❌    |    ◑⁴    |

¹ minicbor uses its own `#[derive(Encode, Decode)]`; serde is a separate `minicbor-serde` crate.

² No serde-based CBOR crate deserializes without a heap — but cbor2's low-level [`core::Decoder`](https://docs.rs/cbor2/latest/cbor2/core/struct.Decoder.html) (and cbor4ii's low-level `Decode`) still decode manually with zero allocation.

³ Sorted map keys, RFC 8949 §4.2.1; most crates emit preferred shortest-form numbers (cbor4ii keeps floats at 64-bit), but only cbor2 ships a full canonical encoder.

⁴ minicbor's `Decoder::skip` validates structure but there is no exact-size primitive.

⁵ cbor4ii has no public `no_std` slice serializer; it fills a fixed buffer through `to_writer` over `&mut [u8]`, which needs `std`.

⁶ cbor4ii's `RawValue` is a core-level borrowed type, not serde-integrated.

`serde_cbor` is unmaintained; the others are maintained.

### Benchmarks

Median time per operation on an Apple M1 Pro, the `no_std + alloc` path
(`to_vec` / `from_slice`); lower is better. The full `std` and
`no_std + no_alloc` tables, payload definitions and methodology are in
[`cbor2-bench`](cbor2-bench/README.md#results).

| op / payload       | cbor2   | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------ | ------- | -------- | ---------- | ------- | -------- |
| `encode/int_array` | 2.79 µs | 6.59 µs  | 1.67 µs    | 2.92 µs | 3.29 µs  |
| `encode/log_batch` | 13.3 µs | 16.1 µs  | 9.54 µs    | 6.09 µs | 4.56 µs  |
| `encode/blob`      | 102 ns  | 131 ns   | 133 ns     | 127 ns  | 130 ns   |
| `decode/int_array` | 5.34 µs | 11.0 µs  | 3.24 µs    | 3.43 µs | 5.23 µs  |
| `decode/log_batch` | 38.5 µs | 66.3 µs  | 34.0 µs    | 36.8 µs | 21.8 µs  |
| `decode/blob`      | 97.5 ns | 224 ns   | 88.5 ns    | 90.1 ns | 91.1 ns  |

`int_array` (1024 × `u64`) and `blob` (a 4 KiB byte string) are byte-identical
across all five crates, so those rows are exact apples-to-apples; `log_batch`
(128 structured records) uses each crate's idiomatic encoding (minicbor's
integer-keyed arrays run ~37% smaller, and cbor4ii keeps floats at 64-bit).
cbor2 is competitive across the board and **uniquely strong in `no_std +
no_alloc`** — it has the fastest fixed-buffer encode of the serde crates and
the only `serialized_size`/`validate` primitives. On `std`/`alloc` structured
throughput **cbor4ii is the standout** (and minicbor's borrowing decoder leads
structured decode); cbor2 trades the encode lead with them by scenario — see
the full tables. In `no_std + no_alloc`, cbor2 also offers zero-alloc
*encoding* ([`to_slice`]), *validation* ([`validate`]) and exact *sizing*
([`serialized_size`]).

```bash
cd cbor2-bench && cargo bench
```

[`to_slice`]: https://docs.rs/cbor2/latest/cbor2/fn.to_slice.html
[`validate`]: https://docs.rs/cbor2/latest/cbor2/fn.validate.html
[`serialized_size`]: https://docs.rs/cbor2/latest/cbor2/fn.serialized_size.html

## Quick start

```toml
[dependencies]
cbor2 = "1"
```

For the `cbor` command line tool, install `cbor2-cli`:

```bash
brew install ldclabs/tap/cbor2-cli   # Homebrew, installs `cbor`
cargo install cbor2-cli              # Cargo, installs `cbor`
```

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

`to_writer` and `from_reader` work with any `std::io::Write`/`Read`, and
`Deserializer::into_iter` decodes a stream of concatenated items.
`from_slice`/`from_reader` read one leading CBOR item; use `validate` when
a buffer must contain exactly one item.

## For AI agents

Code agents should start with [`AGENTS.md`](AGENTS.md) for the compressed API
selection rules, then use [`docs/agent-cookbook.md`](docs/agent-cookbook.md)
for copyable recipes and common migration traps. The runnable
[`agent_patterns`](examples/agent_patterns.rs) example covers exact-item
well-formedness checks, byte strings, borrowed deserialization, raw values,
CBOR sequences and canonical encoding.

## Highlights

* **Full serde integration** — `#[derive(Serialize, Deserialize)]` types
  encode and decode directly.
* **Borrowing `from_slice`** — definite-length text and byte strings can
  deserialize as `&str` and borrowed `serde_bytes` values directly from the
  input buffer; segmented indefinite strings fall back to owned buffers.
* **RFC 8949 preferred serialization** — integers and floats are always
  encoded in their smallest lossless form, including half-precision floats.
* **A dynamic `Value` type** — the CBOR analogue of `serde_json::Value`,
  with a `cbor!` macro for building values in JSON-like syntax.
* **CBOR simple values** — `Simple` and `Value::Simple` preserve registered
  and unassigned simple values beyond serde's built-in bool/null shapes,
  including map keys such as SD-CWT's `simple(59)`.
* **Tag support** — capture and emit semantic tags (RFC 8949 §3.4) through
  the wrapper types in the `tag` module; `u128`/`i128` map to bignum tags
  automatically.
* **Deterministic encoding** — `to_canonical_vec`/`to_canonical_writer` and
  `Value::canonicalize` implement the core deterministic encoding
  requirements (RFC 8949 §4.2.1): bytewise lexicographic map key order,
  definite lengths, preferred serializations, normalized bignums and NaN.
  For protocols built on the older RFC 7049 §3.9 "Canonical CBOR" rule
  (kept as RFC 8949 §4.2.3, and used by ciborium's canonical module), the
  `*_with` variants take `KeyOrder::LengthFirst`.
* **COSE-style integer map keys, arrays and tags** — with the `derive` feature,
  `#[derive(cbor2::Cbor)]` maps struct fields to integer keys
  (`#[cbor(key = 1)]`), encodes named structs as field-order arrays
  (`#[cbor(array)]`) and wraps containers in CBOR tags
  (`#[cbor(tag = 18)]`), as RFC 9052 requires, with no ambiguity against
  textual keys. Tags are written on encode and transparent on decode, so one
  type accepts tagged or untagged input. Field names and the type name stay
  untouched, so the same types still serialize to plain JSON —
  `serde_json::to_string(&v)` just works, with the original field names and
  no tag. The declared keys, array shape and tag stay inspectable at runtime
  through the `cbor2::Cbor` trait.
* **Raw values** — `RawValue` keeps one item as validated, undecoded
  bytes: serializing splices them into the stream untouched and
  deserializing captures them byte for byte, for signature payloads,
  pass-through items and deferred decoding. `TryFrom` converts in both
  directions between `RawValue` and `Value`.
* **Robust decoding** — indefinite-length items, segmented strings,
  duplicate map keys, unknown tags and CBOR sequences (RFC 8742) are all
  handled; recursion is depth-limited and forged lengths cannot trigger
  huge allocations.
* **Concise Diagnostic Notation** — `to_cdn` renders raw CBOR as the
  human-readable text form formalized by the IETF Concise Diagnostic
  Notation draft (CDN, `draft-ietf-cbor-edn-literals`), matching the RFC
  8949 Appendix A examples for ordinary items while preserving
  indefinite-length markers. The API names keep direction explicit:
  `to_cdn*` renders CBOR bytes to CDN text, while `cdn_to_vec`,
  `cdn_sequence_to_vec` and `from_cdn` parse CDN text to CBOR bytes or serde
  values; the older `diagnostic*` names remain as compatibility aliases. CDN
  input covers comments, base-encoded byte strings, embedded CBOR sequences,
  encoding indicators, tags, simple values and CDN application extensions such
  as `dt`/`DT`, `ip`/`IP`, `b1`/`t1`, `ilbs`/`ilts`, `bytes`, `same` and
  `float`; enable the `cdn` feature for `hash`, `cri` and `CRI`.
  `bytes<<"ä", h'2f'>>` produces `h'c3a42f'`, while
  `same<< float'47110815', 0x1.22102ap+15 >>` checks alternate spellings of
  the same item and emits the first one. `Value` implements `Display` with
  the same notation and `Debug` as
  its indented form. For integer-keyed protocol maps,
  `to_cdn_pretty_with_key_comments` can add CDN `// "iss"` comments beside the
  wire integer keys.
* **Allocation-free helpers** — `validate` is a well-formedness check for exactly
  one CBOR item (RFC 8949 §5.3.1, including text UTF-8),
  `serialized_size` computes the exact encoded size of any serializable
  value and `to_slice` encodes into a caller-provided buffer; none of them
  allocates heap memory.
* **Async item I/O** — the `async_io` module frames complete CBOR items on
  async byte streams, then reuses the normal synchronous serde API once an
  item is buffered. Bounded read helpers are available for untrusted streams.
* **A low-level header codec** — the `core` module exposes the pull/push
  `Header` interface for applications that need precise wire control.
* **`no_std` support** — `default-features = false, features = ["alloc"]`
  keeps the full API minus `std::io` interop and `HashMap` conversions;
  without `alloc` the crate still serializes (`to_writer`/`to_slice`/
  `serialized_size`), checks well-formedness and speaks the `core` header codec.

## Crate features

| Feature   | Default         | Effect                                                                                                                                               |
| --------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `std`     | yes             | Implements the `cbor2::io` traits for every `std::io::Read`/`Write`, adds `async_io`, and adds the `HashMap` conversions. Implies `alloc`.           |
| `alloc`   | yes (via `std`) | Everything needing a heap: `Value`, `to_vec`/`from_slice`/`from_reader`, `RawValue`, `diagnostic`, the deterministic encoders and the `cbor!` macro. |
| `cdn`     | no              | Adds the CDN input extensions that need external crates: `hash`, `cri` and `CRI`. Implies `alloc`.                                                |
| `derive`  | no              | The `#[derive(cbor2::Cbor)]` macro.                                                                                                                  |
| `futures` | no              | Adds `async_io::futures` helpers for `futures_io::AsyncRead`/`AsyncWrite`. Implies `std`.                                                            |
| `tokio`   | no              | Adds `async_io::tokio` helpers for `tokio::io::AsyncRead`/`AsyncWrite`. Implies `std`.                                                               |

With no features at all the crate is a `#![no_std]` core for constrained
targets: streaming serialization with `to_writer`/`to_slice`/
`serialized_size`, `validate`, the `tag` wrappers and the `core` header
codec. Deserializing through serde requires `alloc`. Readers and writers
implement the small `cbor2::io` traits, which are provided for byte slices
(and `Vec<u8>` with `alloc`):

```toml
[dependencies]
cbor2 = { version = "1", default-features = false } # or features = ["alloc"]
```

```rust
// Works on no_std + no alloc targets:
let mut buffer = [0u8; 64];
let item = cbor2::to_slice(&("id", 42u8), &mut buffer).unwrap();
assert!(cbor2::validate(&item[..]).is_ok());
```

## Guide

### Byte strings and `serde_bytes`

A common serde pitfall: bare `Vec<u8>` and `&[u8]` serialize as arrays of
integers, not as CBOR byte strings. Use
[`serde_bytes`](https://docs.rs/serde_bytes/latest/serde_bytes/) for binary
payloads.

```rust
let bytes = vec![1u8, 2, 3, 4];

// Bare Vec<u8>: [1, 2, 3, 4]
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "8401020304");

// serde_bytes: h'01020304'
let bytes = serde_bytes::ByteBuf::from(bytes);
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "4401020304");
```

For fields in derived structs, annotate byte buffers explicitly:

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

If you build data with `Value`, use `Value::Bytes(...)` or the `From`
implementations for byte slices/vectors; those already represent a CBOR
byte string.

### Borrowed deserialization from slices

`from_slice` is lifetime-aware: definite-length text and byte-string bodies
can be borrowed directly from the input. This matches serde_json's slice
path and is useful for signed payloads or COSE structures where the input
buffer already lives long enough.

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Packet<'a> {
    #[serde(borrow)]
    label: &'a str,
    #[serde(borrow, with = "serde_bytes")]
    payload: &'a [u8],
}

let bytes = hex::decode("a2656c6162656c626869677061796c6f616442dead").unwrap();
let packet: Packet<'_> = cbor2::from_slice(&bytes).unwrap();
assert_eq!(packet.label, "hi");
assert_eq!(packet.payload, &[0xde, 0xad]);
```

Indefinite-length strings are still accepted, but they cannot be borrowed
because their body is split across segments.

### COSE-style integer map keys, arrays and tags with `#[derive(Cbor)]`

With the `derive` feature, `#[derive(cbor2::Cbor)]` generates the serde
`Serialize`/`Deserialize` impls with CBOR protocol details: fields
annotated `#[cbor(key = ...)]` use integer map keys and the container is
wrapped in a CBOR tag (`#[cbor(tag = ...)]`) on encode. Tag layers are
transparent on decode, so the same type handles a protocol that travels both
tagged and untagged, instead of a second "bare" struct and a `From` impl.
Named structs can also use `#[cbor(array)]` to encode as a compact field-order
CBOR array while keeping Rust field names for JSON and code. Field names and
the type name stay untouched, so the same types still serialize to plain JSON.

```toml
[dependencies]
cbor2 = { version = "1", features = ["derive"] }
```

This reproduces the Simple Encrypted Message of
[RFC 9052, Appendix C.4.1](https://datatracker.ietf.org/doc/html/rfc9052#appendix-C.4)
byte for byte (52 bytes):

```rust
use cbor2::Cbor;

/// Protected header parameters (RFC 9052 §3.1). They travel as a byte
/// string holding their own CBOR encoding.
#[derive(Debug, PartialEq, Cbor)]
struct Protected {
    /// 10 = AES-CCM-16-64-128 (RFC 9053 §4.2)
    #[cbor(key = 1)]
    alg: i8,
}

/// Unprotected header parameters.
#[derive(Debug, PartialEq, Cbor)]
struct Unprotected {
    #[cbor(key = 5)]
    #[serde(with = "serde_bytes")]
    iv: Vec<u8>,
}

/// COSE_Encrypt0 (RFC 9052 §5.2): tag 16 around
/// `[protected: bstr, unprotected: map, ciphertext: bstr]`.
#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 16)]
struct CoseEncrypt0(
    #[serde(with = "serde_bytes")] Vec<u8>, // protected, already encoded
    Unprotected,
    #[serde(with = "serde_bytes")] Vec<u8>, // ciphertext
);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The protected header is the encoded map {1: 10}.
    let protected = cbor2::to_canonical_vec(&Protected { alg: 10 })?;
    assert_eq!(hex::encode(&protected), "a1010a");

    let msg = CoseEncrypt0(
        protected,
        Unprotected {
            iv: hex::decode("89f52f65a1c580933b5261a78c")?,
        },
        hex::decode("5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569")?,
    );

    // The RFC's 52-byte message, byte for byte.
    let bytes = cbor2::to_canonical_vec(&msg)?;
    assert_eq!(bytes.len(), 52);
    assert_eq!(
        hex::encode(&bytes),
        "d08343a1010aa1054d89f52f65a1c580933b5261a78c581c\
         5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569"
    );

    println!("{}", cbor2::to_cdn(&bytes[..])?);
    // 16([h'a1010a', {5: h'89f52f65a1c580933b5261a78c'},
    //     h'5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569'])

    // Decoding requires tag 16 and restores every layer.
    let back: CoseEncrypt0 = cbor2::from_slice(&bytes)?;
    assert_eq!(back, msg);
    let header: Protected = cbor2::from_slice(&back.0)?;
    assert_eq!(header, Protected { alg: 10 });

    // JSON stays natural — original field names, no tags, no integer keys.
    let json = serde_json::to_string(&header)?;
    assert_eq!(json, r#"{"alg":10}"#);
    Ok(())
}
```

The runnable [`examples/cose.rs`](examples/cose.rs) builds this out into the
actual wire types of [`cose2`](https://github.com/ldclabs/cose2) — a complete
RFC 9052 COSE and RFC 8392 CWT library built on cbor2 — with a named
`#[cbor(array)]` struct, an optional (detached) ciphertext and transparent tag
decoding so one type decodes both tagged and tag-less messages:
`cargo run --features derive --example cose`. The companion
[`examples/cwt.rs`](examples/cwt.rs) is cose2's CWT claims set (RFC 8392): a
tagged *map* with registered integer claim keys, natural JSON names,
`skip_serializing_if` claim omission, COSE-label-keyed `#[serde(flatten)]`
extension claims and the same transparent tag decoding. It also uses
`to_cdn_pretty_with_key_comments(&bytes[..], Claims::KEYS)` so the
diagnostic output stays true to the integer-keyed wire shape while showing
the matching string keys as code comments:

```text
61({
  1: "coap://as.example.com", // "iss"
  2: "erikw", // "sub"
  3: "coap://light.example.com", // "aud"
  4: 1444064944, // "exp"
  5: 1443944944, // "nbf"
  6: 1443944944, // "iat"
  7: h'0b71' // "cti"
})
```

Run it with `cargo run --features derive --example cwt`.

The derive also implements the `cbor2::Cbor` trait, which exposes the
declared protocol details at runtime — `T::KEYS`, `T::TAG` and `T::ARRAY` as
allocation-free constants, and `value.keys()` as a
`BTreeMap<String, i128>`:

```rust
use cbor2::Cbor; // one import: the derive macro and the trait

assert_eq!(Protected::KEYS, &[("alg", 1)]);
assert_eq!(CoseEncrypt0::TAG, Some(16));
assert!(!CoseEncrypt0::ARRAY);
```

For COSE structures whose wire shape is an array but whose Rust form should
keep named fields, add `#[cbor(array)]`:

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

let msg = Sign1 {
    protected: vec![0xa0],
    unprotected: 0,
    payload: vec![],
    signature: vec![0xff],
};

assert_eq!(hex::encode(cbor2::to_vec(&msg).unwrap()), "d28441a0004041ff");
assert!(Sign1::ARRAY);
```

### Dynamic values

```rust
use cbor2::{cbor, Simple, Value};

let value = cbor!({
    "code": 415,
    "message": null,
    "extra": { "numbers": [8.2341e+4, 0.251425] },
    (Simple::new(59).unwrap()) => [Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef])],
}).unwrap();

let bytes = cbor2::to_vec(&value).unwrap();
let back: Value = cbor2::from_slice(&bytes).unwrap();
assert_eq!(value, back);

let simple: Simple = cbor2::from_slice(&[0xf8, 0x3b]).unwrap();
assert_eq!(simple, Simple::new(59).unwrap());
```

### Raw values

`RawValue` defers decoding and preserves the exact wire bytes of one item
— the right tool for signature payloads:

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

### Tags

```rust
use cbor2::tag::RequireExact;

// Tag 0: standard date/time string.
let datetime = RequireExact::<String, 0>("2013-03-21T20:04:00Z".into());
let bytes = cbor2::to_vec(&datetime).unwrap();
assert_eq!(bytes[0], 0xc0);
```

### CBOR sequences

```rust
let mut stream = Vec::new();
cbor2::to_writer(&"first", &mut stream).unwrap();
cbor2::to_writer(&2u64, &mut stream).unwrap();

let items: Vec<cbor2::Value> = cbor2::de::Deserializer::from_reader(&stream[..])
    .into_iter()
    .collect::<Result<_, _>>()
    .unwrap();

assert_eq!(items, vec![cbor2::Value::from("first"), cbor2::Value::from(2)]);
assert!(cbor2::validate(&stream[..]).is_err()); // a sequence is not one item
```

### Async item I/O

Serde itself is synchronous, but async transports usually need item
boundaries. The `async_io` module reads one complete CBOR item into a
buffer, validates the same structure as `validate`, and then lets you call
`from_slice` on bytes that you own.

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::read_item(reader).await?;
let value: cbor2::Value = cbor2::from_slice(&item)?;
# Ok(())
# }
```

For untrusted peers, use `read_item_with_limit` or `read_value_with_limit`
unless an outer transport layer already enforces a message size limit:

```rust
# async fn bounded<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<cbor2::Value, cbor2::de::Error> {
let value: cbor2::Value = cbor2::async_io::read_value_with_limit(reader, 1 << 20).await?;
# Ok(value)
# }
```

Use `async_io::write_value` to serialize and send a value, or
`async_io::write_item` when you already have a validated single-item byte
buffer.

With the `futures` or `tokio` feature enabled, use the runtime-specific
adapters instead of writing a local wrapper:

```rust
# #[cfg(feature = "futures")]
# async fn futures_example<R: futures_io::AsyncRead + Unpin + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::futures::read_item(reader).await?;
# let _: cbor2::Value = cbor2::from_slice(&item)?;
# Ok(())
# }
#
# #[cfg(feature = "tokio")]
# async fn tokio_example<R: tokio::io::AsyncRead + Unpin + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::tokio::read_item(reader).await?;
# let _: cbor2::Value = cbor2::from_slice(&item)?;
# Ok(())
# }
```

### More examples

Runnable examples live in `examples/`:

```bash
cargo run --example basic
cargo run --example bytes
cargo run --example sequence
cargo run --example core_headers
cargo run --features derive --example cose
cargo run --features derive --example cwt
```

## Design decisions

This implementation deliberately matches ciborium's wire behavior, so the
two crates interoperate byte for byte:

* Numbers always encode in their smallest lossless form, as deterministic
  encoding (RFC 8949 §4.2.1) requires. Integer width in Rust is treated as
  an in-memory detail, not a wire property.
* Enums encode as a bare string (unit variants) or a single-entry map
  `{variant: payload}` (everything else).
* `Value` maps are `Vec<(Value, Value)>`, preserving wire order and
  arbitrary keys.
* Decoding follows the robustness principle: indefinite lengths, segmented
  strings, half-width floats and unknown tags are accepted even though
  encoding never produces them.

## History

This project descends from the `cbor` crate created by
[Andrew Gallant](https://github.com/BurntSushi) in 2015, which was built on
the pre-serde `rustc-serialize` framework and went unmaintained for many
years. Version 0.5 was a from-scratch rewrite on top of
[serde](https://serde.rs), maintained by [LDC Labs](https://github.com/ldclabs)
and published as **`cbor2`** — the `cbor` name on crates.io stays with the
legacy 0.4 release — and 1.0 stabilizes it. None of the 0.4 API survives.

The rewrite follows the design of (and is wire-compatible with)
[ciborium](https://github.com/enarx/ciborium) — many thanks to its authors.

## Command line tool

The workspace ships a `cbor` command line tool in
[`cbor2-cli`](cbor2-cli/README.md). Bare `cbor` shows any CBOR — from a
file, stdin, a hex string or a base64 string — as diagnostic notation
(RFC 8949 §8, formalized as CDN); `decode` shows pretty diagnostic notation
by default and converts to pretty JSON with `--json`, `encode` converts
JSON-compatible values or CDN text to CBOR, `encode --json` forces strict JSON
input, `encode --diag`/`--cdn` force CDN input, `encode --hex` prints copyable
CBOR hex for agents and docs, and `validate` checks complete CBOR input:

```bash
brew install ldclabs/tap/cbor2-cli   # Homebrew
cargo install cbor2-cli              # Cargo
```

```bash
$ cbor bf61610161629f0203ffff
{_ "a": 1, "b": [_ 2, 3]}

$ echo '{"name": "example", "ok": true}' | cbor encode --json | cbor decode --json
{
  "name": "example",
  "ok": true
}

$ echo '{"name": "example", "ok": true}' | cbor encode --hex
a2646e616d65676578616d706c65626f6bf5

$ printf "bytes<<\"hi\", h'2f'>>" | cbor encode --diag --hex
4368692f

$ cbor validate a2646e616d65676578616d706c65626f6bf5
valid
```

## Testing

`cargo test` runs the unit tests, a single integration-test binary and the
doc tests — including the RFC 8949 Appendix A vectors and fault-injection
tests for I/O failures and malformed input. CI builds and tests every
feature combination, down to a bare-metal `no_std` target. Coverage
measured with `cargo llvm-cov` is 100% of functions and about 98% of
lines; the only never-executed lines are defensive branches that cannot
occur, such as error paths that the `RawValue` validity invariant rules
out.

## Minimum supported Rust version

Rust 1.89.

## License

Licensed under the MIT License.
