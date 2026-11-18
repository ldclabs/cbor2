//! Binary view of a flattened struct. Declared fields use their original
//! deserializer. Buffer only the extension entries so each flattened field can
//! read them with its own type, including CBOR tags, simple values and raw items.

use alloc::{borrow::Cow, vec::Vec};
use serde::{
    de::{self, DeserializeSeed, IntoDeserializer, MapAccess, Visitor},
    ser, Deserialize, Serialize,
};

use super::{Entries, Keys};
use crate::core::{Decoder, Header};

type Names = &'static [&'static str];
pub(crate) const RAW_ITEM: &str = "@@CBOR_FLATTEN_ITEM@@";

#[doc(hidden)]
pub fn serialize<S: ser::Serializer, const N: usize>(
    serializer: S,
    tag: Option<u64>,
    keys: Keys,
    names: Names,
    encode: impl FnOnce(&mut Vec<u8>) -> Result<[(bool, usize); N], crate::ser::Error>,
) -> Result<S::Ok, S::Error> {
    // Leave room for the definite map header. Coalesce the already-encoded
    // map bodies in declaration order without decoding their values.
    const MAX_HEADER: usize = 9;
    let mut bytes = alloc::vec![0; MAX_HEADER];
    let groups = encode(&mut bytes).map_err(ser::Error::custom)?;
    let mut start = MAX_HEADER;
    let mut write = MAX_HEADER;
    let mut count = 0;
    for (is_extension, end) in groups {
        let group = bytes
            .get(start..end)
            .ok_or_else(|| ser::Error::custom("invalid flattened map boundary"))?;
        let mut decoder = Decoder::from(group);
        let Header::Map(length) = decoder.pull_slice().map_err(ser::Error::custom)? else {
            return Err(ser::Error::custom(
                "flattened fields must serialize as a map",
            ));
        };
        if is_extension || length.is_none() {
            let entries: Entries<'_> = crate::from_slice(group).map_err(ser::Error::custom)?;
            count += entries.0.len();
            if is_extension {
                for (key, _) in entries.0 {
                    if let Some(name) =
                        field_name(key.0, keys, names, false).map_err(ser::Error::custom)?
                    {
                        return Err(ser::Error::custom(format_args!(
                            "flattened key conflicts with declared field `{name}`"
                        )));
                    }
                }
            }
        } else if let Some(n) = length {
            count += n;
        }
        let body = start + decoder.offset()..end - usize::from(length.is_none());
        let len = body.len();
        bytes.copy_within(body, write);
        write += len;
        start = end;
    }
    let mut header = [0; MAX_HEADER];
    let mut slot = header.as_mut_slice();
    crate::core::Encoder::from(&mut slot)
        .map(Some(count))
        .map_err(ser::Error::custom)?;
    let start = slot.len();
    bytes[start..MAX_HEADER].copy_from_slice(&header[..MAX_HEADER - start]);
    let map = super::RawSlice(&bytes[start..write]);
    match tag {
        Some(tag) => crate::tag::RequireAny(tag, map).serialize(serializer),
        None => map.serialize(serializer),
    }
}

// Return an existing static serde field name or alias, without allocating
// for definite text and integer labels.
fn field_name(
    bytes: &[u8],
    keys: Keys,
    names: Names,
    reject_tagged: bool,
) -> Result<Option<&'static str>, crate::de::Error> {
    let registered = |n| {
        keys.iter()
            .find_map(|&(name, key)| (n == key).then_some(name))
    };
    let named = |bytes: &[u8]| names.iter().copied().find(|name| name.as_bytes() == bytes);
    // These are complete, validated items. Short labels need no second decoder.
    match bytes[0] {
        n @ 0..=23 => return Ok(registered(i128::from(n))),
        n @ 0x20..=0x37 => return Ok(registered(-1 - i128::from(n - 0x20))),
        0x40..=0x57 | 0x60..=0x77 => return Ok(named(&bytes[1..])),
        _ => {}
    }
    let mut decoder = Decoder::from(bytes);
    let mut tag = None;
    let header = loop {
        match decoder.pull_slice()? {
            Header::Tag(n) => {
                tag.get_or_insert(n);
            }
            header => break header,
        }
    };
    let int = match header {
        Header::Positive(n) => Some(i128::from(n)),
        Header::Negative(n) => Some(-1 - i128::from(n)),
        _ => None,
    };
    if let Some(name) = int.and_then(registered) {
        return Ok(Some(name));
    }
    if let Some(tag) = tag {
        if reject_tagged {
            return Err(crate::de::Error::semantic(
                None,
                alloc::format!(
                    "tagged extension key {tag} cannot be represented by #[serde(flatten)]"
                ),
            ));
        }
        return Ok(None);
    }
    let mut chunks = Vec::new();
    let text = match header {
        Header::Text(Some(n)) | Header::Bytes(Some(n)) => decoder.borrow_body(n)?,
        Header::Text(None) => {
            let text = crate::from_slice::<alloc::string::String>(bytes)?;
            return Ok(names.iter().copied().find(|name| *name == text));
        }
        Header::Bytes(None) => {
            decoder.bytes_body(None, &mut chunks)?;
            &chunks
        }
        _ => return Ok(None),
    };
    Ok(named(text))
}

struct Item<'de>(Cow<'de, [u8]>);
impl<'de> Deserialize<'de> for Item<'de> {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Capture;
        impl<'de> Visitor<'de> for Capture {
            type Value = Item<'de>;
            fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                f.write_str("a raw CBOR item")
            }
            fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
                Ok(Item(Cow::Borrowed(v)))
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                Ok(Item(Cow::Owned(v)))
            }
        }
        d.deserialize_newtype_struct(RAW_ITEM, Capture)
    }
}
impl<'de> Item<'de> {
    fn read<T: DeserializeSeed<'de>>(&self, seed: T) -> Result<T::Value, crate::de::Error> {
        match &self.0 {
            Cow::Borrowed(bytes) => {
                seed.deserialize(&mut crate::de::Deserializer::from_slice(bytes))
            }
            Cow::Owned(bytes) => {
                seed.deserialize(&mut crate::de::Deserializer::from_reader(bytes.as_slice()))
            }
        }
    }
}

#[doc(hidden)]
pub fn deserializer<'de, D: de::Deserializer<'de>>(
    parent: D,
    keys: Keys,
    extensions: Names,
    deny_unknown: bool,
) -> impl de::Deserializer<'de, Error = D::Error> {
    Fields {
        inner: parent,
        schema: Schema {
            keys,
            extensions,
            declared: &[],
            deny_unknown,
        },
    }
}

struct Schema {
    keys: Keys,
    extensions: Names,
    declared: Names,
    deny_unknown: bool,
}
struct Fields<T> {
    inner: T,
    schema: Schema,
}
impl<'de, D: de::Deserializer<'de>> de::Deserializer<'de> for Fields<D> {
    type Error = D::Error;
    fn deserialize_struct<V: Visitor<'de>>(
        mut self,
        _: &'static str,
        fields: Names,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.schema.declared = fields;
        self.inner.deserialize_map(Fields {
            inner: visitor,
            schema: self.schema,
        })
    }
    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, D::Error> {
        Err(de::Error::custom("expected a flattened struct"))
    }
    fn is_human_readable(&self) -> bool {
        false
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct
        map enum identifier ignored_any
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Fields<V> {
    type Value = V::Value;
    fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.inner.expecting(f)
    }
    fn visit_map<A: MapAccess<'de>>(self, parent: A) -> Result<V::Value, A::Error> {
        let mut access = FieldsAccess {
            parent: Some(parent),
            groups: self.schema.extensions.iter(),
            schema: &self.schema,
            entries: Vec::new(),
        };
        let value = self.inner.visit_map(&mut access)?;
        if self.schema.deny_unknown && access.entries.iter().any(|entry| !entry.claimed) {
            return Err(de::Error::custom("unknown field in flattened map"));
        }
        Ok(value)
    }
}

struct Pair<'de> {
    key: Item<'de>,
    value: Item<'de>,
    claimed: bool,
}
struct FieldsAccess<'a, 'de, A> {
    parent: Option<A>,
    schema: &'a Schema,
    groups: core::slice::Iter<'static, &'static str>,
    entries: Vec<Pair<'de>>,
}
impl<'de, A: MapAccess<'de>> MapAccess<'de> for FieldsAccess<'_, 'de, A> {
    type Error = A::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        if let Some(parent) = &mut self.parent {
            while let Some(key) = parent.next_key::<Item<'de>>()? {
                let name = field_name(&key.0, self.schema.keys, self.schema.declared, true)
                    .map_err(de::Error::custom)?;
                if let Some(name) = name.filter(|name| !self.schema.extensions.contains(name)) {
                    return seed.deserialize(name.into_deserializer()).map(Some);
                }
                self.entries.push(Pair {
                    key,
                    value: parent.next_value()?,
                    claimed: false,
                });
            }
            self.parent = None;
        }
        match self.groups.next() {
            Some(&name) => seed.deserialize(name.into_deserializer()).map(Some),
            None => Ok(None),
        }
    }
    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, A::Error> {
        match &mut self.parent {
            Some(parent) => parent.next_value_seed(seed),
            None => seed
                .deserialize(Extensions(&mut self.entries))
                .map_err(de::Error::custom),
        }
    }
}

// Flattened structs claim their recognized keys. Maps see all entries still
// available at that point, just as they do in serde's flatten deserializer.
struct Extensions<'a, 'de>(&'a mut [Pair<'de>]);
impl<'de> de::Deserializer<'de> for Extensions<'_, 'de> {
    type Error = crate::de::Error;
    fn is_human_readable(&self) -> bool {
        false
    }
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(ExtensionMap {
            iter: self.0.iter_mut(),
            fields: None,
            value: None,
        })
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: Names,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_map(ExtensionMap {
            iter: self.0.iter_mut(),
            fields: Some(fields),
            value: None,
        })
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        names: Names,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        for entry in self.0 {
            if !entry.claimed && field_name(&entry.key.0, &[], names, false)?.is_some() {
                let map = ExtensionMap {
                    iter: core::slice::from_mut(entry).iter_mut(),
                    fields: Some(names),
                    value: None,
                };
                return visitor.visit_enum(de::value::MapAccessDeserializer::new(map));
            }
        }
        Err(de::Error::custom("missing flattened enum variant"))
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }
    // Serde’s flatten-specific option visitor can return None if the inner
    // struct is missing required fields. It works with our raw entries too.
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor
            .__private_visit_untagged_option(self)
            .map_err(|()| de::Error::custom("expected a flattened option"))
    }
    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf seq tuple tuple_struct map identifier ignored_any
    }
}
struct ExtensionMap<'a, 'de> {
    iter: core::slice::IterMut<'a, Pair<'de>>,
    fields: Option<Names>,
    value: Option<&'a Item<'de>>,
}
impl<'de> MapAccess<'de> for ExtensionMap<'_, 'de> {
    type Error = crate::de::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        for entry in self.iter.by_ref() {
            if entry.claimed {
                continue;
            }
            if let Some(names) = self.fields {
                if field_name(&entry.key.0, &[], names, false)?.is_none() {
                    continue;
                }
                entry.claimed = true;
            }
            self.value = Some(&entry.value);
            return entry.key.read(seed).map(Some);
        }
        Ok(None)
    }
    fn next_value_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<T::Value, Self::Error> {
        self.value
            .take()
            .ok_or_else(|| Self::Error::semantic(None, "missing flattened value"))?
            .read(seed)
    }
}
