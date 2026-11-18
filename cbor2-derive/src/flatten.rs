//! Binary serialization and deserialization for structs with extension fields.
use super::*;

#[derive(Default)]
pub(super) struct Expansion {
    pub helpers: TokenStream,
    pub serialize: Option<TokenStream>,
    pub deserialize: Option<TokenStream>,
}

pub(super) fn fields(
    shadow: &syn::DeriveInput,
    fields: &[FieldInfo<'_>],
    serde: &SerdeAttrs,
    runtime: &syn::Ident,
    type_name: &str,
    entries: &[Entry],
) -> Expansion {
    let declared_names: Vec<String> = fields
        .iter()
        .filter(|f| f.attrs.flatten.is_none() && f.attrs.skip.is_none())
        .flat_map(|f| {
            let ident = f.field.ident.as_ref().unwrap();
            let mut names = f.attrs.aliases.clone();
            for side in 0..2 {
                let name = f.attrs.field_name(ident, side, serde);
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            names
        })
        .collect();

    // Container attributes can delegate directly to another type. Apply the
    // field adapter only in directions that use this struct's fields.
    let transparent = serde.transparent.is_some();
    let mut expansion = Expansion::default();
    if !transparent && serde.into.is_none() {
        let (helpers, serialize) = serialize_fields(
            shadow,
            fields,
            runtime,
            type_name,
            entries,
            &declared_names,
            serde.tag.is_some(),
        );
        expansion.helpers.extend(helpers);
        expansion.serialize = Some(serialize);
    }
    if !transparent && serde.from.is_none() && serde.try_from.is_none() {
        let (helpers, deserialize) = deserialize_fields(
            shadow,
            fields,
            runtime,
            type_name,
            &declared_names,
            serde.deny_unknown_fields,
        );
        expansion.helpers.extend(helpers);
        expansion.deserialize = Some(deserialize);
    }
    expansion
}

fn deserialize_fields(
    shadow: &syn::DeriveInput,
    fields: &[FieldInfo<'_>],
    runtime: &syn::Ident,
    type_name: &str,
    declared_names: &[String],
    deny_unknown: bool,
) -> (TokenStream, TokenStream) {
    let mut fields_shadow = shadow.clone();
    let fields_ident = format_ident!("__CborFields");
    fields_shadow.ident = fields_ident.clone();
    let syn::Data::Struct(shadow_fields) = &mut fields_shadow.data else {
        unreachable!()
    };
    fields_shadow.attrs.insert(
        0,
        syn::parse_quote!(#[derive(::#runtime::__serde::Deserialize)]),
    );
    fields_shadow
        .attrs
        .push(syn::parse_quote!(#[serde(rename = #type_name)]));
    let mut extension_names = Vec::new();
    for (field, info) in shadow_fields.fields.iter_mut().zip(fields) {
        if info.attrs.flatten.is_some() && info.attrs.skip.is_none() {
            // On input the runtime groups unmatched entries under the
            // flattened field. Use a private name independent of rename_all.
            let mut name = format!("\0cbor2:{}", field.ident.as_ref().unwrap());
            while declared_names.contains(&name) || extension_names.contains(&name) {
                name.push('_');
            }
            edit_serde_metas(&mut field.attrs, |metas| {
                *metas = core::mem::take(metas)
                    .into_iter()
                    .filter(|m| {
                        !["flatten", "rename", "alias"]
                            .iter()
                            .any(|a| m.path().is_ident(a))
                    })
                    .collect();
            });
            field
                .attrs
                .push(syn::parse_quote!(#[serde(rename = #name)]));
            if !info.attrs.skip_deserializing {
                extension_names.push(name);
            }
        }
    }
    (
        quote!(#fields_shadow),
        quote! {
            #fields_ident::deserialize(::#runtime::__private::flatten_deserializer(
                deserializer, <Self as ::#runtime::Cbor>::KEYS, &[#(#extension_names),*], #deny_unknown,
            ))
        },
    )
}

fn serialize_fields(
    shadow: &syn::DeriveInput,
    fields: &[FieldInfo<'_>],
    runtime: &syn::Ident,
    type_name: &str,
    entries: &[Entry],
    declared_names: &[String],
    has_serde_tag: bool,
) -> (TokenStream, TokenStream) {
    // Each run retains serde's field order, skip predicates and custom
    // serializers. Boundaries identify extension keys without reserving
    // any representation in the user's data.

    let wire_name = marker(None, false, entries, type_name).unwrap_or_else(|| type_name.to_owned());
    let runs = fields.chunk_by(|a, b| a.attrs.flatten.is_some() == b.attrs.flatten.is_some());
    let mut offset = 0;
    let mut group_shadows = Vec::new();
    let mut encode_groups = Vec::new();
    for run in runs {
        let start = offset;
        offset += run.len();
        let skipped = run.iter().all(|f| f.attrs.skip_serializing);
        // Keep one group for the discriminator if every field is skipped.
        let tag_only = has_serde_tag && group_shadows.is_empty() && offset == fields.len();
        if skipped && !tag_only {
            continue;
        }
        let is_extra = !skipped && run[0].attrs.flatten.is_some();
        let mut group = shadow.clone();
        let group_ident = format_ident!("__CborGroup{}", start);
        group.ident = group_ident.clone();
        group.attrs.insert(
            0,
            syn::parse_quote!(#[derive(::#runtime::__serde::Serialize)]),
        );
        // A serde struct discriminator is written once, with the original
        // type name, by the first run.
        group
            .attrs
            .push(syn::parse_quote!(#[serde(rename = #wire_name)]));
        if !group_shadows.is_empty() {
            edit_serde_metas(&mut group.attrs, |metas| {
                *metas = core::mem::take(metas)
                    .into_iter()
                    .filter(|m| !m.path().is_ident("tag"))
                    .collect()
            });
        }
        let syn::Data::Struct(data) = &mut group.data else {
            unreachable!()
        };
        for (i, field) in data.fields.iter_mut().enumerate() {
            if !(start..offset).contains(&i) {
                field.attrs.retain(|attr| !attr.path().is_ident("serde"));
                field.attrs.push(syn::parse_quote!(#[serde(skip)]));
            }
        }
        group_shadows.push(group);
        encode_groups.push(quote! {{
            #group_ident::serialize(self, &mut ::#runtime::ser::Serializer::from(&mut *bytes))?;
            (#is_extra, bytes.len())
        }});
    }
    (
        quote!(#(#group_shadows)*),
        quote! {
            ::#runtime::__private::flatten_serialize_fields(
                serializer, <Self as ::#runtime::Cbor>::TAG, <Self as ::#runtime::Cbor>::KEYS,
                &[#(#declared_names),*],
                |bytes| ::core::result::Result::Ok([#(#encode_groups),*]),
            )
        },
    )
}

// A pinned, older cbor2 runtime can still resolve a newer 1.x derive. It
// enables only the original protocol and has no grouped-field entry points.
pub(super) fn legacy(
    input: &syn::DeriveInput,
    runtime: &syn::Ident,
    ser_generics: &syn::Generics,
    de_generics: &syn::Generics,
    de_lifetime: &syn::Lifetime,
    shadow_ident: &syn::Ident,
) -> Expansion {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let (de_impl_generics, _, de_where_clause) = de_generics.split_for_impl();
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

    let helpers = quote! {
        struct __CborShadowRef #shadow_ref_impl_generics #shadow_ref_where_clause {
            value: &#cbor_lifetime #ident #ty_generics,
        }

        impl #ser_ref_impl_generics ::#runtime::__serde::Serialize for __CborShadowRef #ser_ref_ty_generics #ser_ref_where_clause {
            fn serialize<__S>(&self, serializer: __S) -> ::core::result::Result<__S::Ok, __S::Error>
            where
                __S: ::#runtime::__serde::Serializer,
            {
                #shadow_ident::serialize(self.value, serializer)
            }
        }

        struct __CborShadowOwned #impl_generics (#ident #ty_generics) #where_clause;

        impl #de_impl_generics ::#runtime::__serde::Deserialize<#de_lifetime> for __CborShadowOwned #ty_generics #de_where_clause {
            fn deserialize<__D>(deserializer: __D) -> ::core::result::Result<Self, __D::Error>
            where
                __D: ::#runtime::__serde::Deserializer<#de_lifetime>,
            {
                #shadow_ident::deserialize(deserializer).map(Self)
            }
        }

    };
    Expansion {
        helpers,
        serialize: Some(
            quote!(::#runtime::__private::flatten_serialize(&__CborShadowRef { value:self }, serializer, <Self as ::#runtime::Cbor>::TAG, <Self as ::#runtime::Cbor>::KEYS)),
        ),
        deserialize: Some(quote! {
            let __value:__CborShadowOwned #ty_generics = ::#runtime::__private::flatten_deserialize(deserializer, <Self as ::#runtime::Cbor>::KEYS)?;
            ::core::result::Result::Ok(__value.0)
        }),
    }
}
