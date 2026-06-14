# cbor2-cli

Inspect, convert and debug CBOR
([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949)) from the terminal. This
crate installs the `cbor` command, built on
[`cbor2`](https://crates.io/crates/cbor2).

```bash
cargo install cbor2-cli   # installs the `cbor` binary
```

Or install it from the ldclabs Homebrew tap:

```bash
brew install ldclabs/tap/cbor2-cli   # installs the `cbor` binary
```

```text
Usage: cbor [COMMAND] [INPUT]

Commands:
  (none)  Show each CBOR item as one line of diagnostic notation (§8)
  decode  Convert CBOR items to pretty-printed JSON, or to
          pretty-printed diagnostic notation with --diag
  encode  Convert JSON values to CBOR items
```

## Why cbor2-cli

| Need                  | Command support                                                                                                                      |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Inspect pasted CBOR   | Run `cbor <hex-or-base64>` to render RFC 8949 diagnostic notation.                                                                   |
| Preserve wire details | Bare `cbor` captures each item as raw bytes, so indefinite lengths, segmented strings, `undefined` and simple values remain visible. |
| Decode for JSON tools | `cbor decode` pretty-prints CBOR as JSON, one document per item.                                                                     |
| Encode fixtures       | `cbor encode` turns JSON values into CBOR bytes and supports JSON value streams.                                                     |
| Work with sequences   | Multiple JSON values become a CBOR sequence; CBOR sequences decode item by item.                                                     |
| Script reliably       | Data errors exit with status 1, usage errors with status 2.                                                                          |

`INPUT` is a file path, a hex string (optionally `0x`-prefixed), a
base64/base64url string, or `-` for stdin; stdin is the default. An
argument containing a path separator is always a file path. Everything
streams: multiple JSON values become a CBOR sequence (RFC 8742), and a
CBOR sequence becomes one output document or line per item. Data errors
exit with status 1, usage errors with status 2.

## Show: `cbor`

The everyday command. It renders each item as the human-readable text
form of RFC 8949 §8 — what CBOR specs and test vectors are written in —
and it is exact: every item is captured as its wire bytes, so
indefinite-length items keep their `_` markers, segmented strings appear
as `(_ ...)`, `undefined` and unassigned simple values appear as
themselves, byte strings render as `h'...'` and bignums print as plain
integers, exactly as in RFC 8949 Appendix A.

```bash
$ cbor a201020326                  # hex, pasted straight from a spec
{1: 2, 3: -7}

$ cbor 0x8301820203820405          # 0x-prefixed works too
[1, [2, 3], [4, 5]]

$ cbor oWFhAQ                      # base64url, padded or not
{"a": 1}

$ cbor message.cbor                # a file
16([h'a1010a', {5: h'89f52f65a1c580933b5261a78c'}, h'5974e1b9...'])

$ cbor bf61610161629f0203ffff      # wire details survive
{_ "a": 1, "b": [_ 2, 3]}
```

## decode

`cbor decode` decodes each item into the data model and pretty-prints it
as JSON, or — with `-d`/`--diag` — as indented diagnostic notation.
Unlike the bare `cbor`, this re-spells the item (indefinite lengths and
non-preferred encodings are not preserved).

```bash
$ cbor decode a1018202036466697665f5
{
  "1": [
    2,
    3
  ]
}
"five"
true

$ cbor decode --diag a101820203
{
  1: [
    2,
    3
  ]
}
```

JSON conversion is best-effort where CBOR is richer: byte strings become
lowercase hex strings, non-string map keys are JSON-encoded into strings,
non-finite floats and `undefined` become `null`, integers beyond the
64-bit ranges become strings, and tags are dropped (keeping the inner
value).

## encode

`cbor encode` reads JSON text (from a file or stdin) and writes each
value as a CBOR item:

```bash
$ echo '{"name": "example", "ok": true}' | cbor encode | cbor
{"name": "example", "ok": true}

$ echo '{"name": "example", "ok": true}' | cbor encode | xxd -p
a2646e616d65676578616d706c65626f6bf5
```

## License

Dual-licensed under MIT or the [UNLICENSE](http://unlicense.org).
