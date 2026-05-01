use crate::utils::{apply_derives, ensure_required_fields};
use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Item, ItemStruct, Result, Token, Type, parse::Parse, parse::ParseStream, parse_macro_input,
};

/// Implementation of the `#[entity]` attribute macro.
///
/// Behaviour at a glance:
/// - Inserts the fields `id: IdType` and `version: Version` if they are
///   missing, and repositions them to the top of the struct so every
///   entity has a consistent layout.
/// - Generates an `impl ::eventide_domain::entity::Entity` block exposing
///   `new`, `id` and `version`.
/// - Accepts the attribute syntax `#[entity(id = IdType, debug = true|false)]`:
///   - `id` defaults to `String` when omitted.
///   - `debug` defaults to `true` (auto-derive `Debug`). Setting it to
///     `false` skips the `Debug` derive so the user can provide a custom
///     implementation (useful for redacting sensitive fields).
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let cfg = parse_macro_input!(attr as EntityAttrConfig);
    let input = parse_macro_input!(item as Item);

    let mut st = match input {
        Item::Struct(s) => s,
        other => {
            return syn::Error::new(other.span(), "#[entity] only on struct")
                .to_compile_error()
                .into();
        }
    };

    // Only named-field structs are supported; tuple structs and unit
    // structs do not match the DDD entity shape this macro generates code
    // for (we need to reference fields by name in the generated `Entity`
    // impl).
    let fields_named = match &mut st.fields {
        syn::Fields::Named(f) => f,
        _ => {
            return syn::Error::new(st.span(), "only supports named-field struct")
                .to_compile_error()
                .into();
        }
    };

    let id_type = cfg.id_ty.unwrap_or_else(|| syn::parse_quote! { String });

    // Reorganise the fields so that `id` and `version` are guaranteed to
    // exist and appear at the very top of the struct (in that order). The
    // `true` flag asks `ensure_required_fields` to *reposition* any
    // existing `id`/`version` fields rather than just appending missing
    // ones, which keeps the field layout uniform across all entities.
    let version_ty: Type = syn::parse_quote! { ::eventide_domain::value_object::Version };

    ensure_required_fields(
        fields_named,
        &[("id", &id_type), ("version", &version_ty)],
        /*reposition_existing*/ true,
    );

    // Merge / normalise the `derive` set. The user's existing derives are
    // preserved (de-duplicated) and we always add `Default`, `Serialize`
    // and `Deserialize`. `Debug` is added by default but can be opted out
    // of via `#[entity(debug = false)]` so the user can hand-write a
    // redacting `Debug` impl.
    let mut required: Vec<syn::Path> = vec![
        syn::parse_quote!(Default),
        syn::parse_quote!(serde::Serialize),
        syn::parse_quote!(serde::Deserialize),
    ];

    if cfg.derive_debug.unwrap_or(true) {
        required.insert(0, syn::parse_quote!(Debug));
    }

    apply_derives(&mut st.attrs, required);

    let out_struct = ItemStruct { ..st };

    // Emit the `Entity` impl for the (possibly modified) struct. We thread
    // the original generics through so that generic entities are supported
    // — `split_for_impl` gives us the `<T: ...>`, `<T>` and `where ...`
    // fragments needed for a syntactically correct impl block.
    let ident = &out_struct.ident;
    let generics = out_struct.generics.clone();
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let expanded = quote! {
        #out_struct

        impl #impl_generics ::eventide_domain::entity::Entity for #ident #ty_generics #where_clause {
            type Id = #id_type;

            fn new(aggregate_id: Self::Id, version: ::eventide_domain::value_object::Version) -> Self {
                Self { id: aggregate_id, version, ..Default::default() }
            }

            fn id(&self) -> &Self::Id { &self.id }

            fn version(&self) -> ::eventide_domain::value_object::Version { self.version }
        }
    };

    TokenStream::from(expanded)
}

// -------- parsing --------
//
// Helpers below parse the comma-separated key/value list that appears
// inside `#[entity(...)]`. They return a strongly-typed config struct
// that the expander above consumes, keeping parsing concerns isolated
// from token-tree generation.

struct EntityAttrConfig {
    id_ty: Option<Type>,
    derive_debug: Option<bool>,
}

impl Parse for EntityAttrConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut id_ty: Option<Type> = None;
        let mut derive_debug: Option<bool> = None;

        if input.is_empty() {
            return Ok(Self {
                id_ty,
                derive_debug,
            });
        }

        let elems: Punctuated<EntityAttrElem, Token![,]> =
            Punctuated::<EntityAttrElem, Token![,]>::parse_terminated(input)?;

        for elem in elems.into_iter() {
            match elem {
                EntityAttrElem::Id(ty) => {
                    if id_ty.is_some() {
                        return Err(syn::Error::new(
                            ty.span(),
                            "duplicate key 'id' in attribute",
                        ));
                    }
                    id_ty = Some(*ty);
                }
                EntityAttrElem::Debug(b) => {
                    if derive_debug.is_some() {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            "duplicate key 'debug' in attribute",
                        ));
                    }
                    derive_debug = Some(b);
                }
            }
        }

        Ok(Self {
            id_ty,
            derive_debug,
        })
    }
}

enum EntityAttrElem {
    Id(Box<Type>),
    Debug(bool),
}

impl Parse for EntityAttrElem {
    fn parse(input: ParseStream) -> Result<Self> {
        let key: syn::Ident = input.parse()?;

        if key == "id" {
            let _eq: Token![=] = input.parse()?;
            let ty: Type = input.parse()?;
            Ok(EntityAttrElem::Id(Box::new(ty)))
        } else if key == "debug" {
            let _eq: Token![=] = input.parse()?;
            let expr: syn::Expr = input.parse()?;
            match expr {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Bool(b),
                    ..
                }) => Ok(EntityAttrElem::Debug(b.value())),
                other => Err(syn::Error::new(
                    other.span(),
                    "expected boolean literal for 'debug'",
                )),
            }
        } else {
            Err(syn::Error::new(
                key.span(),
                "unknown key in attribute; expected 'id' or 'debug'",
            ))
        }
    }
}
