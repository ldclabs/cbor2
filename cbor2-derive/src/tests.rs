use quote::quote;

use super::*;

fn expanded(item: TokenStream) -> String {
    expand(item).unwrap().to_string()
}

fn error(item: TokenStream) -> String {
    expand(item).unwrap_err().to_string()
}

#[test]
fn rejects_cross_variant_key_inheritance_after_renaming() {
    for item in [
        quote! { enum E { A { #[cbor(key = 1)] value: u8 }, B { value: u8 } } },
        quote! { enum E { A { #[cbor(key = 1)] value: u8 }, B { #[serde(rename = "value")] other: u8 } } },
        quote! { enum E { A { #[cbor(key = 1)] #[serde(rename = "VALUE")] value: u8 }, #[serde(rename_all = "UPPERCASE")] B { value: u8 } } },
        quote! { enum E { A { #[cbor(key = 1)] value: u8 }, B { #[serde(rename(deserialize = "value"))] other: u8 } } },
        quote! { enum E { A { #[cbor(key = 1)] value: u8 }, B { #[serde(alias = "value")] other: u8 } } },
        quote! { enum E { A { #[cbor(key = 1)] value: u8 }, B { #[cbor(key = 2)] #[serde(alias = "value")] other: u8 } } },
    ] {
        assert!(error(item).contains("consistently across enum variants"));
    }
    expanded(
        quote! { enum E { A { #[cbor(key = 1)] value: u8 }, B { #[serde(skip_deserializing, alias = "value")] other: u8 } } },
    );
}

#[test]
fn validates_marker_bypassing_attributes() {
    assert!(error(quote! {
        enum E { Unit, #[serde(untagged)] Data { #[cbor(key = 1)] value: u8 } }
    })
    .contains("untagged variants"));
    assert!(error(quote! {
        #[serde(tag = "kind")]
        struct S { #[cbor(key = 1)] value: u8 }
    })
    .contains("on a struct conflicts"));
    assert!(error(quote! {
        #[serde(tag = "kind")]
        #[cbor(tag = 7)]
        struct S { value: u8 }
    })
    .contains("on a struct conflicts"));
    assert!(error(quote! {
        #[serde(tag = "kind")]
        #[cbor(array)]
        struct S { value: u8 }
    })
    .contains("on a struct conflicts"));
}

#[test]
fn skipped_flatten_does_not_need_a_binary_adapter() {
    let out = expanded(quote! {
        struct S { value: u8, #[serde(skip, flatten)] extra: BTreeMap<String, u8> }
    });
    assert!(!out.contains("flatten_serialize"));
    assert!(!out.contains("flatten_deserialize"));
}

#[test]
fn only_borrowed_lifetimes_constrain_deserialization() {
    let out = expanded(quote! {
        struct S<'owned, 'borrowed> {
            owned: Cow<'owned, str>,
            #[serde(borrow = "'borrowed")]
            borrowed: Cow<'borrowed, str>,
        }
    });
    assert!(out.contains("'__de : 'borrowed"), "{out}");
    assert!(!out.contains("'__de : 'owned"), "{out}");
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
        out.contains("impl :: cbor2 :: __serde :: Serialize for ProtectedHeader"),
        "{out}"
    );
    assert!(
        out.contains(
            "impl < '__de > :: cbor2 :: __serde :: Deserialize < '__de > for ProtectedHeader"
        ),
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
        out.contains("impl :: cbor2 :: __serde :: Serialize for Plain"),
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

    assert!(out.contains("flatten_serialize"), "{out}");
    assert!(out.contains("flatten_deserialize"), "{out}");
    assert!(
        out.contains(r#"rename = "@@CBOR@@61@@iss=1@@Claims""#),
        "{out}"
    );
    assert!(
        out.contains(r#"const KEYS : & 'static [(& 'static str , i128)] = & [("iss" , 1i128)] ;"#),
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
            "impl < T : Clone + :: cbor2 :: __serde :: Serialize > :: cbor2 :: __serde :: Serialize for Wrap < T >"
        ),
        "{out}"
    );
    assert!(
        out.contains("impl < '__de , T : Clone + :: cbor2 :: __serde :: Deserialize < '__de > >"),
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
            "impl < '__de_ : 'a + '__de , 'a , '__de > :: cbor2 :: __serde :: Deserialize < '__de_ > for Borrowed < 'a , '__de >"
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

    let msg = error(quote! {
        #[serde(from = "Other")]
        #[cbor(tag = 7)]
        struct S {
            a: u8,
        }
    });
    assert!(msg.contains("silently ignored on decode"), "{msg}");

    let msg = error(quote! {
        #[serde(try_from = "Other")]
        struct S {
            #[cbor(key = 1)]
            a: u8,
        }
    });
    assert!(msg.contains("silently ignored on decode"), "{msg}");

    // Without any #[cbor(...)] details there is nothing to lose, so
    // both shapes stay allowed.
    let out = expanded(quote! {
        #[serde(transparent)]
        struct S {
            a: u8,
        }
    });
    assert!(out.contains("transparent"), "{out}");

    let out = expanded(quote! {
        #[serde(from = "Other")]
        struct S {
            a: u8,
        }
    });
    assert!(out.contains("Other"), "{out}");
}

#[test]
fn container_bounds_replace_the_inferred_impl_bounds() {
    // `bound = ""` erases the `T: Serialize` / `T: Deserialize<'de>`
    // bounds on the outer impls, as it does on the shadow's impls.
    let out = expanded(quote! {
        #[serde(bound = "")]
        struct S<T> {
            #[cbor(key = 1)]
            a: u8,
            #[serde(skip)]
            marker: PhantomData<T>,
        }
    });
    assert!(
        !out.contains("T : :: cbor2 :: __serde :: Serialize"),
        "{out}"
    );
    assert!(
        !out.contains("T : :: cbor2 :: __serde :: Deserialize"),
        "{out}"
    );

    // Split bounds replace each direction separately, and `'de` in a
    // deserialize bound is renamed to the impl's fresh lifetime.
    let out = expanded(quote! {
        #[serde(bound(deserialize = "T: ::serde::Deserialize<'de> + Default"))]
        struct S<T> {
            #[cbor(key = 1)]
            a: T,
        }
    });
    assert!(
        out.contains("T : :: serde :: Deserialize < '__de > + Default"),
        "{out}"
    );
    assert!(
        out.contains("T : :: cbor2 :: __serde :: Serialize"),
        "{out}"
    );
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
