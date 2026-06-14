//! Prints the encoded byte size each crate produces for every fixture.
//!
//! Run with `cargo run --release --bin sizes`. The serde crates emit
//! text-keyed maps for the log batch while minicbor emits a compact
//! integer-keyed array, so the `log_batch` column is where sizes diverge.

use cbor2_bench::*;
use serde_bytes::ByteBuf;

fn ciborium_len<T: serde::Serialize>(v: &T) -> usize {
    let mut buf = Vec::new();
    ciborium::into_writer(v, &mut buf).unwrap();
    buf.len()
}

fn main() {
    let ints = int_array(INT_ARRAY_LEN);
    let logs = log_batch(LOG_BATCH_LEN);
    let logs_mini = log_batch_mini(&logs);
    let raw = blob(BLOB_LEN);
    let blob_serde = ByteBuf::from(raw.clone());
    let blob_mini = minicbor::bytes::ByteVec::from(raw);

    println!(
        "{:<14} {:>12} {:>12} {:>12}",
        "crate", "int_array", "log_batch", "blob"
    );
    println!("{}", "-".repeat(54));

    let row = |name: &str, a: usize, b: usize, c: usize| {
        println!("{name:<14} {a:>12} {b:>12} {c:>12}");
    };

    row(
        "cbor2",
        cbor2::to_vec(&ints).unwrap().len(),
        cbor2::to_vec(&logs).unwrap().len(),
        cbor2::to_vec(&blob_serde).unwrap().len(),
    );
    row(
        "ciborium",
        ciborium_len(&ints),
        ciborium_len(&logs),
        ciborium_len(&blob_serde),
    );
    row(
        "serde_cbor",
        serde_cbor::to_vec(&ints).unwrap().len(),
        serde_cbor::to_vec(&logs).unwrap().len(),
        serde_cbor::to_vec(&blob_serde).unwrap().len(),
    );
    row(
        "serde_cbor_2",
        serde_cbor_2::to_vec(&ints).unwrap().len(),
        serde_cbor_2::to_vec(&logs).unwrap().len(),
        serde_cbor_2::to_vec(&blob_serde).unwrap().len(),
    );
    row(
        "minicbor",
        minicbor::to_vec(&ints).unwrap().len(),
        minicbor::to_vec(&logs_mini).unwrap().len(),
        minicbor::to_vec(&blob_mini).unwrap().len(),
    );
}
