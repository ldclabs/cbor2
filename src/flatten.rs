//! Internal adapters for derived flattened structs. Values remain on the
//! original serde path, and the serializer buffers wire bytes, not Value.

pub(crate) mod fields;

use crate::raw::{RawSlice, RawValue};
use alloc::{string::String, vec::Vec};
use serde::{de, ser, Deserialize, Serialize};

type Keys = &'static [(&'static str, i128)];

#[doc(hidden)]
pub fn serialize<T: ?Sized + Serialize, S: ser::Serializer>(
    value: &T,
    serializer: S,
    tag: Option<u64>,
    keys: Keys,
) -> Result<S::Ok, S::Error> {
    // Serde does not know a flattened map's length ahead of time. Buffer it
    // once, then borrow its entries so even RawValue field spellings survive.
    let bytes = crate::to_vec(value).map_err(ser::Error::custom)?;
    let entries: Entries<'_> = crate::from_slice(&bytes).map_err(ser::Error::custom)?;
    let map = EncodedMap { entries, keys };
    match tag {
        Some(tag) => crate::tag::RequireAny(tag, map).serialize(serializer),
        None => map.serialize(serializer),
    }
}

struct Entries<'a>(Vec<(RawSlice<'a>, RawSlice<'a>)>);

impl<'de> Deserialize<'de> for Entries<'de> {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Entries<'de>;
            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a flattened map")
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(pair) = access.next_entry()? {
                    entries.push(pair);
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

struct EncodedMap<'a> {
    entries: Entries<'a>,
    keys: Keys,
}

impl Serialize for EncodedMap<'_> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.entries.0.len()))?;
        for (key, value) in &self.entries.0 {
            match mapped_text_key(key.0, self.keys).map_err(ser::Error::custom)? {
                Some(integer) => map.serialize_entry(&integer, value)?,
                None => map.serialize_entry(key, value)?,
            }
        }
        map.end()
    }
}

#[inline]
fn mapped_text_key(bytes: &[u8], keys: Keys) -> Result<Option<i128>, crate::de::Error> {
    // Serde field names almost always use an immediate text header. Their
    // bytes have already been validated by RawSlice capture.
    if bytes
        .first()
        .is_some_and(|head| (0x60..=0x77).contains(head))
    {
        return Ok(keys
            .iter()
            .find_map(|&(name, integer)| (name.as_bytes() == &bytes[1..]).then_some(integer)));
    }
    use crate::core::{Decoder, Header};
    let mut decoder = Decoder::from(bytes);
    let mut owned = String::new();
    let text = loop {
        match decoder.pull_slice()? {
            Header::Tag(_) => continue,
            Header::Text(Some(len)) => {
                break core::str::from_utf8(decoder.borrow_body(len)?)
                    .map_err(|_| crate::de::Error::Syntax(0))?
            }
            Header::Text(None) => {
                decoder.text_body(None, &mut owned)?;
                break &owned;
            }
            _ => return Ok(None),
        }
    };
    Ok(keys
        .iter()
        .find_map(|&(name, integer)| (name == text).then_some(integer)))
}

#[doc(hidden)]
pub fn deserialize<'de, T: Deserialize<'de>, D: de::Deserializer<'de>>(
    deserializer: D,
    keys: Keys,
) -> Result<T, D::Error> {
    T::deserialize(MapDeserializer {
        parent: deserializer,
        keys,
    })
}

struct MapDeserializer<D> {
    parent: D,
    keys: Keys,
}
impl<'de, D: de::Deserializer<'de>> de::Deserializer<'de> for MapDeserializer<D> {
    type Error = D::Error;
    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.parent.deserialize_map(MapVisitor {
            visitor,
            keys: self.keys,
        })
    }
    fn is_human_readable(&self) -> bool {
        self.parent.is_human_readable()
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct
        map struct enum identifier ignored_any
    }
}

struct MapVisitor<V> {
    visitor: V,
    keys: Keys,
}
impl<'de, V: de::Visitor<'de>> de::Visitor<'de> for MapVisitor<V> {
    type Value = V::Value;
    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.visitor.expecting(f)
    }
    fn visit_map<A: de::MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_map(MapAccess {
            parent: access,
            keys: self.keys,
        })
    }
}
struct MapAccess<A> {
    parent: A,
    keys: Keys,
}
impl<'de, A: de::MapAccess<'de>> de::MapAccess<'de> for MapAccess<A> {
    type Error = A::Error;
    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.parent.next_key_seed(KeySeed {
            seed,
            keys: self.keys,
        })
    }
    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, A::Error> {
        self.parent.next_value_seed(seed)
    }
    fn size_hint(&self) -> Option<usize> {
        self.parent.size_hint()
    }
}
struct KeySeed<K> {
    seed: K,
    keys: Keys,
}
impl<'de, K: de::DeserializeSeed<'de>> de::DeserializeSeed<'de> for KeySeed<K> {
    type Value = K::Value;
    fn deserialize<D: de::Deserializer<'de>>(self, parent: D) -> Result<Self::Value, D::Error> {
        self.seed.deserialize(KeyDeserializer {
            parent,
            keys: self.keys,
        })
    }
}
struct KeyDeserializer<D> {
    parent: D,
    keys: Keys,
}
impl<'de, D: de::Deserializer<'de>> de::Deserializer<'de> for KeyDeserializer<D> {
    type Error = D::Error;
    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.parent.deserialize_any(KeyVisitor {
            visitor,
            keys: self.keys,
        })
    }
    fn is_human_readable(&self) -> bool {
        self.parent.is_human_readable()
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct
        map struct enum identifier ignored_any
    }
}
struct KeyVisitor<V> {
    visitor: V,
    keys: Keys,
}
macro_rules! forward_visit {
    ($($method:ident($ty:ty)),* $(,)?) => {$(
        fn $method<E: de::Error>(self, value: $ty) -> Result<Self::Value, E> { self.visitor.$method(value) }
    )*};
}
impl<'de, V: de::Visitor<'de>> de::Visitor<'de> for KeyVisitor<V> {
    type Value = V::Value;
    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.visitor.expecting(f)
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        match self.keys.iter().find(|(_, key)| *key == i128::from(value)) {
            Some((name, _)) => self.visitor.visit_str(name),
            None => self.visitor.visit_u64(value),
        }
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        self.visit_i128(i128::from(value))
    }
    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Self::Value, E> {
        match self.keys.iter().find(|(_, key)| *key == value) {
            Some((name, _)) => self.visitor.visit_str(name),
            None => match i64::try_from(value) {
                Ok(value) => self.visitor.visit_i64(value),
                Err(_) => self
                    .visitor
                    .visit_str(&alloc::format!("{}{value}", crate::de::INT_KEY_PLACEHOLDER)),
            },
        }
    }
    forward_visit! { visit_bool(bool), visit_f64(f64), visit_u128(u128), visit_str(&str), visit_string(String), visit_bytes(&[u8]), visit_byte_buf(Vec<u8>), visit_borrowed_str(&'de str), visit_borrowed_bytes(&'de [u8]) }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_unit()
    }
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_none()
    }
    fn visit_seq<A: de::SeqAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_seq(access)
    }
    fn visit_map<A: de::MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_map(access)
    }
    fn visit_enum<A: de::EnumAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        use de::VariantAccess;
        let (name, variant) = access.variant::<String>()?;
        if name == crate::tag::TAGGED {
            return variant.tuple_variant(2, TaggedKey(self));
        }
        Err(de::Error::custom("expected a field key"))
    }
}
impl<'de, V: de::Visitor<'de>> de::DeserializeSeed<'de> for KeyVisitor<V> {
    type Value = V::Value;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}
struct TaggedKey<V>(KeyVisitor<V>);
impl<'de, V: de::Visitor<'de>> de::Visitor<'de> for TaggedKey<V> {
    type Value = V::Value;
    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("a tagged field key")
    }
    fn visit_seq<A: de::SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let tag = access
            .next_element::<u64>()?
            .ok_or_else(|| de::Error::custom("missing tag"))?;
        let key = access
            .next_element::<RawValue>()?
            .ok_or_else(|| de::Error::custom("missing key"))?;

        // Tags around registered integer keys remain transparent so serde
        // can recognize the declared field. Serde's private flatten buffer
        // cannot represent enum-shaped keys, so reject unknown tagged keys
        // instead of silently turning them into their untagged payload.
        let value: crate::Value = crate::from_slice(key.as_bytes()).map_err(de::Error::custom)?;
        if let Some(name) = tagged_field(&value, self.0.keys) {
            return self.0.visitor.visit_str(name);
        }
        Err(de::Error::custom(alloc::format!(
            "tagged extension key {tag} cannot be represented by #[serde(flatten)]"
        )))
    }
}

fn tagged_field(mut key: &crate::Value, keys: Keys) -> Option<&'static str> {
    while let crate::Value::Tag(_, inner) = key {
        key = inner;
    }
    let crate::Value::Integer(integer) = key else {
        return None;
    };
    let integer = i128::from(*integer);
    keys.iter()
        .find_map(|&(name, key)| (key == integer).then_some(name))
}
