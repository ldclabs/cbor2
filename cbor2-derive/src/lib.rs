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
//! A type using `#[serde(flatten)]` dispatches on `is_human_readable()`:
//! human-readable formats see the plain serde representation. For cbor2,
//! serialization buffers one encoded map and remaps only its keys; decoding
//! passes fields directly to their visitors, preserving borrowed fields,
//! simple values and raw item encodings. Other binary serializers are not
//! supported by this internal raw-item protocol.
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
use syn::ext::IdentExt as _;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned as _;
use syn::visit_mut::{self, VisitMut};

mod bounds;
mod flatten;

// Returns a spanned compile error from the enclosing function or closure.
macro_rules! bail {
    ($span:expr, $($message:tt)+) => {
        return Err(syn::Error::new($span, format!($($message)+)))
    };
}

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
/// uses the dependency name from the caller’s Cargo manifest.
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
        bail!(
            lifetime.lifetime.span(),
            "#[derive(Cbor)] cannot support a lifetime named 'de because serde's \
             Deserialize derive reserves that name; rename the lifetime"
        );
    }

    let container = container_attrs(&input.attrs)?;
    let serde = scan_serde(&input.attrs);
    if let Some(span) = serde
        .rename
        .as_ref()
        .map(|(_, span)| *span)
        .or(serde.split_rename)
    {
        bail!(
            span,
            "#[derive(Cbor)] does not support a container-level #[serde(rename = ...)]; \
             rename the type itself"
        );
    }

    // Parse each field/variant once for validation and impl-bound inference.
    let groups: Vec<_>;
    let mut entries = Vec::new();
    let mut flatten = false;
    match &input.data {
        syn::Data::Struct(data) => {
            groups = vec![FieldGroup::new(&data.fields, SerdeAttrs::default())];
            let fields = &groups[0].fields;
            if container.array.is_some()
                || matches!(&data.fields, syn::Fields::Unnamed(fields) if fields.unnamed.len() > 1)
            {
                validate_positional_fields(fields)?;
            }
            for entry in field_entries(fields)? {
                merge_entry(&mut entries, entry)?;
            }
            if let Some(span) = fields_have_flatten(fields) {
                flatten = true;
                if !matches!(data.fields, syn::Fields::Named(..)) {
                    bail!(
                        span,
                        "#[serde(flatten)] with #[derive(Cbor)] requires a struct \
                         with named fields"
                    );
                }
                if let Some(array) = container.array {
                    bail!(
                        array,
                        "#[serde(flatten)] cannot be used with #[cbor(array)]"
                    );
                }
            }

            if let Some(span) = container.array {
                if !matches!(data.fields, syn::Fields::Named(..)) {
                    bail!(span, "#[cbor(array)] requires a struct with named fields");
                }
                if let Some(entry) = entries.first() {
                    bail!(
                        entry.span,
                        "#[cbor(key = ...)] cannot be used with #[cbor(array)]"
                    );
                }
            }

            if !entries.is_empty() {
                if let Some(span) = serde.rename_all {
                    bail!(
                        span,
                        "#[serde(rename_all = ...)] is not supported with \
                         #[cbor(key = ...)]; rename the fields explicitly"
                    );
                }
            }
        }

        syn::Data::Enum(data) => {
            groups = data
                .variants
                .iter()
                .map(|variant| FieldGroup::new(&variant.fields, scan_serde(&variant.attrs)))
                .collect();
            if let Some(tag) = &container.tag {
                bail!(tag.span, "`tag = ...` is not supported on enums");
            }
            if let Some(span) = container.array {
                bail!(span, "`array` is not supported on enums");
            }

            for (variant, group) in data.variants.iter().zip(&groups) {
                if matches!(&variant.fields, syn::Fields::Unnamed(fields) if fields.unnamed.len() > 1)
                {
                    validate_positional_fields(&group.fields)?;
                }
                if let Some(attr) = variant.attrs.iter().find(|a| a.path().is_ident("cbor")) {
                    bail!(
                        attr.span(),
                        "#[cbor(...)] is not supported on enum variants"
                    );
                }

                let keyed = field_entries(&group.fields)?;
                if let Some(span) = fields_have_flatten(&group.fields) {
                    bail!(
                        span,
                        "#[serde(flatten)] with #[derive(Cbor)] is supported only on structs"
                    );
                }
                if !keyed.is_empty() {
                    if let Some(span) = group.attrs.rename_all {
                        bail!(
                            span,
                            "#[serde(rename_all = ...)] is not supported with \
                             #[cbor(key = ...)]; rename the fields explicitly"
                        );
                    }
                }
                for entry in keyed {
                    merge_entry(&mut entries, entry)?;
                }
            }

            if !entries.is_empty() {
                for group in &groups {
                    if let Some(span) = group.attrs.enum_repr {
                        bail!(
                            span,
                            "untagged variants are not supported in enums with #[cbor(key = ...)]"
                        );
                    }
                }
                validate_enum_keys(&groups, &entries)?;
                if let Some(span) = serde.rename_all_fields {
                    bail!(
                        span,
                        "#[serde(rename_all_fields = ...)] is not supported with \
                         #[cbor(key = ...)]; rename the fields explicitly"
                    );
                }
                if let Some(span) = serde.enum_repr {
                    bail!(
                        span,
                        "only externally tagged enums support #[cbor(key = ...)]"
                    );
                }
            }
        }

        syn::Data::Union(data) => {
            bail!(data.union_token.span(), "Cbor supports structs and enums")
        }
    }

    // These container shapes make serde bypass the container name — and
    // with it the marker that carries the declared protocol details, which
    // would otherwise be dropped silently.
    if container.tag.is_some() || container.array.is_some() || !entries.is_empty() {
        if matches!(input.data, syn::Data::Struct(..)) {
            if let Some(span) = serde.tag {
                bail!(
                    span,
                    "#[serde(tag = ...)] on a struct conflicts with #[cbor(...)] keys, \
                     tags or array shape"
                );
            }
        }
        if let Some(span) = serde.transparent {
            bail!(
                span,
                "#[serde(transparent)] bypasses the container, so the declared \
                 #[cbor(...)] tag, array shape or keys would be silently ignored"
            );
        }
        if let Some(span) = serde.into {
            bail!(
                span,
                "#[serde(into = ...)] serializes through another type, so the declared \
                 #[cbor(...)] tag, array shape or keys would be silently ignored on encode"
            );
        }
        if let Some(span) = serde.from {
            bail!(
                span,
                "#[serde(from = ...)] deserializes through another type, so the declared \
                 #[cbor(...)] tag, array shape or keys would be silently ignored on decode"
            );
        }
        if let Some(span) = serde.try_from {
            bail!(
                span,
                "#[serde(try_from = ...)] deserializes through another type, so the declared \
                 #[cbor(...)] tag, array shape or keys would be silently ignored on decode"
            );
        }
    }

    Ok(generate(
        &input, &container, flatten, &entries, &serde, &groups,
    ))
}

// Generates the serde impls: a hidden *shadow* of the item carrying the
// marker rename plus `#[serde(remote = ...)]`, and two impls delegating
// to the shadow's generated functions. The shadow accesses the real
// type's fields directly, so nothing is copied at runtime, and the real
// type's name and field names stay exactly as written.
fn prepare_shadow(
    input: &syn::DeriveInput,
    runtime: &syn::Ident,
    shadow_ident: &syn::Ident,
    serde: &SerdeAttrs,
    de_generics: &syn::Generics,
    de_lifetime: &syn::Lifetime,
) -> syn::DeriveInput {
    let ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let original: syn::Path = syn::parse_quote!(#ident #ty_generics);

    let mut shadow = input.clone();
    shadow.ident = shadow_ident.clone();
    CopiedAttrs.visit_data_mut(&mut shadow.data);

    // The remote path: the real type, as seen from inside the const
    // block. serde applies the shadow's own generics to it, so the path
    // itself must not carry generic arguments.
    let remote = ident.to_string();

    shadow.attrs = vec![
        syn::parse_quote!(#[serde(remote = #remote)]),
        syn::parse_quote!(#[automatically_derived]),
    ];
    if !serde.explicit_crate {
        let serde_path = format!("::{runtime}::__serde");
        shadow
            .attrs
            .push(syn::parse_quote!(#[serde(crate = #serde_path)]));
    }
    shadow.attrs.extend(copied_attrs(&input.attrs));
    ReplaceSelf { original }.visit_derive_input_mut(&mut shadow);

    if serde.default {
        // The remote visitor constructs the original type: point a bare
        // `default` at its Default impl, and express that bound directly
        // instead of serde's inferred shadow bound.
        let mut path: syn::Path = syn::parse_quote!(#ident #ty_generics);
        if let syn::PathArguments::AngleBracketed(args) =
            &mut path.segments.last_mut().unwrap().arguments
        {
            args.colon2_token = Some(Default::default());
        }
        path.segments.push(syn::parse_quote!(default));
        let path = quote!(#path).to_string();
        edit_serde_metas(&mut shadow.attrs, |metas| {
            *metas = std::mem::take(metas)
                .into_iter()
                .filter(|meta| !meta.path().is_ident("bound"))
                .map(|meta| match meta {
                    syn::Meta::Path(p) if p.is_ident("default") => {
                        syn::parse_quote!(default = #path)
                    }
                    meta => meta,
                })
                .collect();
        });

        let mut predicates = de_generics
            .where_clause
            .as_ref()
            .map(|w| w.predicates.clone())
            .unwrap_or_default();
        for param in de_generics.type_params() {
            let name = &param.ident;
            let bounds = &param.bounds;
            if !bounds.is_empty() {
                predicates.push(syn::parse_quote!(#name: #bounds));
            }
        }
        let serde_de_lifetime = syn::Lifetime::new("'de", proc_macro2::Span::call_site());
        bounds::rename(&mut predicates, de_lifetime, &serde_de_lifetime);
        let de_text = quote!(#predicates).to_string();
        if let Some(ser_bound) = &serde.ser_bound {
            let ser_text = quote!(#ser_bound).to_string();
            shadow.attrs.push(
                syn::parse_quote!(#[serde(bound(serialize = #ser_text, deserialize = #de_text))]),
            );
        } else {
            shadow
                .attrs
                .push(syn::parse_quote!(#[serde(bound(deserialize = #de_text))]));
        }
    }

    shadow
}

fn generate(
    input: &syn::DeriveInput,
    container: &ContainerAttrs,
    flatten: bool,
    entries: &[Entry],
    serde: &SerdeAttrs,
    groups: &[FieldGroup<'_>],
) -> TokenStream {
    let tag = container.tag.as_ref().map(|tag| tag.value);
    let array = container.array.is_some();
    let ident = &input.ident;
    let runtime = match proc_macro_crate::crate_name("cbor2") {
        Ok(proc_macro_crate::FoundCrate::Name(name)) => format_ident!("{}", name),
        _ => format_ident!("cbor2"),
    };
    let type_name = ident.unraw().to_string();
    let name = marker(tag, array, entries, &type_name).unwrap_or_else(|| type_name.clone());
    let shadow_ident = format_ident!("__CborShadow");
    let (ser_generics, de_generics, de_lifetime) = bounds::build(input, serde, groups, &runtime);
    let mut shadow = prepare_shadow(
        input,
        &runtime,
        &shadow_ident,
        serde,
        &de_generics,
        &de_lifetime,
    );
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let (ser_impl_generics, _, ser_where_clause) = ser_generics.split_for_impl();
    let (de_impl_generics, _, de_where_clause) = de_generics.split_for_impl();

    let ordinary_ser = quote!(#shadow_ident::serialize(self, serializer));
    let ordinary_de = quote!(#shadow_ident::deserialize(deserializer));
    let (helpers, serialize, deserialize) = if flatten {
        let expansion = if cfg!(feature = "runtime-1-2-0") {
            flatten::fields(
                &shadow,
                &groups[0].fields,
                serde,
                &runtime,
                &type_name,
                entries,
            )
        } else {
            flatten::legacy(
                input,
                &runtime,
                &ser_generics,
                &de_generics,
                &de_lifetime,
                &shadow_ident,
            )
        };
        let binary_ser = expansion.serialize;
        let binary_de = expansion.deserialize;
        (
            expansion.helpers,
            quote!(if serializer.is_human_readable() {#ordinary_ser} else {#binary_ser}),
            quote!(if deserializer.is_human_readable() {#ordinary_de} else {#binary_de}),
        )
    } else {
        (TokenStream::new(), ordinary_ser, ordinary_de)
    };
    shadow.attrs.insert(0, syn::parse_quote!(#[derive(::#runtime::__serde::Serialize, ::#runtime::__serde::Deserialize)]));
    shadow
        .attrs
        .push(syn::parse_quote!(#[serde(rename = #name)]));
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

    quote! {
        #[doc(hidden)]
        const _: () = {
            #shadow
            #helpers
            #[automatically_derived]
            impl #ser_impl_generics ::#runtime::__serde::Serialize for #ident #ty_generics #ser_where_clause {
                fn serialize<__S: ::#runtime::__serde::Serializer>(&self, serializer: __S) -> ::core::result::Result<__S::Ok,__S::Error> {
                    #serialize
                }
            }
            #[automatically_derived]
            impl #de_impl_generics ::#runtime::__serde::Deserialize<#de_lifetime> for #ident #ty_generics #de_where_clause {
                fn deserialize<__D: ::#runtime::__serde::Deserializer<#de_lifetime>>(deserializer: __D) -> ::core::result::Result<Self,__D::Error> {
                    #deserialize
                }
            }

            #[automatically_derived]
            impl #impl_generics ::#runtime::Cbor for #ident #ty_generics #where_clause {
                const KEYS: &'static [(&'static str, i128)] = &[#(#key_pairs),*];
                const TAG: ::core::option::Option<u64> = #tag_const;
                const ARRAY: bool = #array;
            }
        };
    }
}

fn validate_positional_fields(fields: &[FieldInfo<'_>]) -> syn::Result<()> {
    for field in fields {
        let attrs = &field.attrs;
        if attrs.skip.is_none() {
            if let Some(span) = attrs.positional_skip {
                bail!(
                    span,
                    "conditional or one-directional skipping changes CBOR array \
                     field positions; use an Option placeholder or #[serde(skip)]"
                );
            }
        }
    }
    Ok(())
}

struct ReplaceSelf {
    original: syn::Path,
}
impl VisitMut for ReplaceSelf {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if path.leading_colon.is_none() && path.segments.first().is_some_and(|s| s.ident == "Self")
        {
            let suffix = path.segments.iter().skip(1).cloned().collect::<Vec<_>>();
            *path = self.original.clone();
            if !suffix.is_empty() {
                if let syn::PathArguments::AngleBracketed(args) =
                    &mut path.segments.last_mut().unwrap().arguments
                {
                    args.colon2_token = Some(Default::default());
                }
            }
            path.segments.extend(suffix);
        }
        visit_mut::visit_path_mut(self, path);
    }
    // Serde attributes name functions and paths in strings.
    fn visit_attribute_mut(&mut self, attr: &mut syn::Attribute) {
        edit_serde_metas([attr], |metas| {
            for meta in metas {
                let syn::Meta::NameValue(meta) = meta else {
                    continue;
                };
                let names = [
                    "default",
                    "with",
                    "serialize_with",
                    "deserialize_with",
                    "skip_serializing_if",
                ];
                if !names.iter().any(|name| meta.path.is_ident(name)) {
                    continue;
                }
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit),
                    ..
                }) = &mut meta.value
                {
                    if let Ok(mut path) = lit.parse::<syn::Path>() {
                        self.visit_path_mut(&mut path);
                        *lit = syn::LitStr::new(&quote!(#path).to_string(), lit.span());
                    }
                }
            }
        });
    }
}

// Keeps only the carried-over attributes on the shadow's variants and fields.
struct CopiedAttrs;

impl VisitMut for CopiedAttrs {
    fn visit_variant_mut(&mut self, variant: &mut syn::Variant) {
        variant.attrs = copied_attrs(&variant.attrs);
        visit_mut::visit_variant_mut(self, variant);
    }

    fn visit_field_mut(&mut self, field: &mut syn::Field) {
        field.attrs = copied_attrs(&field.attrs);
    }
}

type Metas = syn::punctuated::Punctuated<syn::Meta, syn::Token![,]>;

// Rewrites the metas of each `#[serde(...)]` attribute that parses as a meta
// list; serde's own derive reports anything else.
fn edit_serde_metas<'a>(
    attrs: impl IntoIterator<Item = &'a mut syn::Attribute>,
    mut edit: impl FnMut(&mut Metas),
) {
    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        if let Ok(mut metas) = attr.parse_args_with(Metas::parse_terminated) {
            edit(&mut metas);
            if let syn::Meta::List(list) = &mut attr.meta {
                list.tokens = quote!(#metas);
            }
        }
    }
}

// Picks an internal lifetime such as the deserializer's `'__de` that cannot
// collide with the user's generics (including a user lifetime of that name).
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
fn marker(tag: Option<u64>, array: bool, entries: &[Entry], name: &str) -> Option<String> {
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
    marker.push_str(name);

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
        let name: syn::Ident = input.parse()?;
        if name != "key" {
            bail!(name.span(), "expected `key = <integer>`");
        }
        input.parse::<syn::Token![=]>()?;

        // CBOR integer keys span major types 0 and 1.
        let (value, span) = int_arg(
            input,
            "key",
            -(u64::MAX as i128) - 1,
            "#[cbor(key = ...)] must fit a CBOR integer (-2^64 ..= 2^64 - 1)",
        )?;
        Ok(KeyArg { value, span })
    }
}

// Parses the unsuffixed integer literal of `#[cbor(<name> = ...)]`, which
// must lie in `min ..= 2^64 - 1`; anything else reports `range`.
fn int_arg(
    input: ParseStream<'_>,
    name: &str,
    min: i128,
    range: &str,
) -> syn::Result<(i128, proc_macro2::Span)> {
    let minus: Option<syn::Token![-]> = input.parse()?;
    let literal: syn::LitInt = input.parse()?;
    let span = literal.span();

    // `base10_parse` ignores a type suffix; a suffixed value would be
    // accepted with the suffix silently meaning nothing.
    if !literal.suffix().is_empty() {
        bail!(
            span,
            "#[cbor({name} = ...)] does not accept a suffixed integer literal"
        );
    }

    // A `LitInt` is already a valid integer, so the only failure left is
    // the range, including overflow beyond i128.
    let value = literal
        .base10_parse::<i128>()
        .ok()
        .and_then(|value| match minus {
            None => Some(value),
            Some(_) if min < 0 => Some(-value),
            Some(_) => None,
        });
    match value {
        Some(value) if (min..=u64::MAX as i128).contains(&value) => Ok((value, span)),
        _ => bail!(span, "{range}"),
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
                let (value, span) = int_arg(
                    meta.value()?,
                    "tag",
                    0,
                    "tag must fit a CBOR tag (0 ..= 2^64 - 1)",
                )?;
                let tag = TagArg {
                    value: value as u64,
                    span,
                };
                if out.tag.replace(tag).is_some() {
                    bail!(meta.path.span(), "duplicate #[cbor(tag = ...)] attribute");
                }
                Ok(())
            } else if meta.path.is_ident("array") {
                if meta.input.peek(syn::Token![=]) {
                    bail!(meta.path.span(), "expected `array` without a value");
                }
                if out.array.replace(meta.path.span()).is_some() {
                    bail!(meta.path.span(), "duplicate #[cbor(array)] attribute");
                }
                Ok(())
            } else {
                bail!(meta.path.span(), "expected `tag = <integer>` or `array`");
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
        Some(e) if e.name == entry.name && e.key == entry.key => {}
        Some(e) if e.name == entry.name => bail!(
            entry.span,
            "field `{}` maps to conflicting keys {} and {}",
            entry.name,
            e.key,
            entry.key
        ),
        Some(e) => bail!(
            entry.span,
            "key {} is already mapped to field `{}`",
            entry.key,
            e.name
        ),
        None => entries.push(entry),
    }
    Ok(())
}

// A marker's key table is shared by every variant. Field names and input
// aliases must not inherit an integer key from a different field.
fn validate_enum_keys(groups: &[FieldGroup<'_>], entries: &[Entry]) -> syn::Result<()> {
    for group in groups {
        for info in &group.fields {
            let Some(ident) = &info.field.ident else {
                continue;
            };
            let keyed = info
                .field
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("cbor"));
            for (side, skipped) in [
                group.attrs.skip_serializing || info.attrs.skip_serializing,
                group.attrs.skip_deserializing || info.attrs.skip_deserializing,
            ]
            .into_iter()
            .enumerate()
            {
                if skipped {
                    continue;
                }
                let name = info.attrs.field_name(ident, side, &group.attrs);
                if let Some(entry) = entries.iter().find(|entry| {
                    (!keyed && entry.name == name)
                        || (side == 1
                            && entry.name != name
                            && info.attrs.aliases.contains(&entry.name))
                }) {
                    bail!(
                        info.field.span(),
                        "field label `{}` must declare #[cbor(key = {})] consistently \
                         across enum variants",
                        entry.name,
                        entry.key
                    );
                }
            }
        }
    }
    Ok(())
}

// Serde's field rename rules (field names start in snake_case).
fn rename_field(name: &str, rule: Option<&str>) -> String {
    match rule {
        Some("UPPERCASE" | "SCREAMING_SNAKE_CASE") => name.to_ascii_uppercase(),
        Some("kebab-case") => name.replace('_', "-"),
        Some("SCREAMING-KEBAB-CASE") => name.to_ascii_uppercase().replace('_', "-"),
        Some("PascalCase" | "camelCase") => {
            let mut out = String::new();
            let mut capitalize = true;
            for ch in name.chars() {
                if ch == '_' {
                    capitalize = true;
                } else if capitalize {
                    out.push(ch.to_ascii_uppercase());
                    capitalize = false;
                } else {
                    out.push(ch);
                }
            }
            if rule == Some("camelCase") {
                if let Some(first) = out.get_mut(..1) {
                    first.make_ascii_lowercase();
                }
            }
            out
        }
        _ => name.to_owned(),
    }
}

impl SerdeAttrs {
    fn field_name(&self, ident: &syn::Ident, side: usize, container: &SerdeAttrs) -> String {
        self.rename
            .as_ref()
            .map(|(name, _)| name.clone())
            .or_else(|| self.split_names[side].clone())
            .unwrap_or_else(|| {
                rename_field(
                    &ident.unraw().to_string(),
                    container.rename_rules[side].as_deref(),
                )
            })
    }
}

// Reads the `#[cbor(key = ...)]` field attributes into key table entries
// under the fields' serde names.
fn field_entries(fields: &[FieldInfo<'_>]) -> syn::Result<Vec<Entry>> {
    let mut entries = Vec::new();

    for info in fields {
        let field = info.field;
        let mut key: Option<KeyArg> = None;
        for attr in &field.attrs {
            if !attr.path().is_ident("cbor") {
                continue;
            }

            let arg: KeyArg = attr.parse_args()?;
            if key.replace(arg).is_some() {
                bail!(attr.span(), "duplicate #[cbor(key = ...)] attribute");
            }
        }
        let serde = &info.attrs;
        if let (Some(..), Some(span)) = (&key, serde.flatten) {
            bail!(
                span,
                "#[serde(flatten)] cannot be combined with #[cbor(key = ...)]"
            );
        }
        // A fully skipped field is never on the wire in either direction,
        // so a key on it is a mistake. (The one-directional
        // `skip_serializing`/`skip_deserializing` variants keep the key
        // meaningful and stay allowed.)
        if let (Some(..), Some(span)) = (&key, serde.skip) {
            bail!(
                span,
                "#[serde(skip)] cannot be combined with #[cbor(key = ...)]; \
                 the field is never on the wire"
            );
        }

        let Some(key) = key else { continue };

        if field.ident.is_none() {
            bail!(key.span, "#[cbor(key = ...)] requires a named field");
        }

        if let Some(span) = serde.split_rename {
            bail!(
                span,
                "split serialize/deserialize renames are not supported with \
                 #[cbor(key = ...)]"
            );
        }

        // The key table is consulted with the field's *serde* name, so an
        // explicit rename carries over.
        let name = match &serde.rename {
            Some((name, _)) => name.clone(),
            None => field
                .ident
                .as_ref()
                .expect("checked above")
                .unraw()
                .to_string(),
        };

        if name.is_empty() || name.contains(['@', ';', '=']) {
            bail!(
                key.span,
                "the serde name of a keyed field may not be empty or contain '@', ';' or '='"
            );
        }

        entries.push(Entry {
            name,
            key: key.value,
            span: key.span,
        });
    }

    Ok(entries)
}

fn fields_have_flatten(fields: &[FieldInfo<'_>]) -> Option<proc_macro2::Span> {
    fields
        .iter()
        .find_map(|field| field.attrs.flatten.filter(|_| field.attrs.skip.is_none()))
}

struct FieldInfo<'a> {
    field: &'a syn::Field,
    attrs: SerdeAttrs,
}

struct FieldGroup<'a> {
    fields: Vec<FieldInfo<'a>>,
    attrs: SerdeAttrs,
}

impl<'a> FieldGroup<'a> {
    fn new(fields: &'a syn::Fields, attrs: SerdeAttrs) -> Self {
        Self {
            fields: fields
                .iter()
                .map(|field| FieldInfo {
                    field,
                    attrs: scan_serde(&field.attrs),
                })
                .collect(),
            attrs,
        }
    }
}

// The serde attribute metas the marker must coordinate with.
#[derive(Default)]
struct SerdeAttrs {
    rename: Option<(String, proc_macro2::Span)>,
    aliases: Vec<String>,
    split_rename: Option<proc_macro2::Span>,
    split_names: [Option<String>; 2],
    rename_all: Option<proc_macro2::Span>,
    rename_rules: [Option<String>; 2],
    rename_all_fields: Option<proc_macro2::Span>,
    enum_repr: Option<proc_macro2::Span>,
    tag: Option<proc_macro2::Span>,
    flatten: Option<proc_macro2::Span>,
    // Container shapes that bypass the container name — and with it the
    // marker carrying the declared tag, array shape and keys.
    transparent: Option<proc_macro2::Span>,
    into: Option<proc_macro2::Span>,
    from: Option<proc_macro2::Span>,
    try_from: Option<proc_macro2::Span>,
    // Container-level `#[serde(bound = ...)]`: replaces the inferred
    // `T: Serialize` / `T: Deserialize<'de>` bounds on the outer impls,
    // exactly as it replaces serde's inferred bounds on the shadow.
    ser_bound: Option<BoundPredicates>,
    de_bound: Option<BoundPredicates>,
    // `#[serde(skip)]`: the field is never on the wire in either direction.
    skip: Option<proc_macro2::Span>,
    positional_skip: Option<proc_macro2::Span>,
    skip_serializing: bool,
    skip_deserializing: bool,
    serialize_with: bool,
    deserialize_with: bool,
    default: bool,
    default_path: bool,
    // None: implicit borrowing only; Some(None): borrow all field lifetimes.
    borrow: Option<Option<Vec<syn::Lifetime>>>,
    explicit_crate: bool,
    deny_unknown_fields: bool,
}

type BoundPredicates = syn::punctuated::Punctuated<syn::WherePredicate, syn::Token![,]>;

// Parses a serde bound string — a possibly empty, comma-separated list of
// where-predicates. `None` on a malformed string: the same string is copied
// to the shadow, where serde's own derive reports the error.
fn parse_bound(lit: &syn::LitStr) -> Option<BoundPredicates> {
    lit.parse_with(syn::punctuated::Punctuated::parse_terminated)
        .ok()
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
            if meta.path.is_ident("alias") {
                let name: syn::LitStr = meta.value()?.parse()?;
                out.aliases.push(name.value());
                return Ok(());
            }
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
                meta.parse_nested_meta(|side| {
                    let name: syn::LitStr = side.value()?.parse()?;
                    if side.path.is_ident("serialize") {
                        out.split_names[0] = Some(name.value());
                    } else if side.path.is_ident("deserialize") {
                        out.split_names[1] = Some(name.value());
                    }
                    Ok(())
                })?;
                return Ok(());
            } else if meta.path.is_ident("rename_all") {
                out.rename_all = Some(meta.path.span());
                if meta.input.peek(syn::Token![=]) {
                    let rule: syn::LitStr = meta.value()?.parse()?;
                    out.rename_rules = [Some(rule.value()), Some(rule.value())];
                } else {
                    meta.parse_nested_meta(|side| {
                        let rule: syn::LitStr = side.value()?.parse()?;
                        if side.path.is_ident("serialize") {
                            out.rename_rules[0] = Some(rule.value());
                        } else if side.path.is_ident("deserialize") {
                            out.rename_rules[1] = Some(rule.value());
                        }
                        Ok(())
                    })?;
                }
                return Ok(());
            } else if meta.path.is_ident("rename_all_fields") {
                out.rename_all_fields = Some(meta.path.span());
            } else if meta.path.is_ident("deny_unknown_fields") {
                out.deny_unknown_fields = true;
            } else if meta.path.is_ident("flatten") {
                out.flatten = Some(meta.path.span());
            } else if meta.path.is_ident("transparent") {
                out.transparent = Some(meta.path.span());
            } else if meta.path.is_ident("into") {
                out.into = Some(meta.path.span());
            } else if meta.path.is_ident("from") {
                out.from = Some(meta.path.span());
            } else if meta.path.is_ident("try_from") {
                out.try_from = Some(meta.path.span());
            } else if meta.path.is_ident("bound") {
                if meta.input.peek(syn::Token![=]) {
                    // `bound = "..."` applies to both directions.
                    let expr: syn::Expr = meta.value()?.parse()?;
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = expr
                    {
                        out.ser_bound = parse_bound(&s);
                        out.de_bound = parse_bound(&s);
                    }
                    return Ok(());
                }

                // `bound(serialize = "...", deserialize = "...")`.
                meta.parse_nested_meta(|side| {
                    let is_ser = side.path.is_ident("serialize");
                    let is_de = side.path.is_ident("deserialize");
                    let expr: syn::Expr = side.value()?.parse()?;
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = expr
                    {
                        if is_ser {
                            out.ser_bound = parse_bound(&s);
                        }
                        if is_de {
                            out.de_bound = parse_bound(&s);
                        }
                    }
                    Ok(())
                })?;
                return Ok(());
            } else if meta.path.is_ident("default") {
                out.default = !meta.input.peek(syn::Token![=]);
                out.default_path = !out.default;
            } else if meta.path.is_ident("borrow") {
                out.borrow = Some(if meta.input.peek(syn::Token![=]) {
                    let lit: syn::LitStr = meta.value()?.parse()?;
                    Some(lit.parse_with(
                        syn::punctuated::Punctuated::<syn::Lifetime, syn::Token![+]>::parse_terminated,
                    )?.into_iter().collect())
                } else {
                    None
                });
                return Ok(());
            } else if meta.path.is_ident("with") {
                out.serialize_with = true;
                out.deserialize_with = true;
            } else if meta.path.is_ident("serialize_with") {
                out.serialize_with = true;
            } else if meta.path.is_ident("deserialize_with") {
                out.deserialize_with = true;
            } else if meta.path.is_ident("crate") {
                out.explicit_crate = true;
            } else if meta.path.is_ident("skip_serializing")
                || meta.path.is_ident("skip_deserializing")
                || meta.path.is_ident("skip_serializing_if")
            {
                out.positional_skip = Some(meta.path.span());
                out.skip_serializing |= meta.path.is_ident("skip_serializing");
                out.skip_deserializing |= meta.path.is_ident("skip_deserializing");
            } else if meta.path.is_ident("skip") {
                out.skip = Some(meta.path.span());
                out.skip_serializing = true;
                out.skip_deserializing = true;
            } else if meta.path.is_ident("tag")
                || meta.path.is_ident("untagged")
                || meta.path.is_ident("content")
            {
                out.enum_repr = Some(meta.path.span());
                if meta.path.is_ident("tag") {
                    out.tag = Some(meta.path.span());
                }
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
mod tests;
