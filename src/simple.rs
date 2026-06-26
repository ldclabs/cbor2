//! Helper type for preserving CBOR simple values (RFC 8949 §3.3).
//!
//! Serde has no native notion of CBOR simple values beyond the built-in
//! booleans and null-like values. [`Simple`] carries the simple value number
//! through this crate's CBOR serializers and deserializers using an internal
//! protocol, much like the [`tag`](crate::tag) module does for semantic tags.
//!
//! ```rust
//! let simple = cbor2::Simple::new(59).unwrap();
//! assert_eq!(hex::encode(cbor2::to_vec(&simple).unwrap()), "f83b");
//! assert_eq!(cbor2::from_slice::<cbor2::Simple>(&[0xf8, 0x3b]).unwrap(), simple);
//! ```

#[cfg(feature = "alloc")]
use core::marker::PhantomData;

#[cfg(feature = "alloc")]
use serde::forward_to_deserialize_any;
use serde::{de, ser, Deserialize, Serialize};

/// The internal simple-value protocol.
pub(crate) const NAME: &str = "@@SIMPLE@@";
pub(crate) const VALUE: &str = "@@VALUE@@";

/// A CBOR simple value number.
///
/// Values 24 through 31 are reserved by RFC 8949 §3.3 and cannot be encoded
/// in well-formed CBOR. Construct a `Simple` with [`Simple::new`] or
/// [`TryFrom<u8>`] so reserved values are rejected before serialization.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Simple(u8);

impl Simple {
    /// Simple value 20: `false`.
    pub const FALSE: Self = Self(crate::core::simple::FALSE);
    /// Simple value 21: `true`.
    pub const TRUE: Self = Self(crate::core::simple::TRUE);
    /// Simple value 22: `null`.
    pub const NULL: Self = Self(crate::core::simple::NULL);
    /// Simple value 23: `undefined`.
    pub const UNDEFINED: Self = Self(crate::core::simple::UNDEFINED);

    /// Creates a simple value, rejecting reserved values 24 through 31.
    #[inline]
    pub const fn new(value: u8) -> Option<Self> {
        match value {
            24..=31 => None,
            _ => Some(Self(value)),
        }
    }

    /// Returns the numeric simple value.
    #[inline]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl From<Simple> for u8 {
    #[inline]
    fn from(value: Simple) -> Self {
        value.0
    }
}

/// Error returned when a byte is reserved and cannot be a well-formed simple
/// value.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct InvalidSimple(u8);

impl InvalidSimple {
    /// Returns the invalid numeric value.
    #[inline]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl core::fmt::Display for InvalidSimple {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid CBOR simple value {}", self.0)
    }
}

impl ser::StdError for InvalidSimple {}

impl TryFrom<u8> for Simple {
    type Error = InvalidSimple;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(InvalidSimple(value))
    }
}

impl Serialize for Simple {
    #[inline]
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_variant(NAME, 0, VALUE, &self.0)
    }
}

impl<'de> Deserialize<'de> for Simple {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        enum Variant {
            Value,
        }

        struct VariantVisitor;

        impl de::Visitor<'_> for VariantVisitor {
            type Value = Variant;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "a CBOR simple value variant")
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Variant, E> {
                match value {
                    0 => Ok(Variant::Value),
                    x => Err(de::Error::invalid_value(
                        de::Unexpected::Unsigned(x),
                        &"variant index 0",
                    )),
                }
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Variant, E> {
                match value {
                    VALUE => Ok(Variant::Value),
                    x => Err(de::Error::unknown_variant(x, &[VALUE])),
                }
            }
        }

        impl<'de> Deserialize<'de> for Variant {
            fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_identifier(VariantVisitor)
            }
        }

        struct InternalVisitor;

        impl<'de> de::Visitor<'de> for InternalVisitor {
            type Value = Simple;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "a CBOR simple value")
            }

            fn visit_enum<A: de::EnumAccess<'de>>(self, acc: A) -> Result<Self::Value, A::Error> {
                use de::VariantAccess as _;

                let (Variant::Value, access) = acc.variant()?;
                let value = access.newtype_variant::<u8>()?;
                Simple::new(value).ok_or_else(|| {
                    de::Error::invalid_value(
                        de::Unexpected::Unsigned(u64::from(value)),
                        &"a non-reserved CBOR simple value",
                    )
                })
            }
        }

        deserializer.deserialize_enum(NAME, &[VALUE], InternalVisitor)
    }
}

// An `EnumAccess`/`Deserializer` that presents a simple value to a visitor
// using the internal simple-value protocol. Only the alloc-gated
// deserializers use it.
#[cfg(feature = "alloc")]
pub(crate) struct SimpleAccess<E> {
    value: Simple,
    _error: PhantomData<E>,
}

#[cfg(feature = "alloc")]
impl<E> SimpleAccess<E> {
    #[inline]
    pub(crate) fn new(value: Simple) -> Self {
        Self {
            value,
            _error: PhantomData,
        }
    }
}

#[cfg(feature = "alloc")]
struct SimpleVariantDeserializer<E>(PhantomData<E>);

#[cfg(feature = "alloc")]
impl<'de, E: de::Error> de::Deserializer<'de> for SimpleVariantDeserializer<E> {
    type Error = E;

    #[inline]
    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(VALUE)
    }

    forward_to_deserialize_any! {
        i8 i16 i32 i64 i128
        u8 u16 u32 u64 u128
        bool f32 f64
        char str string
        bytes byte_buf
        option unit unit_struct newtype_struct
        seq tuple tuple_struct map struct enum
        identifier ignored_any
    }
}

#[cfg(feature = "alloc")]
struct SimpleValueDeserializer<E>(Simple, PhantomData<E>);

#[cfg(feature = "alloc")]
impl<'de, E: de::Error> de::Deserializer<'de> for SimpleValueDeserializer<E> {
    type Error = E;

    #[inline]
    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u8(self.0.value())
    }

    #[inline]
    fn deserialize_u8<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u8(self.0.value())
    }

    #[inline]
    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name == NAME {
            visitor.visit_enum(SimpleAccess::new(self.0))
        } else {
            self.deserialize_any(visitor)
        }
    }

    forward_to_deserialize_any! {
        i8 i16 i32 i64 i128
        u16 u32 u64 u128
        bool f32 f64
        char str string
        bytes byte_buf
        option unit unit_struct newtype_struct
        seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

#[cfg(feature = "alloc")]
impl<'de, E: de::Error> de::EnumAccess<'de> for SimpleAccess<E> {
    type Error = E;
    type Variant = Self;

    #[inline]
    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let variant = seed.deserialize(SimpleVariantDeserializer(PhantomData))?;
        Ok((variant, self))
    }
}

#[cfg(feature = "alloc")]
impl<'de, E: de::Error> de::VariantAccess<'de> for SimpleAccess<E> {
    type Error = E;

    #[inline]
    fn unit_variant(self) -> Result<(), Self::Error> {
        Err(de::Error::custom("expected CBOR simple value"))
    }

    #[inline]
    fn newtype_variant_seed<U: de::DeserializeSeed<'de>>(
        self,
        seed: U,
    ) -> Result<U::Value, Self::Error> {
        seed.deserialize(SimpleValueDeserializer(self.value, PhantomData))
    }

    #[inline]
    fn tuple_variant<V: de::Visitor<'de>>(
        self,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("expected CBOR simple value"))
    }

    #[inline]
    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("expected CBOR simple value"))
    }
}

// The serializer used to extract a simple value number from the internal
// simple pseudo-variant. Every other input is an error.
#[derive(Debug)]
pub(crate) struct NotASimple;

impl core::fmt::Display for NotASimple {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "expected CBOR simple value")
    }
}

impl ser::StdError for NotASimple {}

impl ser::Error for NotASimple {
    fn custom<U: core::fmt::Display>(_msg: U) -> Self {
        NotASimple
    }
}

pub(crate) struct SimpleValueSerializer;

impl ser::Serializer for SimpleValueSerializer {
    type Ok = Simple;
    type Error = NotASimple;

    type SerializeSeq = ser::Impossible<Simple, NotASimple>;
    type SerializeTuple = ser::Impossible<Simple, NotASimple>;
    type SerializeTupleStruct = ser::Impossible<Simple, NotASimple>;
    type SerializeTupleVariant = ser::Impossible<Simple, NotASimple>;
    type SerializeMap = ser::Impossible<Simple, NotASimple>;
    type SerializeStruct = ser::Impossible<Simple, NotASimple>;
    type SerializeStructVariant = ser::Impossible<Simple, NotASimple>;

    #[inline]
    fn serialize_u8(self, value: u8) -> Result<Simple, NotASimple> {
        Simple::new(value).ok_or(NotASimple)
    }

    // Without alloc, serde provides no default for `collect_str`; a formatted
    // string is never a simple value either way.
    fn collect_str<T: ?Sized + core::fmt::Display>(self, _: &T) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_bool(self, _: bool) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_i8(self, _: i8) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_i16(self, _: i16) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_i32(self, _: i32) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_i64(self, _: i64) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_i128(self, _: i128) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_u16(self, _: u16) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_u32(self, _: u32) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_u64(self, _: u64) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_u128(self, _: u128) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_f32(self, _: f32) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_f64(self, _: f64) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_char(self, _: char) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_str(self, _: &str) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_none(self) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_some<T: ?Sized + ser::Serialize>(self, _: &T) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_unit(self) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_newtype_struct<T: ?Sized + ser::Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_newtype_variant<T: ?Sized + ser::Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Simple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, NotASimple> {
        Err(NotASimple)
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, NotASimple> {
        Err(NotASimple)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}
