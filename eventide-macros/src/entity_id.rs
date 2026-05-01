use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Item, Result, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
};

use crate::utils::apply_derives;

/// Implementation of the `#[entity_id]` attribute macro.
///
/// Restrictions and behaviour:
/// - Only supports a tuple struct with **exactly one** unnamed field
///   (i.e. `struct Foo(Inner);`). Other shapes are rejected with a
///   compile error.
/// - Merges / appends the standard derive set for an identifier:
///   `Default`, `Clone`, `Copy`, `Debug` (toggleable), `Serialize`,
///   `Deserialize`, `PartialEq`, `Eq`, `Hash`.
/// - Generates ergonomic conversions and helpers around the wrapper:
///   `new(value)`, `Display`, `FromStr`, `AsRef`/`AsMut`, and
///   `From<Inner>`/`From<Self>` (plus borrowed variants).
/// - Accepts the attribute syntax `#[entity_id(debug = true|false)]`,
///   with `debug` defaulting to `true`.
///
/// **Important**: because `Copy` is unconditionally derived, the inner
/// type must itself be `Copy` (e.g. `Uuid`, `u64`). A wrapper around
/// `String` will not compile.
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let cfg = parse_macro_input!(attr as EntityIdAttrConfig);
    let input = parse_macro_input!(item as Item);

    let st = match input {
        Item::Struct(s) => s,
        other => {
            return syn::Error::new(other.span(), "#[entity_id] only on struct")
                .to_compile_error()
                .into();
        }
    };

    let fields = match &st.fields {
        syn::Fields::Unnamed(f) if f.unnamed.len() == 1 => f,
        syn::Fields::Unnamed(f) => {
            return syn::Error::new(
                f.span(),
                "#[entity_id] requires a tuple struct with exactly one field",
            )
            .to_compile_error()
            .into();
        }
        _ => {
            return syn::Error::new(
                st.span(),
                "#[entity_id] supports only tuple struct, e.g., struct X(String);",
            )
            .to_compile_error()
            .into();
        }
    };

    let inner_ty = &fields.unnamed.first().unwrap().ty;

    // Merge the user's existing derives with the standard set expected of
    // an identifier newtype. Working on a clone of the struct keeps the
    // ownership semantics of the surrounding match arms simple — we
    // mutate `st_out.attrs` rather than the original input.
    let mut st_out = st.clone();

    let mut required: Vec<syn::Path> = vec![
        syn::parse_quote!(Default),
        syn::parse_quote!(Clone),
        syn::parse_quote!(Copy),
        syn::parse_quote!(serde::Serialize),
        syn::parse_quote!(serde::Deserialize),
        syn::parse_quote!(PartialEq),
        syn::parse_quote!(Eq),
        syn::parse_quote!(Hash),
    ];

    if cfg.derive_debug.unwrap_or(true) {
        required.insert(0, syn::parse_quote!(Debug));
    }

    apply_derives(&mut st_out.attrs, required);

    let ident = &st_out.ident;
    let generics = st_out.generics.clone();
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let out = quote! {
        #st_out

        impl #impl_generics #ident #ty_generics #where_clause {
            pub fn new(value: #inner_ty) -> Self { Self(value) }
        }

        impl #impl_generics ::std::str::FromStr for #ident #ty_generics #where_clause
        where #inner_ty: ::std::str::FromStr
        {
            type Err = <#inner_ty as ::std::str::FromStr>::Err;
            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                let inner: #inner_ty = s.parse()?;
                ::std::result::Result::Ok(Self(inner))
            }
        }

        impl #impl_generics ::std::fmt::Display for #ident #ty_generics #where_clause
        where #inner_ty: ::std::fmt::Display
        {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::write!(f, "{}", self.0)
            }
        }

        impl #impl_generics ::core::convert::AsRef<#inner_ty> for #ident #ty_generics #where_clause {
            fn as_ref(&self) -> &#inner_ty { &self.0 }
        }

        impl #impl_generics ::core::convert::AsMut<#inner_ty> for #ident #ty_generics #where_clause {
            fn as_mut(&mut self) -> &mut #inner_ty { &mut self.0 }
        }

        impl #impl_generics ::core::convert::From<#ident #ty_generics> for #inner_ty #where_clause {
            fn from(value: #ident #ty_generics) -> Self { value.0 }
        }

        impl #impl_generics ::core::convert::From<&#ident #ty_generics> for #inner_ty #where_clause
        where #inner_ty: ::core::clone::Clone
        {
            fn from(value: &#ident #ty_generics) -> Self { value.0.clone() }
        }

        impl #impl_generics ::core::convert::From<#inner_ty> for #ident #ty_generics #where_clause {
            fn from(value: #inner_ty) -> Self { Self(value) }
        }

        impl #impl_generics ::core::convert::From<&#inner_ty> for #ident #ty_generics #where_clause
        where #inner_ty: ::core::clone::Clone
        {
            fn from(value: &#inner_ty) -> Self { Self(value.clone()) }
        }
    };

    TokenStream::from(out)
}

// -------- parsing --------
//
// `#[entity_id(...)]` currently has a single recognised key (`debug`),
// but the parser is structured the same way as the other macros so
// adding more keys later does not change the call site.

struct EntityIdAttrConfig {
    derive_debug: Option<bool>,
}

impl Parse for EntityIdAttrConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.is_empty() {
            return Ok(Self { derive_debug: None });
        }

        let mut derive_debug: Option<bool> = None;
        let pairs: syn::punctuated::Punctuated<EntityIdAttrElem, Token![,]> =
            syn::punctuated::Punctuated::parse_terminated(input)?;
        for elem in pairs {
            match elem {
                EntityIdAttrElem::Debug(b) => {
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
        Ok(Self { derive_debug })
    }
}

enum EntityIdAttrElem {
    Debug(bool),
}

impl Parse for EntityIdAttrElem {
    fn parse(input: ParseStream) -> Result<Self> {
        let key: syn::Ident = input.parse()?;
        if key == "debug" {
            let _eq: Token![=] = input.parse()?;
            let expr: syn::Expr = input.parse()?;
            match expr {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Bool(b),
                    ..
                }) => Ok(Self::Debug(b.value())),
                other => Err(syn::Error::new(
                    other.span(),
                    "expected boolean literal for 'debug'",
                )),
            }
        } else {
            Err(syn::Error::new(
                key.span(),
                "unknown key in attribute; expected 'debug'",
            ))
        }
    }
}
