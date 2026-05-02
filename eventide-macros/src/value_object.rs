use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Item, Result, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
};

use crate::utils::{apply_derives, serde_crate_attr, serde_path};

/// #[value_object] 宏实现
/// - 支持结构体（具名或 tuple）与枚举
/// - 合并/追加派生：Default, Clone, (Debug 可控), Serialize, Deserialize, PartialEq, Eq
/// - 参数：
///   - `#[value_object(debug = true|false)]`，默认 true
///   - `#[value_object(default = true|false)]`，默认 true（设为 false 用于含数据变体的枚举）
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let cfg = parse_macro_input!(attr as ValueObjectAttrConfig);
    let mut input = parse_macro_input!(item as Item);

    let serde = serde_path();

    // 组装需要的 derive 集合（struct/enum 通用）
    let mut required: Vec<syn::Path> = vec![
        syn::parse_quote!(Clone),
        syn::parse_quote!(#serde::Serialize),
        syn::parse_quote!(#serde::Deserialize),
        syn::parse_quote!(PartialEq),
        syn::parse_quote!(Eq),
    ];

    if cfg.derive_default.unwrap_or(true) {
        required.insert(0, syn::parse_quote!(Default));
    }

    if cfg.derive_debug.unwrap_or(true) {
        required.insert(0, syn::parse_quote!(Debug));
    }

    let helpers = vec![serde_crate_attr()];

    match &mut input {
        Item::Struct(st) => {
            apply_derives(&mut st.attrs, required, helpers);
            TokenStream::from(quote! { #st })
        }
        Item::Enum(en) => {
            apply_derives(&mut en.attrs, required, helpers);
            TokenStream::from(quote! { #en })
        }
        other => syn::Error::new(other.span(), "#[value_object] only supports struct or enum")
            .to_compile_error()
            .into(),
    }
}

// -------- parsing --------

struct ValueObjectAttrConfig {
    derive_debug: Option<bool>,
    derive_default: Option<bool>,
}

impl Parse for ValueObjectAttrConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.is_empty() {
            return Ok(Self {
                derive_debug: None,
                derive_default: None,
            });
        }

        let mut derive_debug: Option<bool> = None;
        let mut derive_default: Option<bool> = None;
        let pairs: Punctuated<ValueObjectAttrElem, Token![,]> =
            Punctuated::parse_terminated(input)?;

        for elem in pairs {
            match elem {
                ValueObjectAttrElem::Debug(b) => {
                    if derive_debug.is_some() {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            "duplicate key 'debug' in attribute",
                        ));
                    }
                    derive_debug = Some(b);
                }
                ValueObjectAttrElem::Default(b) => {
                    if derive_default.is_some() {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            "duplicate key 'default' in attribute",
                        ));
                    }
                    derive_default = Some(b);
                }
            }
        }
        Ok(Self {
            derive_debug,
            derive_default,
        })
    }
}

enum ValueObjectAttrElem {
    Debug(bool),
    Default(bool),
}

impl Parse for ValueObjectAttrElem {
    fn parse(input: ParseStream) -> Result<Self> {
        let key: syn::Ident = input.parse()?;
        let _eq: Token![=] = input.parse()?;
        let expr: syn::Expr = input.parse()?;

        let value = match expr {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Bool(b),
                ..
            }) => b.value(),
            other => return Err(syn::Error::new(other.span(), "expected boolean literal")),
        };

        if key == "debug" {
            Ok(Self::Debug(value))
        } else if key == "default" {
            Ok(Self::Default(value))
        } else {
            Err(syn::Error::new(
                key.span(),
                "unknown key in attribute; expected 'debug' or 'default'",
            ))
        }
    }
}
