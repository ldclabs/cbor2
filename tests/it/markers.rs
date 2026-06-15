//! Integer map keys and struct tags through the `@@CBOR@@` container
//! marker protocol. These tests write the marker by hand to exercise the
//! library without the `derive` feature; the `cose` module covers
//! `#[derive(cbor2::Cbor)]`.

use cbor2::Value;
use serde::{Deserialize, Serialize};

// What #[derive(Cbor)] effectively expands to: only the *container* is
// renamed; field names stay as written.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename = "@@CBOR@@@@kty=1;alg=3;crv=-1;x=-2@@CoseKey")]
struct CoseKey {
    kty: u8,
    alg: i8,
    crv: u8,
    x: serde_bytes::ByteBuf,
}

fn sample() -> CoseKey {
    CoseKey {
        kty: 2,
        alg: -7,
        crv: 1,
        x: serde_bytes::ByteBuf::from(vec![0x11, 0x22, 0x33, 0x44]),
    }
}

#[test]
fn marked_fields_become_integer_keys() {
    // {1: 2, 3: -7, -1: 1, -2: h'11223344'}
    let bytes = cbor2::to_vec(&sample()).unwrap();
    assert_eq!(hex::encode(&bytes), "a4010203262001214411223344");

    let back: CoseKey = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(back, sample());

    // The same bytes decode through a Value, and Value::serialized
    // produces integer keys too.
    let value: Value = cbor2::from_slice(&bytes).unwrap();
    let keys: Vec<&Value> = value.as_map().unwrap().iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        [
            &Value::from(1),
            &Value::from(3),
            &Value::from(-1),
            &Value::from(-2)
        ]
    );
    assert_eq!(Value::serialized(&sample()).unwrap(), value);
    assert_eq!(value.deserialized::<CoseKey>().unwrap(), sample());

    // Canonical encoding sorts integer keys like any other key.
    let canonical = cbor2::to_canonical_vec(&sample()).unwrap();
    assert_eq!(hex::encode(&canonical), "a4010203262001214411223344");
}

#[test]
fn text_keys_still_decode() {
    // The wire may also carry the field names as text keys — JSON-shaped
    // input, hand-built values — and both forms may mix.
    let mixed = cbor2::cbor!({
        1 => 2,
        "alg" => -7,
        -1 => 1,
        "x" => cbor2::Value::Bytes(vec![0x11, 0x22, 0x33, 0x44]),
    })
    .unwrap();
    let bytes = cbor2::to_vec(&mixed).unwrap();
    assert_eq!(cbor2::from_slice::<CoseKey>(&bytes).unwrap(), sample());
    assert_eq!(mixed.deserialized::<CoseKey>().unwrap(), sample());
}

#[test]
fn json_uses_the_field_names() {
    // The container rename is invisible to serde_json.
    let json = serde_json::to_string(&sample()).unwrap();
    assert_eq!(json, r#"{"kty":2,"alg":-7,"crv":1,"x":[17,34,51,68]}"#);
    assert_eq!(serde_json::from_str::<CoseKey>(&json).unwrap(), sample());
}

#[test]
fn plain_numeric_names_stay_text() {
    // Without a marker there is no ambiguity: a numeric-looking field
    // name is a text key, exactly as in ciborium.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Plain {
        #[serde(rename = "1")]
        a: u8,
    }

    let bytes = cbor2::to_vec(&Plain { a: 7 }).unwrap();
    assert_eq!(hex::encode(&bytes), "a1613107"); // {"1": 7}
    assert_eq!(cbor2::from_slice::<Plain>(&bytes).unwrap(), Plain { a: 7 });
}

#[test]
fn unknown_integer_keys_are_ignored() {
    let extra = cbor2::cbor!({
        1 => 2,
        3 => -7,
        -1 => 1,
        -2 => cbor2::Value::Bytes(vec![0x11, 0x22, 0x33, 0x44]),
        99 => ["ignored", {"deep" => null}],
        -99 => "also ignored",
    })
    .unwrap();
    let bytes = cbor2::to_vec(&extra).unwrap();
    assert_eq!(cbor2::from_slice::<CoseKey>(&bytes).unwrap(), sample());
    assert_eq!(extra.deserialized::<CoseKey>().unwrap(), sample());
}

#[test]
fn marked_tags_wrap_and_decode_transparently() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@123@@alg=1@@Header")]
    struct Header {
        alg: i8,
    }

    let bytes = cbor2::to_vec(&Header { alg: -7 }).unwrap();
    assert_eq!(hex::encode(&bytes), "d87ba10126"); // 123({1: -7})
    assert_eq!(
        cbor2::from_slice::<Header>(&bytes).unwrap(),
        Header { alg: -7 }
    );
    assert_eq!(cbor2::to_canonical_vec(&Header { alg: -7 }).unwrap(), bytes);

    // Decode accepts the tagged form, the untagged form, and foreign wrapper
    // tags transparently.
    assert_eq!(
        cbor2::from_slice::<Header>(&hex::decode("a10126").unwrap()).unwrap(),
        Header { alg: -7 }
    );
    assert_eq!(
        cbor2::from_slice::<Header>(&hex::decode("d87ca10126").unwrap()).unwrap(),
        Header { alg: -7 }
    );
    assert_eq!(
        cbor2::from_slice::<Header>(&hex::decode("d9d9f7d87ba10126").unwrap()).unwrap(),
        Header { alg: -7 }
    );

    // The Value paths agree.
    let value = Value::serialized(&Header { alg: -7 }).unwrap();
    assert_eq!(
        value,
        Value::Tag(123, Box::new(cbor2::cbor!({ 1 => -7 }).unwrap()))
    );
    assert_eq!(value.deserialized::<Header>().unwrap(), Header { alg: -7 });
    assert_eq!(
        cbor2::cbor!({ 1 => -7 })
            .unwrap()
            .deserialized::<Header>()
            .unwrap(),
        Header { alg: -7 }
    );
}

#[test]
fn marked_tags_accept_tagged_or_untagged() {
    // A marker tag is still written on encode, but decode accepts input with
    // or without it.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@61@@iss=1@@Claims")]
    struct Claims {
        iss: u8,
    }

    // Encode still writes the tag, canonically.
    let bytes = cbor2::to_vec(&Claims { iss: 7 }).unwrap();
    assert_eq!(hex::encode(&bytes), "d83da10107"); // 61({1: 7})
    assert_eq!(cbor2::to_canonical_vec(&Claims { iss: 7 }).unwrap(), bytes);

    // Decode accepts the tagged form, the untagged form, and a foreign
    // wrapper tag (transparent, as everywhere else).
    assert_eq!(
        cbor2::from_slice::<Claims>(&bytes).unwrap(),
        Claims { iss: 7 }
    );
    assert_eq!(
        cbor2::from_slice::<Claims>(&hex::decode("a10107").unwrap()).unwrap(),
        Claims { iss: 7 }
    );
    assert_eq!(
        cbor2::from_slice::<Claims>(&hex::decode("d9d9f7a10107").unwrap()).unwrap(),
        Claims { iss: 7 }
    );

    // The Value paths agree, on both forms.
    let value = Value::serialized(&Claims { iss: 7 }).unwrap();
    assert_eq!(
        value,
        Value::Tag(61, Box::new(cbor2::cbor!({ 1 => 7 }).unwrap()))
    );
    assert_eq!(value.deserialized::<Claims>().unwrap(), Claims { iss: 7 });
    assert_eq!(
        cbor2::cbor!({ 1 => 7 })
            .unwrap()
            .deserialized::<Claims>()
            .unwrap(),
        Claims { iss: 7 }
    );
}

#[test]
fn marked_tags_on_tuple_structs_decode_untagged() {
    // The transparent tag behavior also covers tuple structs (array shape),
    // exercising the `unwrap_struct_tag` path on the Value side.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@16@@@@Pair")]
    struct Pair(u8, u8);

    let bytes = cbor2::to_vec(&Pair(1, 2)).unwrap();
    assert_eq!(hex::encode(&bytes), "d0820102"); // 16([1, 2])
    assert_eq!(cbor2::from_slice::<Pair>(&bytes).unwrap(), Pair(1, 2));
    assert_eq!(
        cbor2::from_slice::<Pair>(&hex::decode("820102").unwrap()).unwrap(),
        Pair(1, 2)
    );

    let value = Value::serialized(&Pair(1, 2)).unwrap();
    assert_eq!(
        value,
        Value::Tag(16, Box::new(Value::Array(vec![1.into(), 2.into()])))
    );
    assert_eq!(value.deserialized::<Pair>().unwrap(), Pair(1, 2));
    assert_eq!(
        Value::Array(vec![1.into(), 2.into()])
            .deserialized::<Pair>()
            .unwrap(),
        Pair(1, 2)
    );
}

#[test]
fn marked_named_structs_can_encode_as_arrays() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@18@@@@array@@Sign1")]
    struct Sign1 {
        protected: serde_bytes::ByteBuf,
        unprotected: u8,
        payload: serde_bytes::ByteBuf,
        signature: serde_bytes::ByteBuf,
    }

    let msg = Sign1 {
        protected: serde_bytes::ByteBuf::from(vec![0xa0]),
        unprotected: 0,
        payload: serde_bytes::ByteBuf::from(vec![]),
        signature: serde_bytes::ByteBuf::from(vec![0xff]),
    };

    let bytes = cbor2::to_vec(&msg).unwrap();
    assert_eq!(hex::encode(&bytes), "d28441a0004041ff");
    assert_eq!(cbor2::from_slice::<Sign1>(&bytes).unwrap(), msg);

    let value = Value::serialized(&msg).unwrap();
    assert_eq!(
        value,
        Value::Tag(
            18,
            Box::new(Value::Array(vec![
                Value::Bytes(vec![0xa0]),
                Value::from(0),
                Value::Bytes(vec![]),
                Value::Bytes(vec![0xff]),
            ]))
        )
    );
    assert_eq!(value.deserialized::<Sign1>().unwrap(), msg);

    let untagged = hex::decode("8441a0004041ff").unwrap();
    assert_eq!(cbor2::from_slice::<Sign1>(&untagged).unwrap(), msg);
    assert!(cbor2::cbor!({ "protected" => 1 })
        .unwrap()
        .deserialized::<Sign1>()
        .unwrap_err()
        .to_string()
        .contains("invalid type"));
}

#[test]
fn only_canonical_markers_take_effect() {
    // A frame or tag segment that does not parse leaves an ordinary
    // container name: no tag, no key table.
    #[derive(Serialize)]
    #[serde(rename = "@@CBOR@@x7@@alg=1@@Bad")]
    struct BadTag {
        alg: i8,
    }

    let value = Value::serialized(&BadTag { alg: -7 }).unwrap();
    assert_eq!(value, cbor2::cbor!({ "alg" => -7 }).unwrap());

    // Within a valid frame, only canonical decimal entries map fields;
    // the rest keep their text keys.
    #[derive(Serialize)]
    #[serde(rename = "@@CBOR@@@@a=1;b=01;c=-0;d=+2;e=2x;f=18446744073709551616;g@@Odd")]
    struct Oddballs {
        a: u8,
        b: u8,
        c: u8,
        d: u8,
        e: u8,
        f: u8,
        g: u8,
        h: u8, // not in the table at all
    }

    let oddballs = Oddballs {
        a: 0,
        b: 1,
        c: 2,
        d: 3,
        e: 4,
        f: 5,
        g: 6,
        h: 7,
    };
    let value = Value::serialized(&oddballs).unwrap();
    let keys: Vec<&Value> = value.as_map().unwrap().iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        [
            &Value::from(1),
            &Value::from("b"),
            &Value::from("c"),
            &Value::from("d"),
            &Value::from("e"),
            &Value::from("f"),
            &Value::from("g"),
            &Value::from("h"),
        ]
    );

    // The streaming serializer agrees with the Value serializer.
    let direct = cbor2::to_vec(&oddballs).unwrap();
    assert_eq!(direct, cbor2::to_vec(&value).unwrap());

    // Edge tags and keys work.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(
        rename = "@@CBOR@@18446744073709551615@@hi=18446744073709551615;lo=-18446744073709551616;zero=0@@Edges"
    )]
    struct Edges {
        hi: u8,
        lo: u8,
        zero: u8,
    }

    let edges = Edges {
        hi: 1,
        lo: 2,
        zero: 0,
    };
    let bytes = cbor2::to_vec(&edges).unwrap();
    assert_eq!(
        hex::encode(&bytes),
        "dbffffffffffffffffa31bffffffffffffffff013bffffffffffffffff020000"
    );
    assert_eq!(cbor2::from_slice::<Edges>(&bytes).unwrap(), edges);
}

#[test]
fn struct_variants_use_integer_keys_too() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@@@payload=1@@Message")]
    enum Message {
        Signed { payload: u8 },
    }

    let bytes = cbor2::to_vec(&Message::Signed { payload: 7 }).unwrap();
    // {"Signed": {1: 7}}
    assert_eq!(hex::encode(&bytes), "a1665369676e6564a10107");
    assert_eq!(
        cbor2::from_slice::<Message>(&bytes).unwrap(),
        Message::Signed { payload: 7 }
    );
    assert_eq!(
        Value::serialized(&Message::Signed { payload: 7 })
            .unwrap()
            .deserialized::<Message>()
            .unwrap(),
        Message::Signed { payload: 7 }
    );
}

#[test]
fn non_identifier_keys_are_rejected() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct F {
        a: u8,
    }

    // A float key cannot name a field, on either path.
    let msg = cbor2::from_slice::<F>(&hex::decode("a1f93c0001").unwrap())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("str, bytes or an integer"), "{msg}");

    let value = Value::Map(vec![(Value::Float(1.0), Value::from(1))]);
    let msg = value.deserialized::<F>().unwrap_err().to_string();
    assert!(msg.contains("str or integer"), "{msg}");
}

#[test]
fn tagged_integer_keys_still_match() {
    // A tag wrapped around an integer key is transparent, like elsewhere.
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename = "@@CBOR@@@@a=1@@K")]
    struct K {
        a: u8,
    }

    let bytes = hex::decode("a1c10107").unwrap(); // {1(1): 7}
    assert_eq!(cbor2::from_slice::<K>(&bytes).unwrap(), K { a: 7 });

    let value = Value::Map(vec![(
        Value::Tag(9, Box::new(Value::from(1))),
        Value::from(7),
    )]);
    assert_eq!(value.deserialized::<K>().unwrap(), K { a: 7 });
}

#[test]
fn marker_write_failures_propagate() {
    struct Limited(usize);

    impl std::io::Write for Limited {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::other("limit"));
            }
            let n = self.0.min(data.len());
            self.0 -= n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[derive(Serialize)]
    #[serde(rename = "@@CBOR@@9@@a=1;b=-1@@T")]
    struct T {
        a: u8,
        b: u8,
    }

    // The tag does not fit; then the map header fits but a key does not.
    assert!(matches!(
        cbor2::to_writer(&T { a: 1, b: 2 }, Limited(0)),
        Err(cbor2::ser::Error::Io(..))
    ));
    assert!(matches!(
        cbor2::to_writer(&T { a: 1, b: 2 }, Limited(2)),
        Err(cbor2::ser::Error::Io(..))
    ));
}

#[test]
fn marked_containers_carry_tags_in_every_shape() {
    // Unit, newtype and tuple structs with a marker tag, on both the
    // stream and the Value paths.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@71@@@@U")]
    struct U;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@72@@@@N")]
    struct N(u8);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@73@@@@T")]
    struct T(u8, u8);

    let bytes = cbor2::to_vec(&U).unwrap();
    assert_eq!(hex::encode(&bytes), "d847f6"); // 71(null)
    assert_eq!(cbor2::from_slice::<U>(&bytes).unwrap(), U);

    let bytes = cbor2::to_vec(&N(7)).unwrap();
    assert_eq!(hex::encode(&bytes), "d84807"); // 72(7)
    assert_eq!(cbor2::from_slice::<N>(&bytes).unwrap(), N(7));

    let bytes = cbor2::to_vec(&T(1, 2)).unwrap();
    assert_eq!(hex::encode(&bytes), "d849820102"); // 73([1, 2])
    assert_eq!(cbor2::from_slice::<T>(&bytes).unwrap(), T(1, 2));

    // The Value paths agree.
    let value = Value::serialized(&U).unwrap();
    assert_eq!(value, Value::Tag(71, Box::new(Value::Null)));
    assert_eq!(value.deserialized::<U>().unwrap(), U);

    let value = Value::serialized(&N(7)).unwrap();
    assert_eq!(value, Value::Tag(72, Box::new(Value::from(7))));
    assert_eq!(value.deserialized::<N>().unwrap(), N(7));

    let value = Value::serialized(&T(1, 2)).unwrap();
    assert_eq!(
        value,
        Value::Tag(73, Box::new(Value::Array(vec![1.into(), 2.into()])))
    );
    assert_eq!(value.deserialized::<T>().unwrap(), T(1, 2));

    // The untagged Value paths agree too.
    assert_eq!(Value::Null.deserialized::<U>().unwrap(), U);
    assert_eq!(Value::from(7).deserialized::<N>().unwrap(), N(7));
}

#[test]
fn unmarked_tags_stay_transparent_around_marked_structs() {
    // Marker without a tag: foreign container tags are skipped on both
    // paths, and a non-map payload is rejected.
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename = "@@CBOR@@@@a=1@@K")]
    struct K {
        a: u8,
    }

    let bytes = hex::decode("c9a10107").unwrap(); // 9({1: 7})
    assert_eq!(cbor2::from_slice::<K>(&bytes).unwrap(), K { a: 7 });

    let value = Value::Tag(9, Box::new(cbor2::cbor!({ 1 => 7 }).unwrap()));
    assert_eq!(value.deserialized::<K>().unwrap(), K { a: 7 });

    let msg = cbor2::from_slice::<K>(&hex::decode("07").unwrap())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("map"), "{msg}");
    let msg = Value::from(7).deserialized::<K>().unwrap_err().to_string();
    assert!(msg.contains("map"), "{msg}");
}

#[test]
fn marked_structs_decode_from_indefinite_maps() {
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename = "@@CBOR@@@@a=1@@K")]
    struct K {
        a: u8,
    }

    let bytes = hex::decode("bf0107ff").unwrap(); // {_ 1: 7}
    assert_eq!(cbor2::from_slice::<K>(&bytes).unwrap(), K { a: 7 });
}

#[test]
fn unknown_keys_on_plain_structs_are_ignored() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct F {
        a: u8,
    }

    // {-1: 2, "a": 7}: without a key table, a negative integer key takes
    // the placeholder identifier form and is simply an unknown field.
    let bytes = hex::decode("a22002616107").unwrap();
    assert_eq!(cbor2::from_slice::<F>(&bytes).unwrap(), F { a: 7 });

    // A tag around a text key is transparent on the Value path as well.
    let value = Value::Map(vec![(
        Value::Tag(9, Box::new(Value::from("a"))),
        Value::from(7),
    )]);
    assert_eq!(value.deserialized::<F>().unwrap(), F { a: 7 });
}

// A hand-rolled visitor is the only way to observe `MapAccess::size_hint`
// through the marker protocol: derived struct visitors never ask for it.
#[test]
fn marked_struct_access_reports_size_hints() {
    #[derive(Debug, PartialEq)]
    struct Hint(Option<usize>);

    impl<'de> serde::Deserialize<'de> for Hint {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct V;

            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Hint;

                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "a map")
                }

                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut acc: A,
                ) -> Result<Self::Value, A::Error> {
                    let hint = acc.size_hint();
                    while acc
                        .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                        .is_some()
                    {}
                    Ok(Hint(hint))
                }
            }

            deserializer.deserialize_struct("@@CBOR@@@@a=1@@H", &["a"], V)
        }
    }

    // Definite-length input knows its size; the Value path always does.
    let bytes = hex::decode("a10107").unwrap(); // {1: 7}
    assert_eq!(cbor2::from_slice::<Hint>(&bytes).unwrap(), Hint(Some(1)));

    let value = cbor2::cbor!({ 1 => 7 }).unwrap();
    assert_eq!(value.deserialized::<Hint>().unwrap(), Hint(Some(1)));

    // Indefinite-length input does not.
    let bytes = hex::decode("bf0107ff").unwrap(); // {_ 1: 7}
    assert_eq!(cbor2::from_slice::<Hint>(&bytes).unwrap(), Hint(None));
}

#[test]
fn enum_variant_shapes_across_both_paths() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum E {
        A,
        B(u8),
        C(u8, u8),
        S { x: u8 },
    }

    // Map-form variants; a unit variant requires a unit payload.
    let value = cbor2::cbor!({ "A": null }).unwrap();
    assert_eq!(value.deserialized::<E>().unwrap(), E::A);
    assert!(cbor2::cbor!({ "A": 1 })
        .unwrap()
        .deserialized::<E>()
        .is_err());
    let value = cbor2::cbor!({ "B": 7 }).unwrap();
    assert_eq!(value.deserialized::<E>().unwrap(), E::B(7));
    let value = cbor2::cbor!({ "C": [1, 2] }).unwrap();
    assert_eq!(value.deserialized::<E>().unwrap(), E::C(1, 2));

    // The bare text form only carries unit variants.
    assert_eq!(Value::from("A").deserialized::<E>().unwrap(), E::A);
    assert!(Value::from("B").deserialized::<E>().is_err());
    assert!(Value::from("C").deserialized::<E>().is_err());
    assert!(Value::from("S").deserialized::<E>().is_err());

    // A tag around a struct variant's payload is transparent, and a
    // non-map payload is rejected — on both paths.
    let tagged = Value::Map(vec![(
        Value::from("S"),
        Value::Tag(9, Box::new(cbor2::cbor!({ "x": 7 }).unwrap())),
    )]);
    assert_eq!(tagged.deserialized::<E>().unwrap(), E::S { x: 7 });
    let bytes = cbor2::to_vec(&tagged).unwrap();
    assert_eq!(cbor2::from_slice::<E>(&bytes).unwrap(), E::S { x: 7 });

    let bad = Value::Map(vec![(Value::from("S"), Value::from(7))]);
    let bytes = cbor2::to_vec(&bad).unwrap();
    assert!(cbor2::from_slice::<E>(&bytes).is_err());
}
