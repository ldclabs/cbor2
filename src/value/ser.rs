use alloc::{vec, vec::Vec};

use serde::ser::{self, SerializeMap as _, SerializeSeq as _, SerializeTupleVariant as _};

use super::{Error, Integer, Value};
use crate::ser::StructShape;

// Struct field keys follow the same COSE-style rule as the streaming
// serializer: fields named in the container's key table become integer
// keys.
fn field_key(keys: &str, key: &'static str) -> Value {
    match crate::ser::key_for_field(keys, key) {
        Some(n) => Value::Integer(Integer::try_from(n).expect("field keys are in range")),
        None => key.into(),
    }
}

// Wraps a finished value in the container marker's tag, if any.
fn apply_tag(tag: Option<u64>, value: Value) -> Value {
    match tag {
        Some(tag) => Value::Tag(tag, value.into()),
        None => value,
    }
}

impl ser::Serialize for Value {
    #[inline]
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Bytes(x) => serializer.serialize_bytes(x),
            Value::Bool(x) => serializer.serialize_bool(*x),
            Value::Text(x) => serializer.serialize_str(x),
            Value::Null => serializer.serialize_unit(),
            Value::Simple(x) => serde::Serialize::serialize(x, serializer),

            Value::Tag(t, v) => {
                let mut acc = serializer.serialize_tuple_variant(
                    crate::tag::NAME,
                    1,
                    crate::tag::TAGGED,
                    2,
                )?;
                acc.serialize_field(t)?;
                acc.serialize_field(v)?;
                acc.end()
            }

            Value::Float(x) => {
                let y = *x as f32;
                if (y as f64).to_bits() == x.to_bits() {
                    serializer.serialize_f32(y)
                } else {
                    serializer.serialize_f64(*x)
                }
            }

            Value::Integer(x) => {
                if let Ok(x) = u64::try_from(*x) {
                    serializer.serialize_u64(x)
                } else if let Ok(x) = i64::try_from(*x) {
                    serializer.serialize_i64(x)
                } else {
                    serializer.serialize_i128(i128::from(*x))
                }
            }

            Value::Array(x) => {
                let mut acc = serializer.serialize_seq(Some(x.len()))?;

                for item in x {
                    acc.serialize_element(item)?;
                }

                acc.end()
            }

            Value::Map(x) => {
                let mut acc = serializer.serialize_map(Some(x.len()))?;

                for (key, val) in x {
                    acc.serialize_entry(key, val)?;
                }

                acc.end()
            }
        }
    }
}

macro_rules! mkserialize {
    ($($f:ident($v:ty)),+ $(,)?) => {
        $(
            #[inline]
            fn $f(self, v: $v) -> Result<Self::Ok, Self::Error> {
                Ok(v.into())
            }
        )+
    };
}

struct Named<T> {
    name: &'static str,
    data: T,
    tag: Option<Tagged>,
    keys: &'static str,
}

struct Tagged {
    tag: Option<u64>,
    val: Option<Value>,
}

struct Map {
    data: Vec<(Value, Value)>,
    temp: Option<Value>,
}

struct StructFields {
    map: Vec<(Value, Value)>,
    array: Vec<Value>,
    shape: StructShape,
}

// The collector for a struct: a map plus the key table and tag of its
// container marker, if any.
struct StructMap {
    data: Vec<(Value, Value)>,
    array: Vec<Value>,
    keys: &'static str,
    tag: Option<u64>,
    shape: StructShape,
}

// The collector for a tuple struct, which may carry a marker tag.
struct TaggedArray {
    data: Vec<Value>,
    tag: Option<u64>,
}

struct Serializer<T>(T);

impl ser::Serializer for Serializer<()> {
    type Ok = Value;
    type Error = Error;

    type SerializeSeq = Serializer<Vec<Value>>;
    type SerializeTuple = Serializer<Vec<Value>>;
    type SerializeTupleStruct = Serializer<TaggedArray>;
    type SerializeTupleVariant = Serializer<Named<Vec<Value>>>;
    type SerializeMap = Serializer<Map>;
    type SerializeStruct = Serializer<StructMap>;
    type SerializeStructVariant = Serializer<Named<StructFields>>;

    mkserialize! {
        serialize_bool(bool),

        serialize_f32(f32),
        serialize_f64(f64),

        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),

        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
    }

    #[inline]
    fn serialize_none(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    #[inline]
    fn serialize_some<U: ?Sized + ser::Serialize>(self, value: &U) -> Result<Value, Error> {
        value.serialize(self)
    }

    #[inline]
    fn serialize_unit(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    #[inline]
    fn serialize_unit_struct(self, name: &'static str) -> Result<Value, Error> {
        let tag = crate::ser::parse_struct_marker(name).and_then(|marker| marker.tag);
        Ok(apply_tag(tag, Value::Null))
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Value, Error> {
        Ok(variant.into())
    }

    #[inline]
    fn serialize_newtype_struct<U: ?Sized + ser::Serialize>(
        self,
        name: &'static str,
        value: &U,
    ) -> Result<Value, Error> {
        // A `Value` has no raw form: decode the `RawValue` instead.
        if name == crate::raw::NAME {
            return match value.serialize(crate::raw::RawBytesSerializer) {
                Ok(bytes) => crate::from_slice(&bytes).map_err(ser::Error::custom),
                Err(err) => Err(ser::Error::custom(err)),
            };
        }

        let tag = crate::ser::parse_struct_marker(name).and_then(|marker| marker.tag);
        Ok(apply_tag(tag, value.serialize(self)?))
    }

    #[inline]
    fn serialize_newtype_variant<U: ?Sized + ser::Serialize>(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &U,
    ) -> Result<Value, Error> {
        Ok(match (name, variant) {
            (crate::simple::NAME, crate::simple::VALUE) => Value::Simple(
                value
                    .serialize(crate::simple::SimpleValueSerializer)
                    .map_err(|_| ser::Error::custom("expected CBOR simple value"))?,
            ),
            (crate::tag::NAME, crate::tag::UNTAGGED) => Value::serialized(value)?,
            _ => vec![(variant.into(), Value::serialized(value)?)].into(),
        })
    }

    #[inline]
    fn serialize_seq(self, length: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Ok(Serializer(Vec::with_capacity(length.unwrap_or(0))))
    }

    #[inline]
    fn serialize_tuple(self, length: usize) -> Result<Self::SerializeTuple, Error> {
        self.serialize_seq(Some(length))
    }

    #[inline]
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Ok(Serializer(TaggedArray {
            data: Vec::with_capacity(length),
            tag: crate::ser::parse_struct_marker(name).and_then(|marker| marker.tag),
        }))
    }

    #[inline]
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Ok(Serializer(Named {
            name: variant,
            data: Vec::with_capacity(length),
            tag: match (name, variant) {
                (crate::tag::NAME, crate::tag::TAGGED) => Some(Tagged {
                    tag: None,
                    val: None,
                }),

                _ => None,
            },
            keys: "",
        }))
    }

    #[inline]
    fn serialize_map(self, length: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Ok(Serializer(Map {
            data: Vec::with_capacity(length.unwrap_or(0)),
            temp: None,
        }))
    }

    #[inline]
    fn serialize_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        let marker = crate::ser::parse_struct_marker(name);
        Ok(Serializer(StructMap {
            data: Vec::with_capacity(length),
            array: Vec::with_capacity(length),
            keys: marker.as_ref().map_or("", |marker| marker.keys),
            tag: marker.as_ref().and_then(|marker| marker.tag),
            shape: marker.map_or(StructShape::Map, |marker| marker.shape),
        }))
    }

    #[inline]
    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        let marker = crate::ser::parse_struct_marker(name);
        Ok(Serializer(Named {
            name: variant,
            data: StructFields {
                map: Vec::with_capacity(length),
                array: Vec::with_capacity(length),
                shape: marker
                    .as_ref()
                    .map_or(StructShape::Map, |marker| marker.shape),
            },
            tag: None,
            keys: marker.map_or("", |marker| marker.keys),
        }))
    }

    #[inline]
    fn is_human_readable(&self) -> bool {
        false
    }
}

impl ser::SerializeSeq for Serializer<Vec<Value>> {
    type Ok = Value;
    type Error = Error;

    #[inline]
    fn serialize_element<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        self.0.push(Value::serialized(value)?);
        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Value, Error> {
        Ok(self.0.into())
    }
}

impl ser::SerializeTuple for Serializer<Vec<Value>> {
    type Ok = Value;
    type Error = Error;

    #[inline]
    fn serialize_element<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        self.0.push(Value::serialized(value)?);
        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Value, Error> {
        Ok(self.0.into())
    }
}

impl ser::SerializeTupleStruct for Serializer<TaggedArray> {
    type Ok = Value;
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        self.0.data.push(Value::serialized(value)?);
        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Value, Error> {
        Ok(apply_tag(self.0.tag, self.0.data.into()))
    }
}

impl ser::SerializeTupleVariant for Serializer<Named<Vec<Value>>> {
    type Ok = Value;
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        match self.0.tag.as_mut() {
            Some(tag) => match tag.tag {
                None => match value.serialize(crate::tag::TagNumberSerializer) {
                    Ok(t) => tag.tag = Some(t),
                    Err(..) => return Err(ser::Error::custom("expected tag")),
                },

                Some(..) => tag.val = Some(Value::serialized(value)?),
            },

            None => self.0.data.push(Value::serialized(value)?),
        }

        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Value, Error> {
        Ok(match self.0.tag {
            Some(Tagged {
                tag: Some(t),
                val: Some(v),
            }) => Value::Tag(t, v.into()),
            Some(..) => return Err(ser::Error::custom("invalid tag input")),
            None => vec![(self.0.name.into(), self.0.data.into())].into(),
        })
    }
}

impl ser::SerializeMap for Serializer<Map> {
    type Ok = Value;
    type Error = Error;

    #[inline]
    fn serialize_key<U: ?Sized + ser::Serialize>(&mut self, key: &U) -> Result<(), Error> {
        self.0.temp = Some(Value::serialized(key)?);
        Ok(())
    }

    #[inline]
    fn serialize_value<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        // Tolerate misbehaving Serialize implementations instead of
        // panicking; serde requires a key before every value.
        let key = match self.0.temp.take() {
            Some(key) => key,
            None => {
                return Err(ser::Error::custom(
                    "serialize_value called before serialize_key",
                ))
            }
        };
        let val = Value::serialized(value)?;

        self.0.data.push((key, val));
        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Value, Error> {
        Ok(self.0.data.into())
    }
}

impl ser::SerializeStruct for Serializer<StructMap> {
    type Ok = Value;
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(
        &mut self,
        key: &'static str,
        value: &U,
    ) -> Result<(), Error> {
        let value = Value::serialized(value)?;
        match self.0.shape {
            StructShape::Map => self.0.data.push((field_key(self.0.keys, key), value)),
            StructShape::Array => self.0.array.push(value),
        }
        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Value, Error> {
        Ok(match self.0.shape {
            StructShape::Map => apply_tag(self.0.tag, self.0.data.into()),
            StructShape::Array => apply_tag(self.0.tag, self.0.array.into()),
        })
    }
}

impl ser::SerializeStructVariant for Serializer<Named<StructFields>> {
    type Ok = Value;
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(
        &mut self,
        key: &'static str,
        value: &U,
    ) -> Result<(), Error> {
        let value = Value::serialized(value)?;
        match self.0.data.shape {
            StructShape::Map => self.0.data.map.push((field_key(self.0.keys, key), value)),
            StructShape::Array => self.0.data.array.push(value),
        }
        Ok(())
    }

    #[inline]
    fn end(self) -> Result<Value, Error> {
        let value = match self.0.data.shape {
            StructShape::Map => self.0.data.map.into(),
            StructShape::Array => self.0.data.array.into(),
        };
        Ok(vec![(self.0.name.into(), value)].into())
    }
}

impl Value {
    /// Serializes any `T: Serialize` into a `Value`.
    ///
    /// This uses the same CBOR-oriented serde data model as [`to_vec`]:
    /// structs become maps, enum unit variants become text strings, other
    /// enum variants become single-entry maps, `u128`/`i128` may become
    /// bignum tags, and `cbor2::tag` wrappers become [`Value::Tag`].
    ///
    /// [`to_vec`]: crate::to_vec
    ///
    /// ```rust
    /// use serde::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct Event<'a> {
    ///     level: &'a str,
    ///     count: u64,
    /// }
    ///
    /// let value = cbor2::Value::serialized(&Event { level: "info", count: 2 }).unwrap();
    /// assert_eq!(value.to_string(), r#"{"level": "info", "count": 2}"#);
    /// ```
    #[inline]
    pub fn serialized<T: ?Sized + ser::Serialize>(value: &T) -> Result<Self, Error> {
        value.serialize(Serializer(()))
    }
}
