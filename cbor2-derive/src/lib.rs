//! Derive support for protocol-shaped CBOR with `cbor2`.
//!
//! This crate provides the implementation behind `#[derive(cbor2::Cbor)]`.
//! Users normally enable it through the `derive` feature of the `cbor2`
//! crate:
//!
//! ```toml
//! [dependencies]
//! cbor2 = { version = "1", features = ["derive"] }
//! serde_bytes = "0.11" # only needed for binary fields like the example below
//! ```
//!
//! The derive generates `serde::Serialize` and `serde::Deserialize` impls
//! for CBOR protocols that need integer map keys, field-order arrays and
//! semantic tags, such as COSE (RFC 9052). Map-shaped structs can also use
//! `#[serde(flatten)]` for extension fields beside the registered integer-key
//! subset. It implements `cbor2::Cbor`, exposing the declared keys, tag and
//! array shape as runtime metadata. The original Rust field names stay intact
//! for JSON and other serde formats.
//!
//! A type using `#[serde(flatten)]` takes a buffered code path that
//! dispatches on `is_human_readable()`: human-readable formats (JSON, ...)
//! see the plain field names, while non-human-readable formats are routed
//! through a dynamic `cbor2::Value` to remap the integer keys. That routing
//! assumes the binary format is self-describing like CBOR; flattened types
//! are not supported in non-self-describing binary formats such as bincode.
//! Types without `#[serde(flatten)]` have no such restriction.
//!
//! ```ignore
//! use cbor2::Cbor;
//!
//! #[derive(Debug, PartialEq, Cbor)]
//! #[cbor(tag = 18)]
//! struct CoseHeader {
//!     #[cbor(key = 1)]
//!     alg: i8,
//!     #[cbor(key = 4)]
//!     #[serde(with = "serde_bytes")]
//!     kid: Vec<u8>,
//! }
//!
//! assert_eq!(CoseHeader::KEYS, &[("alg", 1), ("kid", 4)]);
//! assert_eq!(CoseHeader::TAG, Some(18));
//! ```

use core::fmt::Write as _;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned as _;

// The marker prefix recognized by the `cbor2` serializers. Keep in sync
// with `cbor2::ser::STRUCT_MARKER`; the integration tests of the `cbor2`
// crate pin the resulting wire bytes.
const MARKER: &str = "@@CBOR@@";

/// Derives `serde::Serialize` and `serde::Deserialize` with CBOR protocol
/// details: integer map keys (`#[cbor(key = <integer>)]` on fields),
/// field-order array structs (`#[cbor(array)]` on the container) and a
/// CBOR tag (`#[cbor(tag = <integer>)]` on the container). The tag is
/// written on encode and transparent on decode, so input is accepted with
/// or without it. The declared details are also exposed through an
/// implementation of the `cbor2::Cbor` trait, so the generated code
/// requires the `cbor2` crate under that name.
///
/// Do not also derive serde's `Serialize`/`Deserialize`: this macro
/// generates both impls (the implementations would conflict).
#[proc_macro_derive(Cbor, attributes(cbor, serde))]
pub fn derive_cbor(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    expand(item.into())
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

fn expand(item: TokenStream) -> syn::Result<TokenStream> {
    let input: syn::DeriveInput = syn::parse2(item)?;

    if let Some(lifetime) = input
        .generics
        .lifetimes()
        .find(|def| def.lifetime.ident == "de")
    {
        return Err(syn::Error::new(
            lifetime.lifetime.span(),
            "#[derive(Cbor)] cannot support a lifetime named 'de because serde's \
             Deserialize derive reserves that name; rename the lifetime",
        ));
    }

    let container = container_attrs(&input.attrs)?;
    let serde = scan_serde(&input.attrs);
    if let Some(span) = serde.rename.map(|(_, span)| span).or(serde.split_rename) {
        return Err(syn::Error::new(
            span,
            "#[derive(Cbor)] does not support a container-level #[serde(rename = ...)]; \
             rename the type itself",
        ));
    }

    let mut entries = Vec::new();
    let mut flatten = false;
    match &input.data {
        syn::Data::Struct(data) => {
            for entry in field_entries(&data.fields)? {
                merge_entry(&mut entries, entry)?;
            }
            if let Some(span) = fields_have_flatten(&data.fields) {
                flatten = true;
                if !matches!(data.fields, syn::Fields::Named(..)) {
                    return Err(syn::Error::new(
                        span,
                        "#[serde(flatten)] with #[derive(Cbor)] requires a struct with named fields",
                    ));
                }
                if let Some(array) = container.array {
                    return Err(syn::Error::new(
                        array,
                        "#[serde(flatten)] cannot be used with #[cbor(array)]",
                    ));
                }
            }

            if let Some(span) = container.array {
                if !matches!(data.fields, syn::Fields::Named(..)) {
                    return Err(syn::Error::new(
                        span,
                        "#[cbor(array)] requires a struct with named fields",
                    ));
                }
                if let Some(entry) = entries.first() {
                    return Err(syn::Error::new(
                        entry.span,
                        "#[cbor(key = ...)] cannot be used with #[cbor(array)]",
                    ));
                }
            }

            if !entries.is_empty() {
                if let Some(span) = serde.rename_all {
                    return Err(syn::Error::new(
                        span,
                        "#[serde(rename_all = ...)] is not supported with \
                         #[cbor(key = ...)]; rename the fields explicitly",
                    ));
                }
            }
        }

        syn::Data::Enum(data) => {
            if let Some(tag) = &container.tag {
                return Err(syn::Error::new(
                    tag.span,
                    "`tag = ...` is not supported on enums",
                ));
            }
            if let Some(span) = container.array {
                return Err(syn::Error::new(span, "`array` is not supported on enums"));
            }

            for variant in &data.variants {
                if let Some(attr) = variant.attrs.iter().find(|a| a.path().is_ident("cbor")) {
                    return Err(syn::Error::new(
                        attr.span(),
                        "#[cbor(...)] is not supported on enum variants",
                    ));
                }

                let keyed = field_entries(&variant.fields)?;
                if let Some(span) = fields_have_flatten(&variant.fields) {
                    return Err(syn::Error::new(
                        span,
                        "#[serde(flatten)] with #[derive(Cbor)] is supported only on structs",
                    ));
                }
                if !keyed.is_empty() {
                    if let Some(span) = scan_serde(&variant.attrs).rename_all {
                        return Err(syn::Error::new(
                            span,
                            "#[serde(rename_all = ...)] is not supported with \
                             #[cbor(key = ...)]; rename the fields explicitly",
                        ));
                    }
                }
                for entry in keyed {
                    merge_entry(&mut entries, entry)?;
                }
            }

            if !entries.is_empty() {
                if let Some(span) = serde.rename_all_fields {
                    return Err(syn::Error::new(
                        span,
                        "#[serde(rename_all_fields = ...)] is not supported with \
                         #[cbor(key = ...)]; rename the fields explicitly",
                    ));
                }
                if let Some(span) = serde.enum_repr {
                    return Err(syn::Error::new(
                        span,
                        "only externally tagged enums support #[cbor(key = ...)]",
                    ));
                }
            }
        }

        syn::Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "Cbor supports structs and enums",
            ));
        }
    }

    // These container shapes make serde bypass the container name — and
    // with it the marker that carries the declared protocol details, which
    // would otherwise be dropped silently.
    if container.tag.is_some() || container.array.is_some() || !entries.is_empty() {
        if let Some(span) = serde.transparent {
            return Err(syn::Error::new(
                span,
                "#[serde(transparent)] bypasses the container, so the declared \
                 #[cbor(...)] tag, array shape or keys would be silently ignored",
            ));
        }
        if let Some(span) = serde.into {
            return Err(syn::Error::new(
                span,
                "#[serde(into = ...)] serializes through another type, so the declared \
                 #[cbor(...)] tag, array shape or keys would be silently ignored on encode",
            ));
        }
    }

    Ok(generate(
        &input,
        container.tag.as_ref().map(|tag| tag.value),
        container.array.is_some(),
        flatten,
        &entries,
    ))
}

// Generates the serde impls: a hidden *shadow* of the item carrying the
// marker rename plus `#[serde(remote = ...)]`, and two impls delegating
// to the shadow's generated functions. The shadow accesses the real
// type's fields directly, so nothing is copied at runtime, and the real
// type's name and field names stay exactly as written.
fn generate(
    input: &syn::DeriveInput,
    tag: Option<u64>,
    array: bool,
    flatten: bool,
    entries: &[Entry],
) -> TokenStream {
    let ident = &input.ident;
    let shadow_ident = format_ident!("__CborShadow");

    let mut shadow = input.clone();
    shadow.ident = shadow_ident.clone();
    shadow.attrs = copied_attrs(&input.attrs);
    match &mut shadow.data {
        syn::Data::Struct(data) => {
            for field in data.fields.iter_mut() {
                field.attrs = copied_attrs(&field.attrs);
            }
        }
        syn::Data::Enum(data) => {
            for variant in data.variants.iter_mut() {
                variant.attrs = copied_attrs(&variant.attrs);
                for field in variant.fields.iter_mut() {
                    field.attrs = copied_attrs(&field.attrs);
                }
            }
        }
        syn::Data::Union(..) => unreachable!("rejected above"),
    }

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // The remote path: the real type, as seen from inside the const
    // block. serde applies the shadow's own generics to it, so the path
    // itself must not carry generic arguments.
    let remote = ident.to_string();

    let mut head = vec![
        syn::parse_quote!(#[derive(::serde::Serialize, ::serde::Deserialize)]),
        syn::parse_quote!(#[serde(remote = #remote)]),
        syn::parse_quote!(#[automatically_derived]),
    ];
    if let Some(marker) = marker(tag, array, entries, ident) {
        head.push(syn::parse_quote!(#[serde(rename = #marker)]));
    }
    head.append(&mut shadow.attrs);
    shadow.attrs = head;

    // `T: Serialize` / `T: Deserialize<'de>` bounds, like serde's derive.
    let mut ser_generics = input.generics.clone();
    for param in ser_generics.type_params_mut() {
        param.bounds.push(syn::parse_quote!(::serde::Serialize));
    }
    let (ser_impl_generics, ..) = ser_generics.split_for_impl();

    let de_lifetime = fresh_de_lifetime(&input.generics);
    let mut de_generics = input.generics.clone();
    for param in de_generics.type_params_mut() {
        param
            .bounds
            .push(syn::parse_quote!(::serde::Deserialize<#de_lifetime>));
    }
    let mut de_lifetime_param = syn::LifetimeParam::new(de_lifetime.clone());
    de_lifetime_param
        .bounds
        .extend(input.generics.lifetimes().map(|def| def.lifetime.clone()));
    de_generics
        .params
        .insert(0, syn::GenericParam::Lifetime(de_lifetime_param));
    let (de_impl_generics, ..) = de_generics.split_for_impl();

    let serde_impls = if flatten {
        let cbor_lifetime = fresh_lifetime(&input.generics, "__cbor");

        let mut shadow_ref_generics = input.generics.clone();
        shadow_ref_generics.params.insert(
            0,
            syn::GenericParam::Lifetime(syn::LifetimeParam::new(cbor_lifetime.clone())),
        );
        let (shadow_ref_impl_generics, _shadow_ref_ty_generics, shadow_ref_where_clause) =
            shadow_ref_generics.split_for_impl();

        let mut ser_ref_generics = ser_generics.clone();
        ser_ref_generics.params.insert(
            0,
            syn::GenericParam::Lifetime(syn::LifetimeParam::new(cbor_lifetime.clone())),
        );
        let (ser_ref_impl_generics, ser_ref_ty_generics, ser_ref_where_clause) =
            ser_ref_generics.split_for_impl();

        quote! {
            struct __CborShadowRef #shadow_ref_impl_generics #shadow_ref_where_clause {
                value: &#cbor_lifetime #ident #ty_generics,
            }

            impl #ser_ref_impl_generics ::serde::Serialize for __CborShadowRef #ser_ref_ty_generics #ser_ref_where_clause {
                fn serialize<__S>(&self, serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #shadow_ident::serialize(self.value, serializer)
                }
            }

            struct __CborShadowOwned #impl_generics (#ident #ty_generics) #where_clause;

            impl #de_impl_generics ::serde::Deserialize<#de_lifetime> for __CborShadowOwned #ty_generics #where_clause {
                fn deserialize<__D>(deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<#de_lifetime>,
                {
                    #shadow_ident::deserialize(deserializer).map(Self)
                }
            }

            #[automatically_derived]
            impl #ser_impl_generics ::serde::Serialize for #ident #ty_generics #where_clause {
                fn serialize<__S>(&self, serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    if serializer.is_human_readable() {
                        return #shadow_ident::serialize(self, serializer);
                    }

                    let __value = ::cbor2::Value::serialized(&__CborShadowRef { value: self })
                        .map_err(::serde::ser::Error::custom)?;
                    let __value = ::cbor2::__private::__cbor2_flatten_serialize(
                        __value,
                        <#ident #ty_generics as ::cbor2::Cbor>::TAG,
                        <#ident #ty_generics as ::cbor2::Cbor>::KEYS,
                    )
                    .map_err(::serde::ser::Error::custom)?;
                    ::serde::Serialize::serialize(&__value, serializer)
                }
            }

            #[automatically_derived]
            impl #de_impl_generics ::serde::Deserialize<#de_lifetime> for #ident #ty_generics #where_clause {
                fn deserialize<__D>(deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<#de_lifetime>,
                {
                    if deserializer.is_human_readable() {
                        return #shadow_ident::deserialize(deserializer);
                    }

                    let __value: ::cbor2::Value =
                        ::serde::Deserialize::deserialize(deserializer)?;
                    let __value = ::cbor2::__private::__cbor2_flatten_deserialize(
                        __value,
                        <#ident #ty_generics as ::cbor2::Cbor>::KEYS,
                    )
                    .map_err(::serde::de::Error::custom)?;
                    let __value: __CborShadowOwned #ty_generics =
                        ::cbor2::__private::__cbor2_flatten_deserialize_value(&__value)
                        .map_err(::serde::de::Error::custom)?;
                    ::core::result::Result::Ok(__value.0)
                }
            }
        }
    } else {
        quote! {
            #[automatically_derived]
            impl #ser_impl_generics ::serde::Serialize for #ident #ty_generics #where_clause {
                fn serialize<__S>(&self, serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    #shadow_ident::serialize(self, serializer)
                }
            }

            #[automatically_derived]
            impl #de_impl_generics ::serde::Deserialize<#de_lifetime> for #ident #ty_generics #where_clause {
                fn deserialize<__D>(deserializer: __D) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<#de_lifetime>,
                {
                    #shadow_ident::deserialize(deserializer)
                }
            }
        }
    };

    // The `cbor2::Cbor` trait exposes the declared protocol details.
    let key_pairs = entries.iter().map(|entry| {
        let name = &entry.name;
        let key = entry.key;
        quote!((#name, #key))
    });
    let tag_const = match tag {
        Some(tag) => quote!(::core::option::Option::Some(#tag)),
        None => quote!(::core::option::Option::None),
    };
    let array_const = array;

    quote! {
        #[doc(hidden)]
        const _: () = {
            #shadow

            #serde_impls

            #[automatically_derived]
            impl #impl_generics ::cbor2::Cbor for #ident #ty_generics #where_clause {
                const KEYS: &'static [(&'static str, i128)] = &[#(#key_pairs),*];
                const TAG: ::core::option::Option<u64> = #tag_const;
                const ARRAY: bool = #array_const;
            }
        };
    }
}

// Picks an internal deserializer lifetime that cannot collide with the
// user's generics. User code may legitimately name a lifetime `'de`.
fn fresh_de_lifetime(generics: &syn::Generics) -> syn::Lifetime {
    fresh_lifetime(generics, "__de")
}

fn fresh_lifetime(generics: &syn::Generics, base: &str) -> syn::Lifetime {
    let mut name = String::from(base);
    while generics.lifetimes().any(|def| def.lifetime.ident == name) {
        name.push('_');
    }

    syn::Lifetime::new(&format!("'{name}"), proc_macro2::Span::call_site())
}

// The attributes that carry over to the shadow: serde configuration,
// conditional compilation, and lint silencing — the shadow repeats the
// user's field and variant names, so an `#[allow]` on the original must
// silence the shadow too. Everything else — docs, derives, `#[cbor]` —
// stays behind.
fn copied_attrs(attrs: &[syn::Attribute]) -> Vec<syn::Attribute> {
    attrs
        .iter()
        .filter(|attr| {
            let path = attr.path();
            path.is_ident("serde")
                || path.is_ident("cfg")
                || path.is_ident("cfg_attr")
                || path.is_ident("allow")
                || path.is_ident("expect")
        })
        .cloned()
        .collect()
}

// The `@@CBOR@@<tag>@@<keys>@@<name>` container marker, when the item
// declares a tag, array shape or integer keys.
fn marker(tag: Option<u64>, array: bool, entries: &[Entry], ident: &syn::Ident) -> Option<String> {
    if tag.is_none() && entries.is_empty() && !array {
        return None;
    }

    let mut marker = String::from(MARKER);
    if let Some(tag) = tag {
        let _ = write!(&mut marker, "{tag}");
    }
    marker.push_str("@@");
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            marker.push(';');
        }
        let _ = write!(&mut marker, "{}={}", entry.name, entry.key);
    }
    marker.push_str("@@");
    if array {
        marker.push_str("array@@");
    }
    let name = ident.to_string();
    marker.push_str(name.strip_prefix("r#").unwrap_or(&name));

    Some(marker)
}

// `tag = <integer>` inside the container's `#[cbor(...)]`.
struct TagArg {
    value: u64,
    span: proc_macro2::Span,
}

// `key = <integer>` inside a field's `#[cbor(...)]`.
struct KeyArg {
    value: i128,
    span: proc_macro2::Span,
}

impl Parse for KeyArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        const RANGE: &str = "#[cbor(key = ...)] must fit a CBOR integer (-2^64 ..= 2^64 - 1)";

        let name: syn::Ident = input.parse()?;
        if name != "key" {
            return Err(syn::Error::new(name.span(), "expected `key = <integer>`"));
        }
        input.parse::<syn::Token![=]>()?;

        let negative = input.peek(syn::Token![-]);
        if negative {
            input.parse::<syn::Token![-]>()?;
        }
        let literal: syn::LitInt = input.parse()?;

        // `base10_parse` ignores a type suffix; a suffixed key would be
        // accepted with the suffix silently meaning nothing.
        if !literal.suffix().is_empty() {
            return Err(syn::Error::new(
                literal.span(),
                "#[cbor(key = ...)] does not accept a suffixed integer literal",
            ));
        }

        // A `LitInt` is already a valid integer, so the only parse failure
        // left is overflow; report it as the CBOR range.
        let magnitude: i128 = literal
            .base10_parse()
            .map_err(|_| syn::Error::new(literal.span(), RANGE))?;
        let value = if negative { -magnitude } else { magnitude };

        Ok(KeyArg {
            value,
            span: literal.span(),
        })
    }
}

struct ContainerAttrs {
    tag: Option<TagArg>,
    array: Option<proc_macro2::Span>,
}

// Reads the container-level `#[cbor(tag = ..., array)]` attribute.
fn container_attrs(attrs: &[syn::Attribute]) -> syn::Result<ContainerAttrs> {
    let mut out = ContainerAttrs {
        tag: None,
        array: None,
    };

    for attr in attrs {
        if !attr.path().is_ident("cbor") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("tag") {
                const RANGE: &str = "tag must fit a CBOR tag (0 ..= 2^64 - 1)";
                let value = meta.value()?;
                if value.peek(syn::Token![-]) {
                    return Err(syn::Error::new(value.span(), RANGE));
                }
                let literal: syn::LitInt = value.parse()?;
                if !literal.suffix().is_empty() {
                    return Err(syn::Error::new(
                        literal.span(),
                        "#[cbor(tag = ...)] does not accept a suffixed integer literal",
                    ));
                }
                let tag = TagArg {
                    value: literal
                        .base10_parse()
                        .map_err(|_| syn::Error::new(literal.span(), RANGE))?,
                    span: literal.span(),
                };
                if out.tag.replace(tag).is_some() {
                    return Err(syn::Error::new(
                        meta.path.span(),
                        "duplicate #[cbor(tag = ...)] attribute",
                    ));
                }
                Ok(())
            } else if meta.path.is_ident("array") {
                if meta.input.peek(syn::Token![=]) {
                    return Err(syn::Error::new(
                        meta.path.span(),
                        "expected `array` without a value",
                    ));
                }
                if out.array.replace(meta.path.span()).is_some() {
                    return Err(syn::Error::new(
                        meta.path.span(),
                        "duplicate #[cbor(array)] attribute",
                    ));
                }
                Ok(())
            } else {
                Err(syn::Error::new(
                    meta.path.span(),
                    "expected `tag = <integer>` or `array`",
                ))
            }
        })?;
    }

    Ok(out)
}

// One `<name>=<key>` entry of the marker's key table.
struct Entry {
    name: String,
    key: i128,
    span: proc_macro2::Span,
}

// Adds an entry, rejecting ambiguous mappings. Identical mappings merge,
// so enum variants may share a field.
fn merge_entry(entries: &mut Vec<Entry>, entry: Entry) -> syn::Result<()> {
    match entries
        .iter()
        .find(|e| e.name == entry.name || e.key == entry.key)
    {
        Some(e) if e.name == entry.name && e.key == entry.key => Ok(()),
        Some(e) if e.name == entry.name => Err(syn::Error::new(
            entry.span,
            format!(
                "field `{}` maps to conflicting keys {} and {}",
                entry.name, e.key, entry.key
            ),
        )),
        Some(e) => Err(syn::Error::new(
            entry.span,
            format!("key {} is already mapped to field `{}`", entry.key, e.name),
        )),
        None => {
            entries.push(entry);
            Ok(())
        }
    }
}

// Reads the `#[cbor(key = ...)]` field attributes into key table entries
// under the fields' serde names.
fn field_entries(fields: &syn::Fields) -> syn::Result<Vec<Entry>> {
    let mut entries = Vec::new();

    for field in fields {
        let mut key: Option<KeyArg> = None;
        for attr in &field.attrs {
            if !attr.path().is_ident("cbor") {
                continue;
            }

            let arg: KeyArg = attr.parse_args()?;
            if key.replace(arg).is_some() {
                return Err(syn::Error::new(
                    attr.span(),
                    "duplicate #[cbor(key = ...)] attribute",
                ));
            }
        }
        let serde = scan_serde(&field.attrs);
        if let (Some(..), Some(span)) = (&key, serde.flatten) {
            return Err(syn::Error::new(
                span,
                "#[serde(flatten)] cannot be combined with #[cbor(key = ...)]",
            ));
        }
        // A fully skipped field is never on the wire in either direction,
        // so a key on it is a mistake. (The one-directional
        // `skip_serializing`/`skip_deserializing` variants keep the key
        // meaningful and stay allowed.)
        if let (Some(..), Some(span)) = (&key, serde.skip) {
            return Err(syn::Error::new(
                span,
                "#[serde(skip)] cannot be combined with #[cbor(key = ...)]; \
                 the field is never on the wire",
            ));
        }

        let Some(key) = key else { continue };

        if field.ident.is_none() {
            return Err(syn::Error::new(
                key.span,
                "#[cbor(key = ...)] requires a named field",
            ));
        }

        // CBOR integer keys span major types 0 and 1.
        if key.value > u64::MAX as i128 || key.value < -(u64::MAX as i128) - 1 {
            return Err(syn::Error::new(
                key.span,
                "#[cbor(key = ...)] must fit a CBOR integer (-2^64 ..= 2^64 - 1)",
            ));
        }

        if let Some(span) = serde.split_rename {
            return Err(syn::Error::new(
                span,
                "split serialize/deserialize renames are not supported with \
                 #[cbor(key = ...)]",
            ));
        }

        // The key table is consulted with the field's *serde* name, so an
        // explicit rename carries over.
        let name = match serde.rename {
            Some((name, _)) => name,
            None => {
                let ident = field.ident.as_ref().expect("checked above").to_string();
                ident.strip_prefix("r#").unwrap_or(&ident).to_string()
            }
        };

        if name.is_empty() || name.contains(['@', ';', '=']) {
            return Err(syn::Error::new(
                key.span,
                "the serde name of a keyed field may not be empty or contain '@', ';' or '='",
            ));
        }

        entries.push(Entry {
            name,
            key: key.value,
            span: key.span,
        });
    }

    Ok(entries)
}

fn fields_have_flatten(fields: &syn::Fields) -> Option<proc_macro2::Span> {
    fields
        .iter()
        .find_map(|field| scan_serde(&field.attrs).flatten)
}

// The serde attribute metas the marker must coordinate with.
#[derive(Default)]
struct SerdeAttrs {
    rename: Option<(String, proc_macro2::Span)>,
    split_rename: Option<proc_macro2::Span>,
    rename_all: Option<proc_macro2::Span>,
    rename_all_fields: Option<proc_macro2::Span>,
    enum_repr: Option<proc_macro2::Span>,
    flatten: Option<proc_macro2::Span>,
    // Container shapes that bypass the container name — and with it the
    // marker carrying the declared tag, array shape and keys.
    transparent: Option<proc_macro2::Span>,
    into: Option<proc_macro2::Span>,
    // `#[serde(skip)]`: the field is never on the wire in either direction.
    skip: Option<proc_macro2::Span>,
}

// Scans `#[serde(...)]` attributes, tolerating any meta shapes we do not
// understand — the serde derive validates them later anyway.
fn scan_serde(attrs: &[syn::Attribute]) -> SerdeAttrs {
    let mut out = SerdeAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }

        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                if meta.input.peek(syn::Token![=]) {
                    let expr: syn::Expr = meta.value()?.parse()?;
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = expr
                    {
                        out.rename = Some((s.value(), meta.path.span()));
                    }
                    return Ok(());
                }
                out.split_rename = Some(meta.path.span());
            } else if meta.path.is_ident("rename_all") {
                out.rename_all = Some(meta.path.span());
            } else if meta.path.is_ident("rename_all_fields") {
                out.rename_all_fields = Some(meta.path.span());
            } else if meta.path.is_ident("flatten") {
                out.flatten = Some(meta.path.span());
            } else if meta.path.is_ident("transparent") {
                out.transparent = Some(meta.path.span());
            } else if meta.path.is_ident("into") {
                out.into = Some(meta.path.span());
            } else if meta.path.is_ident("skip") {
                out.skip = Some(meta.path.span());
            } else if meta.path.is_ident("tag")
                || meta.path.is_ident("untagged")
                || meta.path.is_ident("content")
            {
                out.enum_repr = Some(meta.path.span());
            }

            if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _: TokenStream = content.parse()?;
            } else if !meta.input.is_empty() && !meta.input.peek(syn::Token![,]) {
                let _: syn::Expr = meta.value()?.parse()?;
            }

            Ok(())
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    fn expanded(item: TokenStream) -> String {
        expand(item).unwrap().to_string()
    }

    fn error(item: TokenStream) -> String {
        expand(item).unwrap_err().to_string()
    }

    #[test]
    fn generates_a_marked_remote_shadow() {
        let out = expanded(quote! {
            #[cbor(tag = 123)]
            struct ProtectedHeader {
                #[cbor(key = 1)]
                alg: i8,
                #[cbor(key = 4)]
                #[serde(with = "serde_bytes")]
                kid: Vec<u8>,
                plain: bool,
            }
        });

        assert!(
            out.contains(r#"rename = "@@CBOR@@123@@alg=1;kid=4@@ProtectedHeader""#),
            "{out}"
        );
        assert!(out.contains(r#"remote = "ProtectedHeader""#), "{out}");
        assert!(out.contains(r#"with = "serde_bytes""#), "{out}");
        assert!(
            out.contains("impl :: serde :: Serialize for ProtectedHeader"),
            "{out}"
        );
        assert!(
            out.contains("impl < '__de > :: serde :: Deserialize < '__de > for ProtectedHeader"),
            "{out}"
        );
        // The #[cbor(...)] attributes stay off the shadow.
        assert!(!out.contains("# [cbor"), "{out}");

        // The declared details surface through the cbor2::Cbor trait.
        assert!(
            out.contains("impl :: cbor2 :: Cbor for ProtectedHeader"),
            "{out}"
        );
        assert!(
            out.contains(r#"const KEYS : & 'static [(& 'static str , i128)] = & [("alg" , 1i128) , ("kid" , 4i128)] ;"#),
            "{out}"
        );
        assert!(
            out.contains(":: core :: option :: Option :: Some (123u64)"),
            "{out}"
        );
    }

    #[test]
    fn generates_plain_serde_impls_without_cbor_attributes() {
        let out = expanded(quote! {
            struct Plain {
                a: u8,
            }
        });

        assert!(!out.contains("@@CBOR@@"), "{out}");
        assert!(out.contains(r#"remote = "Plain""#), "{out}");
        assert!(
            out.contains("impl :: serde :: Serialize for Plain"),
            "{out}"
        );

        // The trait impl is still generated, with an empty table.
        assert!(
            out.contains(r#"const KEYS : & 'static [(& 'static str , i128)] = & [] ;"#),
            "{out}"
        );
        assert!(out.contains(":: core :: option :: Option :: None"), "{out}");
    }

    #[test]
    fn uses_the_serde_rename_as_the_key_table_name() {
        let out = expanded(quote! {
            struct S {
                #[cbor(key = 1)]
                #[serde(rename = "alg", default)]
                algorithm: i8,
            }
        });

        assert!(out.contains(r#"rename = "@@CBOR@@@@alg=1@@S""#), "{out}");
        assert!(out.contains(r#"rename = "alg""#), "{out}");
        assert!(out.contains("default"), "{out}");
    }

    #[test]
    fn supports_field_order_array_structs() {
        let out = expanded(quote! {
            #[cbor(tag = 18, array)]
            struct Sign1 {
                protected: Vec<u8>,
                unprotected: u8,
                payload: Vec<u8>,
                signature: Vec<u8>,
            }
        });

        assert!(
            out.contains(r#"rename = "@@CBOR@@18@@@@array@@Sign1""#),
            "{out}"
        );
        assert!(out.contains("const ARRAY : bool = true"), "{out}");
        assert!(out.contains("impl :: cbor2 :: Cbor for Sign1"), "{out}");
    }

    #[test]
    fn supports_flattened_map_structs() {
        let out = expanded(quote! {
            #[cbor(tag = 61)]
            struct Claims {
                #[cbor(key = 1)]
                #[serde(rename = "iss")]
                issuer: String,
                #[serde(flatten)]
                extra: BTreeMap<String, cbor2::Value>,
            }
        });

        assert!(out.contains("__cbor2_flatten_serialize"), "{out}");
        assert!(out.contains("__cbor2_flatten_deserialize"), "{out}");
        assert!(
            out.contains(r#"rename = "@@CBOR@@61@@iss=1@@Claims""#),
            "{out}"
        );
        assert!(
            out.contains(
                r#"const KEYS : & 'static [(& 'static str , i128)] = & [("iss" , 1i128)] ;"#
            ),
            "{out}"
        );
    }

    #[test]
    fn strips_raw_identifier_prefixes() {
        let out = expanded(quote! {
            struct S {
                #[cbor(key = 1)]
                r#type: u8,
            }
        });

        assert!(out.contains(r#"rename = "@@CBOR@@@@type=1@@S""#), "{out}");
    }

    #[test]
    fn merges_enum_variant_fields() {
        let out = expanded(quote! {
            enum Message {
                Signed {
                    #[cbor(key = 1)]
                    payload: u8,
                },
                Verified {
                    #[cbor(key = 1)]
                    payload: u8,
                    #[cbor(key = 2)]
                    peer: u8,
                },
                Unit,
            }
        });

        assert!(
            out.contains(r#"rename = "@@CBOR@@@@payload=1;peer=2@@Message""#),
            "{out}"
        );
    }

    #[test]
    fn keeps_generics_and_their_bounds() {
        let out = expanded(quote! {
            #[cbor(tag = 7)]
            struct Wrap<T: Clone> {
                #[cbor(key = 1)]
                inner: T,
            }
        });

        assert!(out.contains(r#"remote = "Wrap""#), "{out}");
        assert!(
            out.contains(
                "impl < T : Clone + :: serde :: Serialize > :: serde :: Serialize for Wrap < T >"
            ),
            "{out}"
        );
        assert!(
            out.contains("impl < '__de , T : Clone + :: serde :: Deserialize < '__de > >"),
            "{out}"
        );
        // The trait impl carries the original generics, without serde bounds.
        assert!(
            out.contains("impl < T : Clone > :: cbor2 :: Cbor for Wrap < T >"),
            "{out}"
        );
    }

    #[test]
    fn avoids_deserialize_lifetime_collisions() {
        let out = expanded(quote! {
            struct Borrowed<'a, '__de> {
                #[cbor(key = 1)]
                value: &'a str,
                other: &'__de str,
            }
        });

        assert!(
            out.contains(
                "impl < '__de_ : 'a + '__de , 'a , '__de > :: serde :: Deserialize < '__de_ > for Borrowed < 'a , '__de >"
            ),
            "{out}"
        );
    }

    #[test]
    fn rejects_user_lifetime_named_de() {
        let msg = error(quote! {
            struct Borrowed<'de> {
                value: &'de str,
            }
        });

        assert!(msg.contains("lifetime named 'de"), "{msg}");
    }

    #[test]
    fn accepts_the_full_integer_ranges() {
        let out = expanded(quote! {
            #[cbor(tag = 18446744073709551615)]
            struct Edges {
                #[cbor(key = 0)]
                zero: u8,
                #[cbor(key = 18446744073709551615)]
                hi: u8,
                #[cbor(key = -18446744073709551616)]
                lo: u8,
            }
        });

        assert!(
            out.contains(
                r#"rename = "@@CBOR@@18446744073709551615@@zero=0;hi=18446744073709551615;lo=-18446744073709551616@@Edges""#
            ),
            "{out}"
        );
    }

    #[test]
    fn rejects_invalid_uses() {
        let msg = error(quote! {
            struct S {
                #[cbor(key = 18446744073709551616)]
                a: u8,
            }
        });
        assert!(msg.contains("must fit a CBOR integer"), "{msg}");

        let msg = error(quote! {
            #[cbor(tag = 18446744073709551616)]
            struct S;
        });
        assert!(msg.contains("must fit a CBOR tag"), "{msg}");

        let msg = error(quote! {
            #[cbor(tag = -1)]
            struct S;
        });
        assert!(msg.contains("must fit a CBOR tag"), "{msg}");

        let msg = error(quote! {
            #[cbor(tag = 1)]
            #[cbor(tag = 2)]
            struct S;
        });
        assert!(msg.contains("duplicate #[cbor(tag = ...)]"), "{msg}");

        let msg = error(quote! {
            #[cbor(tag = 1)]
            enum E { A }
        });
        assert!(msg.contains("not supported on enums"), "{msg}");

        let msg = error(quote! {
            #[cbor(array)]
            enum E { A }
        });
        assert!(msg.contains("`array` is not supported on enums"), "{msg}");

        let msg = error(quote! {
            #[cbor(array)]
            struct S(u8);
        });
        assert!(msg.contains("requires a struct with named fields"), "{msg}");

        let msg = error(quote! {
            #[cbor(array)]
            struct S {
                #[cbor(key = 1)]
                a: u8,
            }
        });
        assert!(msg.contains("cannot be used with #[cbor(array)]"), "{msg}");

        let msg = error(quote! {
            #[cbor(array)]
            struct S {
                a: u8,
                #[serde(flatten)]
                extra: BTreeMap<String, u8>,
            }
        });
        assert!(msg.contains("cannot be used with #[cbor(array)]"), "{msg}");

        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                #[cbor(key = 2)]
                a: u8,
            }
        });
        assert!(msg.contains("duplicate #[cbor(key = ...)]"), "{msg}");

        let msg = error(quote! {
            struct S(#[cbor(key = 1)] u8);
        });
        assert!(msg.contains("named field"), "{msg}");

        let msg = error(quote! {
            struct S {
                #[cbor(name = 1)]
                a: u8,
            }
        });
        assert!(msg.contains("expected `key = <integer>`"), "{msg}");

        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                a: u8,
                #[cbor(key = 9)]
                #[serde(flatten)]
                extra: BTreeMap<String, u8>,
            }
        });
        assert!(msg.contains("cannot be combined with #[cbor(key"), "{msg}");

        let msg = error(quote! {
            enum E {
                A {
                    #[serde(flatten)]
                    extra: BTreeMap<String, u8>,
                },
            }
        });
        assert!(msg.contains("supported only on structs"), "{msg}");

        let msg = error(quote! {
            #[cbor(key = 1)]
            struct S {
                a: u8,
            }
        });
        assert!(msg.contains("expected `tag = <integer>`"), "{msg}");

        let msg = error(quote! {
            union U { a: u8 }
        });
        assert!(msg.contains("supports structs and enums"), "{msg}");

        let msg = error(quote! {
            #[cbor(tag = 1, foo)]
            struct S {
                a: u8,
            }
        });
        assert!(
            msg.contains("expected `tag = <integer>` or `array`"),
            "{msg}"
        );
    }

    #[test]
    fn copies_lint_attributes_to_the_shadow() {
        let out = expanded(quote! {
            #[allow(non_snake_case)]
            struct S {
                #[cbor(key = 1)]
                #[allow(unused)]
                fooBar: u8,
            }
        });

        // Both the container-level and the field-level allow survive on the
        // shadow, which repeats the user's names.
        assert!(out.contains("allow (non_snake_case)"), "{out}");
        assert!(out.contains("allow (unused)"), "{out}");
    }

    #[test]
    fn rejects_suffixed_integer_literals() {
        let msg = error(quote! {
            struct S {
                #[cbor(key = 1u8)]
                a: u8,
            }
        });
        assert!(msg.contains("suffixed integer literal"), "{msg}");

        let msg = error(quote! {
            #[cbor(tag = 7u64)]
            struct S {
                a: u8,
            }
        });
        assert!(msg.contains("suffixed integer literal"), "{msg}");
    }

    #[test]
    fn oversized_key_literals_report_the_cbor_range() {
        // Beyond i128: the parse itself fails, but the error still names
        // the CBOR range instead of a generic overflow.
        let msg = error(quote! {
            struct S {
                #[cbor(key = 170141183460469231731687303715884105728)]
                a: u8,
            }
        });
        assert!(msg.contains("must fit a CBOR integer"), "{msg}");
    }

    #[test]
    fn rejects_container_shapes_that_bypass_the_marker() {
        let msg = error(quote! {
            #[serde(transparent)]
            #[cbor(tag = 7)]
            struct S {
                a: u8,
            }
        });
        assert!(msg.contains("silently ignored"), "{msg}");

        let msg = error(quote! {
            #[serde(into = "Other")]
            struct S {
                #[cbor(key = 1)]
                a: u8,
            }
        });
        assert!(msg.contains("silently ignored on encode"), "{msg}");

        // Without any #[cbor(...)] details there is nothing to lose, so
        // both shapes stay allowed.
        let out = expanded(quote! {
            #[serde(transparent)]
            struct S {
                a: u8,
            }
        });
        assert!(out.contains("transparent"), "{out}");
    }

    #[test]
    fn rejects_key_on_fully_skipped_fields() {
        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                #[serde(skip)]
                a: u8,
            }
        });
        assert!(msg.contains("never on the wire"), "{msg}");

        // One-directional skips keep the key meaningful.
        let out = expanded(quote! {
            struct S {
                #[cbor(key = 1)]
                #[serde(skip_serializing_if = "Option::is_none", default)]
                a: Option<u8>,
            }
        });
        assert!(out.contains(r#"rename = "@@CBOR@@@@a=1@@S""#), "{out}");

        // A skipped field without a key stays fine.
        let out = expanded(quote! {
            struct S {
                #[cbor(key = 1)]
                a: u8,
                #[serde(skip)]
                b: u8,
            }
        });
        assert!(out.contains(r#"rename = "@@CBOR@@@@a=1@@S""#), "{out}");
    }

    #[test]
    fn rejects_serde_conflicts() {
        let msg = error(quote! {
            #[serde(rename = "Other")]
            struct S {
                #[cbor(key = 1)]
                a: u8,
            }
        });
        assert!(msg.contains("container-level #[serde(rename"), "{msg}");

        let msg = error(quote! {
            #[serde(rename_all = "camelCase")]
            struct S {
                #[cbor(key = 1)]
                a_b: u8,
            }
        });
        assert!(msg.contains("rename_all"), "{msg}");

        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                #[serde(rename(serialize = "x", deserialize = "y"))]
                a: u8,
            }
        });
        assert!(msg.contains("split serialize/deserialize renames"), "{msg}");

        let msg = error(quote! {
            #[serde(tag = "type")]
            enum E {
                A {
                    #[cbor(key = 1)]
                    a: u8,
                },
            }
        });
        assert!(msg.contains("externally tagged"), "{msg}");

        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                a: u8,
                #[cbor(key = 1)]
                b: u8,
            }
        });
        assert!(msg.contains("already mapped"), "{msg}");

        let msg = error(quote! {
            enum E {
                A {
                    #[cbor(key = 1)]
                    x: u8,
                },
                B {
                    #[cbor(key = 2)]
                    x: u8,
                },
            }
        });
        assert!(msg.contains("conflicting keys"), "{msg}");

        let msg = error(quote! {
            enum E {
                #[cbor(tag = 1)]
                A,
            }
        });
        assert!(msg.contains("not supported on enum variants"), "{msg}");

        // A rename whose value would corrupt the marker grammar.
        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                #[serde(rename = "a=b")]
                a: u8,
            }
        });
        assert!(msg.contains("may not be empty or contain"), "{msg}");
    }
}
