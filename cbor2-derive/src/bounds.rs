//! Bounds on the delegating impls must match serde's remote functions.

use syn::visit_mut::{self, VisitMut};

use super::{BoundPredicates, FieldGroup, FieldInfo, SerdeAttrs};

pub(super) fn build(
    input: &syn::DeriveInput,
    attrs: &SerdeAttrs,
    groups: &[FieldGroup<'_>],
    runtime: &syn::Ident,
) -> (syn::Generics, syn::Generics, syn::Lifetime) {
    let lifetime = super::fresh_lifetime(&input.generics, "__de");
    let mut ser = input.generics.clone();
    let mut de = input.generics.clone();

    for group in groups {
        append(&mut ser, group.attrs.ser_bound.as_ref(), None);
        append(&mut de, group.attrs.de_bound.as_ref(), Some(&lifetime));
        for field in &group.fields {
            append(&mut ser, field.attrs.ser_bound.as_ref(), None);
            append(&mut de, field.attrs.de_bound.as_ref(), Some(&lifetime));
        }
    }
    match &attrs.ser_bound {
        Some(bound) => append(&mut ser, Some(bound), None),
        None => infer(
            &mut ser,
            groups,
            |field, variant| {
                !field.attrs.skip_serializing
                    && !field.attrs.serialize_with
                    && field.attrs.ser_bound.is_none()
                    && !variant.skip_serializing
                    && !variant.serialize_with
                    && variant.ser_bound.is_none()
            },
            syn::parse_quote!(::#runtime::__serde::Serialize),
        ),
    }
    match &attrs.de_bound {
        Some(bound) => append(&mut de, Some(bound), Some(&lifetime)),
        None => {
            infer(
                &mut de,
                groups,
                |field, variant| {
                    !field.attrs.skip_deserializing
                        && !field.attrs.deserialize_with
                        && field.attrs.de_bound.is_none()
                        && !variant.skip_deserializing
                        && !variant.deserialize_with
                        && variant.de_bound.is_none()
                },
                syn::parse_quote!(::#runtime::__serde::Deserialize<#lifetime>),
            );
            infer(
                &mut de,
                groups,
                |field, _| {
                    field.attrs.default
                        || (field.attrs.skip_deserializing
                            && !field.attrs.default_path
                            && !attrs.default
                            && !attrs.default_path)
                },
                syn::parse_quote!(::core::default::Default),
            );
        }
    }
    if attrs.default {
        let ident = &input.ident;
        let (_, args, _) = input.generics.split_for_impl();
        de.make_where_clause()
            .predicates
            .push(syn::parse_quote!(#ident #args: ::core::default::Default));
    }

    let mut borrowed = Lifetimes::default();
    for group in groups {
        for field in &group.fields {
            if field.attrs.skip_deserializing {
                continue;
            }
            let borrow = field.attrs.borrow.as_ref().or(group.attrs.borrow.as_ref());
            match borrow {
                Some(Some(lifetimes)) => {
                    for lifetime in lifetimes {
                        borrowed.visit_lifetime_mut(&mut lifetime.clone());
                    }
                }
                Some(None) => borrowed.visit_type_mut(&mut field.field.ty.clone()),
                None if implicitly_borrowed(&field.field.ty) => {
                    borrowed.visit_type_mut(&mut field.field.ty.clone());
                }
                None => {}
            }
        }
    }
    let mut param = syn::LifetimeParam::new(lifetime.clone());
    param.bounds.extend(borrowed.0);
    de.params.insert(0, syn::GenericParam::Lifetime(param));
    (ser, de, lifetime)
}

fn append(
    generics: &mut syn::Generics,
    predicates: Option<&BoundPredicates>,
    to: Option<&syn::Lifetime>,
) {
    if let Some(predicates) = predicates {
        let mut predicates = predicates.clone();
        if let Some(to) = to {
            rename(&mut predicates, &syn::parse_quote!('de), to);
        }
        generics.make_where_clause().predicates.extend(predicates);
    }
}

// Infer bounds on participating type parameters, not whole field types:
// bounding Option<Box<Self>> would create a recursive trait obligation.
fn infer(
    generics: &mut syn::Generics,
    groups: &[FieldGroup<'_>],
    include: impl Fn(&FieldInfo<'_>, &SerdeAttrs) -> bool,
    bound: syn::TypeParamBound,
) {
    let mut used = TypeParams {
        declared: generics
            .type_params()
            .map(|param| param.ident.clone())
            .collect(),
        used: Vec::new(),
        associated: Vec::new(),
    };
    for group in groups {
        for field in &group.fields {
            if include(field, &group.attrs) {
                // Serde adds an associated-type bound for fields like T::Value.
                if let syn::Type::Path(ty) = ungroup(&field.field.ty) {
                    if ty.path.segments.len() > 1
                        && used.declared.contains(&ty.path.segments[0].ident)
                    {
                        used.associated.push(ty.clone());
                    }
                }
                used.visit_type_mut(&mut field.field.ty.clone());
            }
        }
    }
    for param in generics.type_params_mut() {
        if used.used.contains(&param.ident) {
            param.bounds.push(bound.clone());
        }
    }
    for ty in used.associated {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#ty: #bound));
    }
}

struct TypeParams {
    declared: Vec<syn::Ident>,
    used: Vec<syn::Ident>,
    associated: Vec<syn::TypePath>,
}

impl VisitMut for TypeParams {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        if path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "PhantomData")
        {
            return;
        }
        if path.leading_colon.is_none() && path.segments.len() == 1 {
            let ident = &path.segments[0].ident;
            if self.declared.contains(ident) && !self.used.contains(ident) {
                self.used.push(ident.clone());
            }
        }
        visit_mut::visit_path_mut(self, path);
    }
}

#[derive(Default)]
struct Lifetimes(Vec<syn::Lifetime>);

impl VisitMut for Lifetimes {
    fn visit_lifetime_mut(&mut self, lifetime: &mut syn::Lifetime) {
        if lifetime.ident != "_" && !self.0.iter().any(|lt| lt.ident == lifetime.ident) {
            self.0.push(lifetime.clone());
        }
    }
}

fn ungroup(mut ty: &syn::Type) -> &syn::Type {
    while let syn::Type::Group(group) = ty {
        ty = &group.elem;
    }
    ty
}

fn implicitly_borrowed(ty: &syn::Type) -> bool {
    if borrowed_reference(ty) {
        return true;
    }
    let syn::Type::Path(ty) = ungroup(ty) else {
        return false;
    };
    let Some(segment) = ty.path.segments.last() else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    segment.ident == "Option"
        && args.args.len() == 1
        && matches!(&args.args[0], syn::GenericArgument::Type(ty) if borrowed_reference(ty))
}

fn borrowed_reference(ty: &syn::Type) -> bool {
    let syn::Type::Reference(reference) = ungroup(ty) else {
        return false;
    };
    match ungroup(&reference.elem) {
        syn::Type::Path(ty) => ty.path.is_ident("str"),
        syn::Type::Slice(slice) => {
            matches!(ungroup(&slice.elem), syn::Type::Path(ty) if ty.path.is_ident("u8"))
        }
        _ => false,
    }
}

pub(super) fn rename(predicates: &mut BoundPredicates, from: &syn::Lifetime, to: &syn::Lifetime) {
    struct Rename<'a> {
        from: &'a syn::Lifetime,
        to: &'a syn::Lifetime,
    }
    impl VisitMut for Rename<'_> {
        fn visit_lifetime_mut(&mut self, lifetime: &mut syn::Lifetime) {
            if lifetime.ident == self.from.ident {
                *lifetime = self.to.clone();
            }
        }
    }
    let mut visitor = Rename { from, to };
    for predicate in predicates {
        visitor.visit_where_predicate_mut(predicate);
    }
}
