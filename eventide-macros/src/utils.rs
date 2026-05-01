use quote::ToTokens;
use syn::{Attribute, Field, FieldsNamed, Token, Type, punctuated::Punctuated};

// Split a list of attributes into two pieces: the non-`derive`
// attributes (which we want to keep verbatim) and the flat list of
// derive paths that were inside `#[derive(...)]` clauses (which we will
// merge with the macro's required set).
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

// Merge the macro's required derives with the user's existing derives,
// de-duplicating by normalised key (see `derive_key`). Required derives
// are pushed first, so they always win when there is a name clash, and
// user derives that did not already appear are appended afterwards. The
// final `Attribute` returned is a fresh `#[derive(...)]` containing the
// merged list.
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

// Compute a deduplication key for a derive path. We collapse the
// last-segment ident to a canonical form so that paths which name the
// same trait under different prefixes (e.g. plain `Serialize` and
// `serde::Serialize`) hash to the same key and are not duplicated in
// the merged list.
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

// Convenience wrapper that applies the merge in place: it splits the
// existing attribute list, merges the derives, and then replaces
// `*attrs` with a single normalised `#[derive(...)]` attribute followed
// by the original non-derive attributes.
pub(crate) fn apply_derives(attrs: &mut Vec<Attribute>, required: Vec<syn::Path>) {
    let (retained, existing) = split_derives(attrs);
    let merged = merge_derives(existing, required);
    *attrs = std::iter::once(merged).chain(retained).collect();
}

/// Ensure a named-field struct or enum variant contains the requested
/// fields, with two slightly different policies depending on the caller.
///
/// Parameters:
/// - `required` — list of `(field_name, field_type)` pairs processed in
///   the given order. The fields are inserted with the supplied types
///   when missing; existing fields with matching names are reused as-is
///   (their original type is preserved, even if it differs from the
///   one passed in).
/// - `reposition_existing`:
///   * `true` — the required fields are moved to the very front of the
///     struct in the order they appear in `required`, regardless of
///     where the user wrote them. This is what `#[entity]` needs so
///     that `id`/`version` always lead the struct layout.
///   * `false` — required fields are only *appended* (in front of the
///     user's fields) when missing; if they already exist their
///     original position is preserved. This is what `#[domain_event]`
///     needs so the user's per-variant fields keep their original
///     ordering.
pub(crate) fn ensure_required_fields(
    fields_named: &mut FieldsNamed,
    required: &[(&str, &Type)],
    reposition_existing: bool,
) {
    let old_named = fields_named.named.clone();
    let mut new_named: Punctuated<Field, Token![,]> = Punctuated::new();

    if reposition_existing {
        // Place the required fields at the front in the requested order:
        // reuse the user's existing field (preserving its attributes /
        // type) if one exists with that name, otherwise synthesise a new
        // field of the supplied type.
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

        // Append the remaining (non-required) fields in their original
        // order so the user's intended layout is preserved aside from
        // the lifted required-field prefix.
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
        // Append-only mode: only insert required fields that are
        // missing (placing them at the front), and otherwise keep the
        // user's original ordering untouched.
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
