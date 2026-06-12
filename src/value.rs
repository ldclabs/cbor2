//! A dynamic CBOR value.

use std::collections::{BTreeMap, HashMap};

mod canonical;
mod de;
mod integer;
mod ser;

pub use canonical::KeyOrder;
pub use integer::Integer;

/// An error when serializing to or deserializing from a [`Value`].
#[derive(Clone, Debug)]
pub enum Error {
    /// A custom error message produced by serde.
    Custom(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Custom(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl serde::de::Error for Error {
    #[inline]
    fn custom<T: core::fmt::Display>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }
}

impl serde::ser::Error for Error {
    #[inline]
    fn custom<T: core::fmt::Display>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }
}

/// A representation of any CBOR item that can be inspected and manipulated
/// dynamically.
///
/// Maps are represented as `Vec<(Value, Value)>` rather than as an ordered
/// or hashed map type. This preserves the order of the pairs on the wire and
/// makes no assumptions about key uniqueness; convert with `TryFrom` —
/// `HashMap` and `BTreeMap` are supported directly — if you need a map type.
///
/// `Value` intentionally models the serde-visible CBOR data model, not every
/// byte-level spelling. For example, indefinite-length strings are decoded
/// into the same variants as definite-length strings, while unknown tags
/// remain as [`Value::Tag`] and oversized bignums stay as tagged byte
/// strings.
#[non_exhaustive]
#[derive(Clone, PartialEq, PartialOrd)]
pub enum Value {
    /// An integer (major type 0 or 1).
    Integer(Integer),

    /// A byte string (major type 2).
    Bytes(Vec<u8>),

    /// A floating-point value (major type 7).
    Float(f64),

    /// A text string (major type 3).
    Text(String),

    /// A boolean (major type 7).
    Bool(bool),

    /// Null (major type 7).
    Null,

    /// A tagged value (major type 6).
    Tag(u64, Box<Value>),

    /// An array (major type 4).
    Array(Vec<Value>),

    /// A map (major type 5).
    Map(Vec<(Value, Value)>),
}

/// Formats the value as indented CBOR diagnostic notation (RFC 8949 §8).
///
/// This is the multi-line counterpart of the [`Display`](Self#impl-Display-for-Value)
/// implementation: arrays and maps spread one element per line, nested
/// levels are indented by two spaces, and scalars render exactly as in
/// the compact form.
///
/// ```
/// use cbor2::cbor;
///
/// let value = cbor!({ "a": [1, 2] }).unwrap();
/// assert_eq!(
///     format!("{value:?}"),
///     "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
/// );
/// ```
impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut out = String::new();
        crate::diag::write_value_pretty(&mut out, self, 0);
        f.write_str(&out)
    }
}

macro_rules! accessors {
    ($(#[doc = $doc:literal] $is:ident $as:ident $into:ident($variant:ident) -> $t:ty;)+) => {
        $(
            #[doc = concat!("Returns true if the value is ", $doc, ".")]
            #[inline]
            pub fn $is(&self) -> bool {
                matches!(self, Value::$variant(..))
            }

            #[doc = concat!("If the value is ", $doc, ", returns a reference to it. Returns `None` otherwise.")]
            #[inline]
            pub fn $as(&self) -> Option<&$t> {
                match self {
                    Value::$variant(x) => Some(x),
                    _ => None,
                }
            }

            #[doc = concat!("If the value is ", $doc, ", returns it as `Ok`. Returns `Err(self)` otherwise.")]
            #[inline]
            pub fn $into(self) -> Result<$t, Self> {
                match self {
                    Value::$variant(x) => Ok(x),
                    other => Err(other),
                }
            }
        )+
    };
}

impl Value {
    accessors! {
        #[doc = "a byte string"]
        is_bytes as_bytes into_bytes(Bytes) -> Vec<u8>;

        #[doc = "an array"]
        is_array as_array into_array(Array) -> Vec<Value>;

        #[doc = "a map"]
        is_map as_map into_map(Map) -> Vec<(Value, Value)>;
    }

    /// Returns true if the value is an integer.
    #[inline]
    pub fn is_integer(&self) -> bool {
        matches!(self, Value::Integer(..))
    }

    /// If the value is an integer, returns it. Returns `None` otherwise.
    #[inline]
    pub fn as_integer(&self) -> Option<Integer> {
        match self {
            Value::Integer(x) => Some(*x),
            _ => None,
        }
    }

    /// If the value is an integer, returns it as `Ok`. Returns `Err(self)`
    /// otherwise.
    #[inline]
    pub fn into_integer(self) -> Result<Integer, Self> {
        match self {
            Value::Integer(x) => Ok(x),
            other => Err(other),
        }
    }

    /// Returns true if the value is a float.
    #[inline]
    pub fn is_float(&self) -> bool {
        matches!(self, Value::Float(..))
    }

    /// If the value is a float, returns it. Returns `None` otherwise.
    #[inline]
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(x) => Some(*x),
            _ => None,
        }
    }

    /// If the value is a float, returns it as `Ok`. Returns `Err(self)`
    /// otherwise.
    #[inline]
    pub fn into_float(self) -> Result<f64, Self> {
        match self {
            Value::Float(x) => Ok(x),
            other => Err(other),
        }
    }

    /// Returns true if the value is a text string.
    #[inline]
    pub fn is_text(&self) -> bool {
        matches!(self, Value::Text(..))
    }

    /// If the value is a text string, returns a reference to it. Returns
    /// `None` otherwise.
    #[inline]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(x) => Some(x),
            _ => None,
        }
    }

    /// If the value is a text string, returns it as `Ok`. Returns
    /// `Err(self)` otherwise.
    #[inline]
    pub fn into_text(self) -> Result<String, Self> {
        match self {
            Value::Text(x) => Ok(x),
            other => Err(other),
        }
    }

    /// Returns true if the value is a boolean.
    #[inline]
    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(..))
    }

    /// If the value is a boolean, returns it. Returns `None` otherwise.
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(x) => Some(*x),
            _ => None,
        }
    }

    /// If the value is a boolean, returns it as `Ok`. Returns `Err(self)`
    /// otherwise.
    #[inline]
    pub fn into_bool(self) -> Result<bool, Self> {
        match self {
            Value::Bool(x) => Ok(x),
            other => Err(other),
        }
    }

    /// Returns true if the value is null.
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Returns true if the value is a tag.
    #[inline]
    pub fn is_tag(&self) -> bool {
        matches!(self, Value::Tag(..))
    }

    /// If the value is a tag, returns the tag number and a reference to the
    /// inner value. Returns `None` otherwise.
    #[inline]
    pub fn as_tag(&self) -> Option<(u64, &Value)> {
        match self {
            Value::Tag(tag, data) => Some((*tag, data)),
            _ => None,
        }
    }

    /// If the value is a tag, returns the pair of the tag number and the
    /// inner value as `Ok`. Returns `Err(self)` otherwise.
    #[inline]
    pub fn into_tag(self) -> Result<(u64, Box<Value>), Self> {
        match self {
            Value::Tag(tag, data) => Ok((tag, data)),
            other => Err(other),
        }
    }

    /// If the value is a byte string, returns a mutable reference to it.
    /// Returns `None` otherwise.
    #[inline]
    pub fn as_bytes_mut(&mut self) -> Option<&mut Vec<u8>> {
        match self {
            Value::Bytes(x) => Some(x),
            _ => None,
        }
    }

    /// If the value is a text string, returns a mutable reference to it.
    /// Returns `None` otherwise.
    #[inline]
    pub fn as_text_mut(&mut self) -> Option<&mut String> {
        match self {
            Value::Text(x) => Some(x),
            _ => None,
        }
    }

    /// If the value is an array, returns a mutable reference to it. Returns
    /// `None` otherwise.
    #[inline]
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(x) => Some(x),
            _ => None,
        }
    }

    /// If the value is a map, returns a mutable reference to it. Returns
    /// `None` otherwise.
    #[inline]
    pub fn as_map_mut(&mut self) -> Option<&mut Vec<(Value, Value)>> {
        match self {
            Value::Map(x) => Some(x),
            _ => None,
        }
    }

    /// If the value is a tag, returns mutable references to the tag number
    /// and the inner value. Returns `None` otherwise.
    #[inline]
    pub fn as_tag_mut(&mut self) -> Option<(&mut u64, &mut Value)> {
        match self {
            Value::Tag(tag, data) => Some((tag, data.as_mut())),
            _ => None,
        }
    }
}

/// Formats the value in CBOR diagnostic notation (RFC 8949 §8).
///
/// Byte strings appear as `h'..'`, text is escaped to pure ASCII in the
/// style of RFC 8949 Appendix A, floats always carry a decimal point or
/// exponent and bignum tags (2 and 3) are written as plain integers.
///
/// ```
/// use cbor2::{cbor, Value};
///
/// let value = cbor!({ "k": [1, -2.5, null] }).unwrap();
/// assert_eq!(value.to_string(), r#"{"k": [1, -2.5, null]}"#);
/// ```
impl core::fmt::Display for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut out = String::new();
        crate::diag::write_value(&mut out, self);
        f.write_str(&out)
    }
}

macro_rules! implfrom {
    ($($variant:ident($t:ty)),+ $(,)?) => {
        $(
            impl From<$t> for Value {
                #[inline]
                fn from(value: $t) -> Self {
                    Self::$variant(value.into())
                }
            }
        )+
    };
}

implfrom! {
    Integer(Integer),
    Integer(u64),
    Integer(i64),
    Integer(u32),
    Integer(i32),
    Integer(u16),
    Integer(i16),
    Integer(u8),
    Integer(i8),

    Bytes(Vec<u8>),
    Bytes(&[u8]),

    Float(f64),
    Float(f32),

    Text(String),
    Text(&str),

    Bool(bool),

    Array(&[Value]),
    Array(Vec<Value>),

    Map(&[(Value, Value)]),
    Map(Vec<(Value, Value)>),
}

impl From<u128> for Value {
    #[inline]
    fn from(value: u128) -> Self {
        if let Ok(x) = Integer::try_from(value) {
            return Value::Integer(x);
        }

        let mut bytes = &value.to_be_bytes()[..];
        while let Some(0) = bytes.first() {
            bytes = &bytes[1..];
        }

        Value::Tag(crate::core::tag::BIGPOS, Value::Bytes(bytes.into()).into())
    }
}

impl From<i128> for Value {
    #[inline]
    fn from(value: i128) -> Self {
        if let Ok(x) = Integer::try_from(value) {
            return Value::Integer(x);
        }

        let (tag, raw) = match value.is_negative() {
            true => (crate::core::tag::BIGNEG, value as u128 ^ !0),
            false => (crate::core::tag::BIGPOS, value as u128),
        };

        let mut bytes = &raw.to_be_bytes()[..];
        while let Some(0) = bytes.first() {
            bytes = &bytes[1..];
        }

        Value::Tag(tag, Value::Bytes(bytes.into()).into())
    }
}

impl From<char> for Value {
    #[inline]
    fn from(value: char) -> Self {
        let mut v = String::with_capacity(value.len_utf8());
        v.push(value);
        Value::Text(v)
    }
}

impl From<std::borrow::Cow<'_, str>> for Value {
    #[inline]
    fn from(value: std::borrow::Cow<'_, str>) -> Self {
        Value::Text(value.into_owned())
    }
}

impl<const N: usize> From<[u8; N]> for Value {
    #[inline]
    fn from(value: [u8; N]) -> Self {
        Value::Bytes(value.into())
    }
}

impl<const N: usize> From<&[u8; N]> for Value {
    #[inline]
    fn from(value: &[u8; N]) -> Self {
        Value::Bytes(value.into())
    }
}

#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
impl From<usize> for Value {
    #[inline]
    fn from(value: usize) -> Self {
        Value::Integer(value.into())
    }
}

#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
impl From<isize> for Value {
    #[inline]
    fn from(value: isize) -> Self {
        Value::Integer(value.into())
    }
}

/// Converts `Some` to the inner value and `None` to [`Value::Null`].
impl<T: Into<Value>> From<Option<T>> for Value {
    #[inline]
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => value.into(),
            None => Value::Null,
        }
    }
}

/// Converts to [`Value::Map`], keeping the map's iteration order — which
/// a `HashMap` randomizes. Encode with the `to_canonical_*` functions (or
/// [`canonicalize`](Value::canonicalize) first) when a deterministic
/// order matters.
///
/// ```
/// use std::collections::HashMap;
/// use cbor2::Value;
///
/// let map: HashMap<&str, u64> = [("a", 1)].into();
/// assert_eq!(Value::from(map), cbor2::cbor!({ "a": 1 }).unwrap());
/// ```
impl<K: Into<Value>, V: Into<Value>> From<HashMap<K, V>> for Value {
    fn from(value: HashMap<K, V>) -> Self {
        Value::Map(
            value
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

/// Converts to [`Value::Map`] in the map's order: ascending by key.
impl<K: Into<Value>, V: Into<Value>> From<BTreeMap<K, V>> for Value {
    fn from(value: BTreeMap<K, V>) -> Self {
        Value::Map(
            value
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

/// Collects an iterator of values into [`Value::Array`].
///
/// ```
/// use cbor2::Value;
///
/// let value: Value = (1..=3).collect();
/// assert_eq!(value, cbor2::cbor!([1, 2, 3]).unwrap());
/// ```
impl<T: Into<Value>> FromIterator<T> for Value {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Value::Array(iter.into_iter().map(Into::into).collect())
    }
}

// A serde-style "invalid type" error for a failed conversion.
fn invalid_type(value: &Value, expected: &'static str) -> Error {
    serde::de::Error::invalid_type(value.into(), &expected)
}

macro_rules! tryfrom_value {
    ($($t:ty: $variant:ident => $expected:literal,)+) => {$(
        #[doc = concat!("Converts from [`Value::", stringify!($variant), "`]; any other variant is an `\"invalid type\"` error.")]
        ///
        /// The [`Value::into_*`](Value::into_bytes) accessors are the
        /// non-consuming-on-failure alternative: they hand the original
        /// value back instead of an error message.
        impl TryFrom<Value> for $t {
            type Error = Error;

            #[inline]
            fn try_from(value: Value) -> Result<Self, Error> {
                match value {
                    Value::$variant(x) => Ok(x),
                    other => Err(invalid_type(&other, $expected)),
                }
            }
        }
    )+};
}

tryfrom_value! {
    Integer: Integer => "integer",
    Vec<u8>: Bytes => "bytes",
    f64: Float => "float",
    String: Text => "text",
    bool: Bool => "bool",
    Vec<Value>: Array => "array",
    Vec<(Value, Value)>: Map => "map",
}

/// Converts from a single-character [`Value::Text`].
impl TryFrom<Value> for char {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Error> {
        if let Value::Text(text) = &value {
            let mut chars = text.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                return Ok(c);
            }
        }

        Err(invalid_type(&value, "a single-character text"))
    }
}

macro_rules! tryfrom_int {
    ($($(#[$($attr:meta)+])? $t:ident)+) => {$(
        $(#[$($attr)+])?
        #[doc = concat!("Converts from a [`Value::Integer`] in `", stringify!($t), "` range.")]
        impl TryFrom<Value> for $t {
            type Error = Error;

            #[inline]
            fn try_from(value: Value) -> Result<Self, Error> {
                match value {
                    Value::Integer(x) => $t::try_from(x).map_err(|_| {
                        Error::Custom(format!(
                            concat!(
                                "invalid value: integer `{}`, expected ",
                                stringify!($t),
                            ),
                            i128::from(x),
                        ))
                    }),
                    other => Err(invalid_type(&other, "integer")),
                }
            }
        }
    )+};
}

tryfrom_int! {
    u8 u16 u32 u64
    i8 i16 i32 i64

    #[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
    usize

    #[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
    isize
}

/// Converts from any integer representation that fits, including the
/// bignum form (tag 2 or 3) that [`From<u128>`](Value#impl-From%3Cu128%3E-for-Value)
/// produces for values beyond 64 bits.
impl TryFrom<Value> for u128 {
    type Error = Error;

    #[inline]
    fn try_from(value: Value) -> Result<Self, Error> {
        value.deserialized()
    }
}

/// Converts from any integer representation that fits, including the
/// bignum form (tag 2 or 3) that [`From<i128>`](Value#impl-From%3Ci128%3E-for-Value)
/// produces for values beyond 64 bits.
impl TryFrom<Value> for i128 {
    type Error = Error;

    #[inline]
    fn try_from(value: Value) -> Result<Self, Error> {
        value.deserialized()
    }
}

// Converts the entries of a map, reporting the first failure.
fn map_entries<K, V, M>(pairs: Vec<(Value, Value)>) -> Result<M, Error>
where
    K: TryFrom<Value>,
    K::Error: core::fmt::Display,
    V: TryFrom<Value>,
    V::Error: core::fmt::Display,
    M: FromIterator<(K, V)>,
{
    pairs
        .into_iter()
        .map(|(k, v)| {
            Ok((
                K::try_from(k).map_err(|err| Error::Custom(format!("invalid map key: {err}")))?,
                V::try_from(v).map_err(|err| Error::Custom(format!("invalid map value: {err}")))?,
            ))
        })
        .collect()
}

/// Converts from [`Value::Map`], converting every key and value in turn;
/// later duplicate keys overwrite earlier ones. For deep, typed
/// extraction with detailed errors prefer
/// [`Value::deserialized`](Value::deserialized).
///
/// ```
/// use std::collections::HashMap;
/// use cbor2::Value;
///
/// let value = cbor2::cbor!({ "a": 1, "b": 2 }).unwrap();
/// let map: HashMap<String, u64> = value.try_into().unwrap();
/// assert_eq!(map["a"], 1);
/// ```
impl<K, V> TryFrom<Value> for HashMap<K, V>
where
    K: TryFrom<Value> + Eq + std::hash::Hash,
    K::Error: core::fmt::Display,
    V: TryFrom<Value>,
    V::Error: core::fmt::Display,
{
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Error> {
        match value {
            Value::Map(pairs) => map_entries(pairs),
            other => Err(invalid_type(&other, "map")),
        }
    }
}

/// Converts from [`Value::Map`], converting every key and value in turn;
/// later duplicate keys overwrite earlier ones.
impl<K, V> TryFrom<Value> for BTreeMap<K, V>
where
    K: TryFrom<Value> + Ord,
    K::Error: core::fmt::Display,
    V: TryFrom<Value>,
    V::Error: core::fmt::Display,
{
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Error> {
        match value {
            Value::Map(pairs) => map_entries(pairs),
            other => Err(invalid_type(&other, "map")),
        }
    }
}
