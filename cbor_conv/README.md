# cbor_conv

Command line tools for converting between CBOR and JSON, built on the
[`cbor2`](https://crates.io/crates/cbor2) crate.

* `json2cbor` reads a stream of JSON values on stdin and writes each as a
  CBOR item to stdout.
* `cbor2json` reads a stream of CBOR items on stdin and writes each as a
  pretty-printed JSON value to stdout. Byte strings become hex strings,
  non-string map keys are JSON-encoded into strings, non-finite floats
  become null and tags are dropped (keeping the inner value).

Both tools are streaming converters: multiple JSON values become a CBOR
sequence, and a CBOR sequence becomes multiple pretty-printed JSON documents.

```bash
$ echo '{"name": "example", "ok": true}' | json2cbor | cbor2json
{
  "name": "example",
  "ok": true
}
```

```bash
$ printf '%s\n%s\n' '1' '{"two":2}' | json2cbor | cbor2json
1
{
  "two": 2
}
```
