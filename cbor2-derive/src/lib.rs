//! The attribute macro behind `cbor2::int_keys`.
//!
//! See the documentation in the `cbor2` crate; this crate is an
//! implementation detail and is not meant to be used directly.

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned as _;

// The marker prefix recognized by the `cbor2` serializers. Keep in sync
// with `cbor2::ser::KEY_MARKER`; the integration tests of the `cbor2` crate
// pin the resulting wire bytes.
const MARKER: &str = "@@KEY@@";

/// Maps struct fields to integer CBOR map keys (see `cbor2::int_keys`).
#[proc_macro_attribute]
pub fn int_keys(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    expand(attr.into(), item.into())
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !attr.is_empty() {
        return Err(syn::Error::new(attr.span(), "int_keys takes no arguments"));
    }

    let mut item: syn::Item = syn::parse2(item)?;
    match &mut item {
        syn::Item::Struct(item) => rewrite_fields(&mut item.fields)?,
        syn::Item::Enum(item) => {
            for variant in &mut item.variants {
                rewrite_fields(&mut variant.fields)?;
            }
        }
        other => {
            return Err(syn::Error::new(
                other.span(),
                "int_keys supports structs and enums",
            ));
        }
    }

    Ok(item.into_token_stream())
}

// `key = <integer>` inside `#[cbor(...)]`.
struct KeyArg {
    value: i128,
    span: proc_macro2::Span,
}

impl Parse for KeyArg {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
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

        let magnitude: i128 = literal.base10_parse()?;
        let value = if negative { -magnitude } else { magnitude };

        Ok(KeyArg {
            value,
            span: literal.span(),
        })
    }
}

fn rewrite_fields(fields: &mut syn::Fields) -> syn::Result<()> {
    for field in fields.iter_mut() {
        let mut key: Option<KeyArg> = None;
        let mut kept = Vec::with_capacity(field.attrs.len());

        for attr in field.attrs.drain(..) {
            if !attr.path().is_ident("cbor") {
                kept.push(attr);
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

        field.attrs = kept;

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

        for attr in &field.attrs {
            if attr.path().is_ident("serde") {
                let mut renamed = false;
                // Ignore serde attribute shapes we do not understand; the
                // serde derive validates them later anyway.
                let _ = attr.parse_nested_meta(|meta| {
                    renamed |= meta.path.is_ident("rename");
                    if !meta.input.is_empty() && !meta.input.peek(syn::Token![,]) {
                        let _: syn::Expr = meta.value()?.parse()?;
                    }
                    Ok(())
                });
                if renamed {
                    return Err(syn::Error::new(
                        key.span,
                        "#[cbor(key = ...)] conflicts with #[serde(rename = ...)]",
                    ));
                }
            }
        }

        let name = format!("{MARKER}{}", key.value);
        field
            .attrs
            .push(syn::parse_quote!(#[serde(rename = #name)]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    fn expanded(item: TokenStream) -> String {
        expand(TokenStream::new(), item).unwrap().to_string()
    }

    fn error(item: TokenStream) -> String {
        expand(TokenStream::new(), item).unwrap_err().to_string()
    }

    #[test]
    fn rewrites_keys_into_marker_renames() {
        let out = expanded(quote! {
            struct CoseKey {
                #[cbor(key = 1)]
                kty: u8,
                #[cbor(key = -2)]
                #[serde(alias = "x")]
                x: Vec<u8>,
                plain: bool,
            }
        });

        assert!(out.contains(r#"rename = "@@KEY@@1""#), "{out}");
        assert!(out.contains(r#"rename = "@@KEY@@-2""#), "{out}");
        assert!(out.contains(r#"alias = "x""#), "{out}");
        assert!(!out.contains("cbor"), "{out}");
    }

    #[test]
    fn coexists_with_flag_attributes() {
        // serde metas without values (`default`) and non-serde attributes
        // pass through untouched.
        let out = expanded(quote! {
            struct S {
                #[cbor(key = 1)]
                #[serde(default, alias = "k")]
                #[allow(unused)]
                a: u8,
            }
        });

        assert!(out.contains(r#"rename = "@@KEY@@1""#), "{out}");
        assert!(out.contains("default"), "{out}");
        assert!(out.contains("allow"), "{out}");
    }

    #[test]
    fn rewrites_enum_variant_fields() {
        let out = expanded(quote! {
            enum Message {
                Signed {
                    #[cbor(key = 1)]
                    payload: u8,
                },
                Unit,
            }
        });

        assert!(out.contains(r#"rename = "@@KEY@@1""#), "{out}");
    }

    #[test]
    fn accepts_the_full_cbor_integer_range() {
        let out = expanded(quote! {
            struct Edges {
                #[cbor(key = 18446744073709551615)]
                hi: u8,
                #[cbor(key = -18446744073709551616)]
                lo: u8,
                #[cbor(key = 0)]
                zero: u8,
            }
        });

        assert!(
            out.contains(r#"rename = "@@KEY@@18446744073709551615""#),
            "{out}"
        );
        assert!(
            out.contains(r#"rename = "@@KEY@@-18446744073709551616""#),
            "{out}"
        );
        assert!(out.contains(r#"rename = "@@KEY@@0""#), "{out}");
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
            struct S {
                #[cbor(key = -18446744073709551617)]
                a: u8,
            }
        });
        assert!(msg.contains("must fit a CBOR integer"), "{msg}");

        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                #[cbor(key = 2)]
                a: u8,
            }
        });
        assert!(msg.contains("duplicate"), "{msg}");

        let msg = error(quote! {
            struct S {
                #[cbor(key = 1)]
                #[serde(rename = "one")]
                a: u8,
            }
        });
        assert!(msg.contains("conflicts with"), "{msg}");

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
                #[cbor(key = "1")]
                a: u8,
            }
        });
        assert!(msg.contains("expected integer literal"), "{msg}");

        let msg = error(quote! {
            fn not_a_struct() {}
        });
        assert!(msg.contains("supports structs and enums"), "{msg}");
    }

    #[test]
    fn rejects_macro_arguments() {
        let msg = expand(
            quote!(unexpected),
            quote!(
                struct S;
            ),
        )
        .unwrap_err()
        .to_string();
        assert!(msg.contains("takes no arguments"), "{msg}");
    }
}
