# cbor2

A serde implementation of [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949)
— the Concise Binary Object Representation (CBOR) — for Rust.

[![CI](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml/badge.svg)](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cbor2.svg)](https://crates.io/crates/cbor2)
[![docs.rs](https://docs.rs/cbor2/badge.svg)](https://docs.rs/cbor2)

CBOR adopts and modestly builds on the *data model* used by JSON, except the
encoding is in binary form. Its primary goals include a balance of
implementation size, message size and extensibility.

Dual-licensed under MIT or the [UNLICENSE](http://unlicense.org).

## Status

This project descends from the `cbor` crate created by
[Andrew Gallant](https://github.com/BurntSushi) in 2015, which was built on
the pre-serde `rustc-serialize` framework and went unmaintained for many
years. Version 0.5 is a from-scratch rewrite on top of
[serde](https://serde.rs), maintained by [LDC Labs](https://github.com/ldclabs)
and published as **`cbor2`** — the `cbor` name on crates.io stays with the
legacy 0.4 release. None of the 0.4 API survives.

The rewrite follows the design of (and is wire-compatible with)
[ciborium](https://github.com/enarx/ciborium) — many thanks to its authors.
If you need `no_std` support today, use ciborium; this crate currently
requires `std`.

## Features

* **Full serde integration** — `#[derive(Serialize, Deserialize)]` types
  encode and decode directly.
* **RFC 8949 preferred serialization** — integers and floats are always
  encoded in their smallest lossless form, including half-precision floats.
* **A dynamic [`Value`] type** — the CBOR analogue of `serde_json::Value`,
  with a `cbor!` macro for building values in JSON-like syntax.
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
* **Integer map keys (COSE)** — with the `derive` feature, the
  `#[cbor2::int_keys]` attribute macro maps struct fields to integer keys
  (`#[cbor(key = 1)]`), as RFC 9052 requires, with no ambiguity against
  textual keys; `alias` and the other serde field attributes work as
  usual.
* **Robust decoding** — indefinite-length items, segmented strings,
  duplicate map keys, unknown tags and CBOR sequences (RFC 8742) are all
  handled; recursion is depth-limited and forged lengths cannot trigger
  huge allocations.
* **Diagnostic notation** — `diagnostic` renders raw CBOR as the
  human-readable text of RFC 8949 §8 (matching the Appendix A examples
  exactly, indefinite-length markers and all); `Value` implements
  `Display` with the same notation and `Debug` as its indented,
  multi-line form.
* **Allocation-free helpers** — `validate` checks that an input is exactly
  one well-formed CBOR item (RFC 8949 §5.3.1, including text UTF-8) and
  `serialized_size` computes the exact encoded size of any serializable
  value; neither allocates heap memory.
* **A low-level header codec** — the `core` module exposes the pull/push
  `Header` interface for applications that need precise wire control.

## Usage

```toml
[dependencies]
cbor2 = "0.5"
```

### Type-based encoding and decoding

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

### Dynamic values

```rust
use cbor2::{cbor, Value};

let value = cbor!({
    "code" => 415,
    "message" => null,
    "extra" => { "numbers" => [8.2341e+4, 0.251425] },
}).unwrap();

let bytes = cbor2::to_vec(&value).unwrap();
let back: Value = cbor2::from_slice(&bytes).unwrap();
assert_eq!(value, back);
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

### More examples

Runnable examples live in `examples/`:

```bash
cargo run --example basic
cargo run --example bytes
cargo run --example sequence
cargo run --example core_headers
cargo run --features derive --example cose_int_keys
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

## Command line tools

The workspace ships two small converters in `cbor_conv`:

```bash
$ echo '{"name": "example", "ok": true}' | json2cbor | cbor2json
{
  "name": "example",
  "ok": true
}
```

## Roadmap

* `no_std` + `alloc` support
* Benchmarks against other CBOR implementations

## Testing

`cargo test` runs the unit tests, a single integration-test binary and the
doc tests — including the RFC 8949 Appendix A vectors and fault-injection
tests for I/O failures and malformed input. Line coverage of the library is
100% (measured with `cargo llvm-cov`); the only never-executed regions are
five error branches that are unreachable on 64-bit targets or guard
conditions that cannot occur.

## Minimum supported Rust version

Rust 1.85.

## License

Dual-licensed under MIT or the [UNLICENSE](http://unlicense.org), like the
original crate.
