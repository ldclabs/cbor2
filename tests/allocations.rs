//! Count allocations in the calling thread only, so the test harness and
//! concurrently running tests cannot affect these memory contracts.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static COUNTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}
struct Counting;
fn record(bytes: usize) {
    let _ = COUNTS.try_with(|counts| {
        if let Some((count, total)) = counts.get() {
            counts.set(Some((count + 1, total + bytes)));
        }
    });
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn measured<T>(operation: impl FnOnce() -> T) -> (T, (usize, usize)) {
    COUNTS.with(|counts| counts.set(Some((0, 0))));
    let value = operation();
    let counts = COUNTS.with(|counts| counts.replace(None).unwrap());
    (value, counts)
}

#[test]
fn raw_writes_and_sizing_do_not_allocate() {
    let payload = vec![0x55u8; 1024 * 1024];
    let raw = cbor2::RawValue::serialized(serde_bytes::Bytes::new(&payload)).unwrap();
    let mut output = vec![0; raw.as_bytes().len()];
    let (size, counts) = measured(|| cbor2::serialized_size(&raw).unwrap());
    assert_eq!(size as usize, output.len());
    assert_eq!(counts, (0, 0));
    let (length, counts) = measured(|| cbor2::to_slice(&raw, &mut output).unwrap().len());
    assert_eq!(length, output.len());
    assert_eq!(output, raw.as_bytes());
    assert_eq!(counts, (0, 0));
    let (captured, counts) = measured(|| cbor2::from_slice::<cbor2::RawValue>(&output).unwrap());
    assert_eq!(captured, raw);
    assert_eq!(counts, (1, output.len()));
}

#[cfg(feature = "derive")]
#[test]
fn value_array_allocates_one_container() {
    #[derive(cbor2::Cbor)]
    #[cbor(array)]
    struct Four {
        a: u64,
        b: u64,
        c: u64,
        d: u64,
    }
    let value = Four {
        a: 1,
        b: 2,
        c: 3,
        d: 4,
    };
    let (array, counts) = measured(|| cbor2::Value::serialized(&value).unwrap());
    assert_eq!(array, cbor2::cbor!([1, 2, 3, 4]).unwrap());
    assert_eq!(counts, (1, 4 * std::mem::size_of::<cbor2::Value>()));
}

#[test]
fn cli_style_validation_does_not_buffer_item_payload() {
    let payload = vec![0u8; 1024 * 1024];
    let bytes = cbor2::to_vec(serde_bytes::Bytes::new(&payload)).unwrap();
    let (_, counts) = measured(|| {
        let mut iter = cbor2::de::Deserializer::from_reader(bytes.as_slice())
            .into_iter::<serde::de::IgnoredAny>();
        assert!(iter.next().unwrap().is_ok());
        assert!(iter.next().is_none());
    });
    assert_eq!(counts, (0, 0));
}

#[test]
fn value_array_to_vec_allocates_once() {
    for length in [16, 128, 1024] {
        let values: Vec<u64> = (0..length).collect();
        let value = cbor2::Value::serialized(&values).unwrap();
        let (decoded, counts) = measured(|| value.deserialized::<Vec<u64>>().unwrap());
        assert_eq!(decoded, values);
        assert_eq!(counts, (1, length as usize * std::mem::size_of::<u64>()));
    }
}

#[test]
fn pretty_indefinite_map_only_allocates_output() {
    let mut bytes = vec![0xbf];
    for n in 0..128u64 {
        cbor2::to_writer(&n, &mut bytes).unwrap();
        cbor2::to_writer(&n, &mut bytes).unwrap();
    }
    bytes.push(0xff);
    let value: cbor2::Value = cbor2::from_slice(&bytes).unwrap();
    let definite = cbor2::to_vec(&value).unwrap();
    let (actual, counts) = measured(|| cbor2::to_cdn_pretty(bytes.as_slice()).unwrap());
    let (expected, definite_counts) =
        measured(|| cbor2::to_cdn_pretty(definite.as_slice()).unwrap());
    assert_eq!(actual.replacen("{_", "{", 1), expected);
    assert_eq!(counts.0, definite_counts.0);
}

#[test]
fn large_arrays_of_one_byte_items_do_not_overreserve() {
    // The per-element capacity hint is capped for long arrays, so the
    // buffer stays within a few KiB of the encoding instead of four times.
    for len in [10_000usize, 100_000] {
        let bytes = cbor2::to_vec(&vec![true; len]).unwrap();
        assert!(bytes.len() > len);
        assert!(
            bytes.capacity() <= bytes.len() + 4096 + 9,
            "{len}: {}",
            bytes.capacity()
        );
    }
}

#[test]
fn dynamic_containers_bound_capacity_without_a_threshold_cliff() {
    for n in [1023usize, 1024, 1025] {
        for map in [false, true] {
            let mut input = vec![if map { 0xb9 } else { 0x99 }];
            input.extend_from_slice(&(n as u16).to_be_bytes());
            input.resize(3 + n * if map { 2 } else { 1 }, 0);
            let (_, (calls, _)) = measured(|| cbor2::from_slice::<cbor2::Value>(&input).unwrap());
            assert_eq!(calls, if n <= 1024 { 1 } else { 2 });
            let (err, (_, bytes)) = measured(|| cbor2::from_slice::<cbor2::Value>(&input[..3]));
            assert!(err.is_err());
            assert!(bytes < 1024);
        }
    }
}

#[test]
fn ordinary_cdn_integers_allocate_only_container_buffers() {
    for literal in ["42", "-7"] {
        let source = format!("[{}]", vec![literal; 1000].join(","));
        let (_, (calls, _)) = measured(|| cbor2::cdn_to_vec(&source).unwrap());
        assert!(calls < 24, "{literal}: {calls}");
    }
}
