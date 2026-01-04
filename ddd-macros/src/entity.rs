use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Item, ItemStruct, Result, Token, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
};

use crate::utils::{apply_derives, ensure_required_fields};

/// #[entity] 宏实现
/// - 若缺失则追加字段：`id: IdType`, `version: Version`，并置于字段最前
/// - 自动实现 `::ddd_domain::entity::Entity`（new/id/version）
/// - 支持参数：`#[entity(id = IdType, debug = true|false)]`；
///   - `id` 默认 `String`
///   - `debug` 默认 `true`（派生 Debug）。当为 `false` 时不派生 Debug，便于用户自定义实现。
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

    // 仅支持具名字段结构体
    let fields_named = match &mut st.fields {
        syn::Fields::Named(f) => f,
        _ => {
            return syn::Error::new(st.span(), "only supports named-field struct")
                .to_compile_error()
                .into();
        }
    };

    let id_type = cfg.id_ty.unwrap_or_else(|| syn::parse_quote! { String });

    // 重新组织字段：确保 id/version 在最前，并避免重复
    let version_ty: Type = syn::parse_quote! { ::ddd_domain::value_object::Version };

    ensure_required_fields(
        fields_named,
        &[("id", &id_type), ("version", &version_ty)],
        // reposition_existing
        true,
    );

    // 合并/规范 derive：默认添加 Debug（可通过 debug=false 关闭）、Default、Serialize、Deserialize
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

    // 生成 Entity 实现
    let ident = &out_struct.ident;
    let generics = out_struct.generics.clone();
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let expanded = quote! {
        #out_struct

        impl #impl_generics ::ddd_domain::entity::Entity for #ident #ty_generics #where_clause {
            type Id = #id_type;

            fn new(aggregate_id: Self::Id, version: ::ddd_domain::value_object::Version) -> Self {
                Self { id: aggregate_id, version, ..Default::default() }
            }

            fn id(&self) -> &Self::Id { &self.id }

            fn version(&self) -> ::ddd_domain::value_object::Version { self.version }
        }
    };

    TokenStream::from(expanded)
}

// -------- parsing --------

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
