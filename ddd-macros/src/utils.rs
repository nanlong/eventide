use quote::ToTokens;
use syn::{Attribute, Field, FieldsNamed, Token, Type, punctuated::Punctuated};

// 提取非 derive 属性与已有 derive 列表
pub(crate) fn split_derives(attrs: &[Attribute]) -> (Vec<Attribute>, Vec<syn::Path>) {
    let mut retained = Vec::new();
    let mut existing = Vec::new();
    for attr in attrs.iter() {
        if attr.path().is_ident("derive") {
            if let Ok(list) = attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, Token![,]>::parse_terminated,
            ) {
                for p in list.into_iter() {
                    existing.push(p);
                }
            }
        } else {
            retained.push(attr.clone());
        }
    }
    (retained, existing)
}

// 合并默认与已有 derive（去重，优先保留 required）
pub(crate) fn merge_derives(existing: Vec<syn::Path>, required: Vec<syn::Path>) -> Attribute {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut final_list: Vec<syn::Path> = Vec::new();
    let mut push_unique = |p: syn::Path| {
        let key = derive_key(&p);
        if seen.insert(key) {
            final_list.push(p);
        }
    };
    for p in required {
        push_unique(p);
    }
    for p in existing {
        push_unique(p);
    }
    syn::parse_quote!(#[derive(#(#final_list),*)])
}

// 归一化 derive 的 key，避免 Serialize/serde::Serialize 重复
pub(crate) fn derive_key(p: &syn::Path) -> String {
    if let Some(last) = p.segments.last() {
        let last_ident = last.ident.to_string();
        match last_ident.as_str() {
            "Serialize" | "Deserialize" => format!("serde::{}", last_ident),
            _ => last_ident,
        }
    } else {
        p.to_token_stream().to_string()
    }
}

// 直接在 attrs 上应用默认派生合并
pub(crate) fn apply_derives(attrs: &mut Vec<Attribute>, required: Vec<syn::Path>) {
    let (retained, existing) = split_derives(attrs);
    let merged = merge_derives(existing, required);
    *attrs = std::iter::once(merged).chain(retained).collect();
}

/// 确保具名字段结构体/变体包含所需字段
/// - required: (字段名, 字段类型) 列表，按给定顺序处理
/// - reposition_existing: 若为 true，则即使已存在也会把所需字段移至最前（entity 需要）； 若为
///   false，则仅在缺失时追加，保留既有顺序（event 需要）。
pub(crate) fn ensure_required_fields(
    fields_named: &mut FieldsNamed,
    required: &[(&str, &Type)],
    reposition_existing: bool,
) {
    let old_named = fields_named.named.clone();
    let mut new_named: Punctuated<Field, Token![,]> = Punctuated::new();

    if reposition_existing {
        // 把需要的字段按顺序放在前面：若存在则复用原字段；否则创建新字段
        for (name, ty) in required.iter() {
            if let Some(existing) = old_named
                .iter()
                .find(|f| f.ident.as_ref().map(|i| i == *name).unwrap_or(false))
            {
                new_named.push(existing.clone());
            } else {
                let ident: syn::Ident = syn::parse_str(name).expect("valid field ident");
                let field: Field = syn::parse_quote! { #ident: #ty };
                new_named.push(field);
            }
        }

        // 其余非必需字段保持原始顺序
        for f in old_named.into_iter() {
            let is_required = f
                .ident
                .as_ref()
                .map(|i| required.iter().any(|(n, _)| i == n))
                .unwrap_or(false);
            if !is_required {
                new_named.push(f);
            }
        }
    } else {
        // 仅在缺失时追加（位于最前），否则保留原始顺序
        for (name, ty) in required.iter() {
            if !has_field_named_in(&old_named, name) {
                let ident: syn::Ident = syn::parse_str(name).expect("valid field ident");
                let field: Field = syn::parse_quote! { #ident: #ty };
                new_named.push(field);
            }
        }
        for f in old_named.into_iter() {
            new_named.push(f);
        }
    }

    fields_named.named = new_named;
}

fn has_field_named_in(named: &Punctuated<Field, Token![,]>, name: &str) -> bool {
    named
        .iter()
        .any(|f| f.ident.as_ref().map(|i| i == name).unwrap_or(false))
}
