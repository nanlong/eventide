use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    Expr, Ident, Item, Result, Token, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
};

use crate::utils::{apply_derives, ensure_required_fields};

/// #[domain_event] 宏实现
/// - 支持三种变体类型：命名字段 `Variant { .. }`、单元 `Variant`、元组 `Variant(T)`
/// - 单元变体和元组变体自动转换为命名字段变体
/// - 确保每个变体具备字段：`id: IdType`, `aggregate_version: Version`
/// - 生成 `::ddd_domain::domain_event::DomainEvent` 实现（event_id/type/version/aggregate_version）
/// - 支持：`#[event(id = IdType, version = N)]`（枚举级默认值）
/// - 变体可覆写：`#[event(event_type = "...", event_version = N)]`
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let cfg = parse_macro_input!(attr as EventAttrConfig);
    let mut input = parse_macro_input!(item as Item);

    let enum_item = match &mut input {
        Item::Enum(e) => e,
        other => {
            return syn::Error::new(
                other.span(),
                "#[domain_event] can only be used on enum types",
            )
            .to_compile_error()
            .into();
        }
    };

    let id_type = cfg.id_ty.unwrap_or_else(|| syn::parse_quote! { String });
    let version_lit = cfg.version.unwrap_or_else(|| syn::parse_quote! { 1 });

    // 合并/追加默认派生：Debug, Clone, PartialEq, Serialize, Deserialize
    let required: Vec<syn::Path> = vec![
        syn::parse_quote!(Debug),
        syn::parse_quote!(Clone),
        syn::parse_quote!(PartialEq),
        syn::parse_quote!(serde::Serialize),
        syn::parse_quote!(serde::Deserialize),
    ];
    apply_derives(&mut enum_item.attrs, required);

    let mut variant_types: HashMap<String, syn::LitStr> = HashMap::new();
    let mut variant_versions: HashMap<String, syn::LitInt> = HashMap::new();

    for v in &mut enum_item.variants {
        // 先处理变体属性（所有变体类型通用）
        let mut retained_attrs = Vec::new();
        let mut type_lit: Option<syn::LitStr> = None;
        let mut version_lit_local: Option<syn::LitInt> = None;

        for attr in v.attrs.iter() {
            if attr.path().is_ident("event") {
                match parse_variant_event_attr(attr) {
                    Ok(vc) => {
                        if let Some(lit) = vc.ty {
                            if type_lit.is_some() {
                                return syn::Error::new(
                                    attr.span(),
                                    "duplicate 'event_type' specified for this variant",
                                )
                                .to_compile_error()
                                .into();
                            }
                            type_lit = Some(lit);
                        }
                        if let Some(lit) = vc.version {
                            if version_lit_local.is_some() {
                                return syn::Error::new(
                                    attr.span(),
                                    "duplicate 'event_version' specified for this variant",
                                )
                                .to_compile_error()
                                .into();
                            }
                            version_lit_local = Some(lit);
                        }
                    }
                    Err(err) => {
                        return err.to_compile_error().into();
                    }
                }
            } else if attr.path().is_ident("event_type") || attr.path().is_ident("event_version") {
                return syn::Error::new(
                    attr.span(),
                    "legacy #[event_type]/#[event_version] syntax is no longer supported; use \
                     #[event(event_type = ..., event_version = ...)]",
                )
                .to_compile_error()
                .into();
            } else {
                retained_attrs.push(attr.clone());
            }
        }

        v.attrs = retained_attrs;
        if let Some(lit) = type_lit {
            variant_types.insert(v.ident.to_string(), lit);
        }
        if let Some(lit) = version_lit_local {
            variant_versions.insert(v.ident.to_string(), lit);
        }

        // 处理字段转换
        match &mut v.fields {
            syn::Fields::Named(fields_named) => {
                let version_ty: Type = syn::parse_quote! { ::ddd_domain::value_object::Version };
                ensure_required_fields(
                    fields_named,
                    &[("id", &id_type), ("aggregate_version", &version_ty)],
                    // reposition_existing
                    false,
                );
            }
            syn::Fields::Unit => {
                // 单元变体转换为命名字段变体: Variant => Variant { id, aggregate_version }
                v.fields = syn::Fields::Named(create_required_fields_only(&id_type));
            }
            syn::Fields::Unnamed(fields_unnamed) => {
                // 元组变体转换为命名字段变体: Variant(T) => Variant { value: T, id,
                // aggregate_version }
                v.fields = syn::Fields::Named(convert_tuple_to_named(fields_unnamed, &id_type));
            }
        }
    }

    // 生成 DomainEvent 实现
    let enum_ident = &enum_item.ident;
    let enum_name_string = enum_ident.to_string();

    let type_match_arms = enum_item.variants.iter().map(|v| {
        let v_ident = &v.ident;
        let key = v_ident.to_string();
        if let Some(lit) = variant_types.get(&key) {
            quote! { Self::#v_ident { .. } => #lit }
        } else {
            let combined = format!("{}.{}", enum_name_string, key);
            let lit = syn::LitStr::new(&combined, v_ident.span());
            quote! { Self::#v_ident { .. } => #lit }
        }
    });

    let id_match_arms = enum_item.variants.iter().map(|v| {
        let v_ident = &v.ident;
        quote! { Self::#v_ident { id, .. } => id.as_str() }
    });

    let ver_match_arms = enum_item.variants.iter().map(|v| {
        let v_ident = &v.ident;
        let key = v_ident.to_string();
        if let Some(lit) = variant_versions.get(&key) {
            quote! { Self::#v_ident { .. } => #lit }
        } else {
            quote! { Self::#v_ident { .. } => #version_lit }
        }
    });

    let agg_ver_match_arms = enum_item.variants.iter().map(|v| {
        let v_ident = &v.ident;
        quote! { Self::#v_ident { aggregate_version, .. } => *aggregate_version }
    });

    let out = quote! {
        #enum_item

        impl ::ddd_domain::domain_event::DomainEvent for #enum_ident {
            fn event_id(&self) -> &str { match self { #( #id_match_arms, )* } }
            fn event_type(&self) -> &str { match self { #( #type_match_arms, )* } }
            fn event_version(&self) -> usize { match self { #( #ver_match_arms, )* } }
            fn aggregate_version(&self) -> ::ddd_domain::value_object::Version { match self { #( #agg_ver_match_arms, )* } }
        }
    };

    TokenStream::from(out)
}

// -------- utils & parsing --------

struct VariantEventAttrConfig {
    ty: Option<syn::LitStr>,
    version: Option<syn::LitInt>,
}

fn parse_variant_event_attr(attr: &syn::Attribute) -> Result<VariantEventAttrConfig> {
    match &attr.meta {
        syn::Meta::List(_) => {
            let mut ty: Option<syn::LitStr> = None;
            let mut version: Option<syn::LitInt> = None;
            let pairs: Punctuated<VariantEventAttrKv, Token![,]> = attr
                .parse_args_with(Punctuated::<VariantEventAttrKv, Token![,]>::parse_terminated)?;

            for kv in pairs {
                match kv.key.to_string().as_str() {
                    "event_type" => {
                        if ty.is_some() {
                            return Err(syn::Error::new(
                                kv.key.span(),
                                "duplicate key 'event_type' in attribute",
                            ));
                        }
                        let lit = match kv.value {
                            Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(lit),
                                ..
                            }) => lit,
                            other => {
                                return Err(syn::Error::new(
                                    other.span(),
                                    "expected string literal for 'event_type'",
                                ));
                            }
                        };
                        ty = Some(lit);
                    }
                    "event_version" => {
                        if version.is_some() {
                            return Err(syn::Error::new(
                                kv.key.span(),
                                "duplicate key 'event_version' in attribute",
                            ));
                        }
                        let lit = match kv.value {
                            Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Int(lit),
                                ..
                            }) => lit,
                            other => {
                                return Err(syn::Error::new(
                                    other.span(),
                                    "expected integer literal for 'event_version'",
                                ));
                            }
                        };
                        version = Some(lit);
                    }
                    _ => {
                        return Err(syn::Error::new(
                            kv.key.span(),
                            "unknown key; expected 'event_type' | 'event_version'",
                        ));
                    }
                }
            }

            Ok(VariantEventAttrConfig { ty, version })
        }
        other => Err(syn::Error::new(other.span(), "expected #[event(...)]")),
    }
}

struct VariantEventAttrKv {
    key: Ident,
    #[allow(dead_code)]
    eq: Token![=],
    value: Expr,
}

impl Parse for VariantEventAttrKv {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            key: input.parse()?,
            eq: input.parse()?,
            value: input.parse()?,
        })
    }
}

/// 创建只包含 id 和 aggregate_version 的命名字段集（用于单元变体转换）
fn create_required_fields_only(id_type: &Type) -> syn::FieldsNamed {
    let version_ty: Type = syn::parse_quote! { ::ddd_domain::value_object::Version };
    syn::parse_quote! {
        {
            id: #id_type,
            aggregate_version: #version_ty
        }
    }
}

/// 将元组变体字段转换为命名字段，并添加必需字段
/// - 单字段: `Variant(T)` => `Variant { value: T, id, aggregate_version }`
/// - 多字段: `Variant(T, U)` => `Variant { value_0: T, value_1: U, id, aggregate_version }`
fn convert_tuple_to_named(fields_unnamed: &syn::FieldsUnnamed, id_type: &Type) -> syn::FieldsNamed {
    let version_ty: Type = syn::parse_quote! { ::ddd_domain::value_object::Version };
    let unnamed_fields = &fields_unnamed.unnamed;

    let named_fields: Vec<syn::Field> = if unnamed_fields.len() == 1 {
        // 单字段：使用 value 作为字段名
        let field = &unnamed_fields[0];
        let ty = &field.ty;
        vec![syn::Field {
            attrs: field.attrs.clone(),
            vis: field.vis.clone(),
            mutability: syn::FieldMutability::None,
            ident: Some(syn::Ident::new("value", proc_macro2::Span::call_site())),
            colon_token: Some(Default::default()),
            ty: ty.clone(),
        }]
    } else {
        // 多字段：使用 value_0, value_1, ... 作为字段名
        unnamed_fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let ty = &field.ty;
                syn::Field {
                    attrs: field.attrs.clone(),
                    vis: field.vis.clone(),
                    mutability: syn::FieldMutability::None,
                    ident: Some(syn::Ident::new(
                        &format!("value_{}", i),
                        proc_macro2::Span::call_site(),
                    )),
                    colon_token: Some(Default::default()),
                    ty: ty.clone(),
                }
            })
            .collect()
    };

    // 添加必需字段
    let id_field: syn::Field = syn::parse_quote! { id: #id_type };
    let version_field: syn::Field = syn::parse_quote! { aggregate_version: #version_ty };

    let mut all_fields: Punctuated<syn::Field, Token![,]> = Punctuated::new();
    for f in named_fields {
        all_fields.push(f);
    }
    all_fields.push(id_field);
    all_fields.push(version_field);

    syn::FieldsNamed {
        brace_token: Default::default(),
        named: all_fields,
    }
}

// 枚举级配置：id 类型、默认版本号
struct EventAttrConfig {
    id_ty: Option<Type>,
    version: Option<syn::LitInt>,
}

impl Parse for EventAttrConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut id_ty: Option<Type> = None;
        let mut version: Option<syn::LitInt> = None;

        if input.is_empty() {
            return Ok(Self { id_ty, version });
        }

        let pairs: Punctuated<syn::ExprAssign, Token![,]> =
            Punctuated::<syn::ExprAssign, Token![,]>::parse_terminated(input)?;

        for assign in pairs.into_iter() {
            let key_ident = match *assign.left {
                syn::Expr::Path(p) if p.path.segments.len() == 1 => {
                    p.path.segments[0].ident.clone()
                }
                other => return Err(syn::Error::new(other.span(), "invalid attribute key")),
            };
            match key_ident.to_string().as_str() {
                "id" => {
                    if id_ty.is_some() {
                        return Err(syn::Error::new(
                            key_ident.span(),
                            "duplicate key 'id' in attribute",
                        ));
                    }
                    let ty_parsed: Type = syn::parse2(assign.right.to_token_stream())?;
                    id_ty = Some(ty_parsed);
                }
                "version" => {
                    if version.is_some() {
                        return Err(syn::Error::new(
                            key_ident.span(),
                            "duplicate key 'version' in attribute",
                        ));
                    }
                    let lit: syn::LitInt = syn::parse2(assign.right.to_token_stream())?;
                    version = Some(lit);
                }
                _ => {
                    return Err(syn::Error::new(
                        key_ident.span(),
                        "unknown key; expected 'id' | 'version'",
                    ));
                }
            }
        }

        Ok(Self { id_ty, version })
    }
}
