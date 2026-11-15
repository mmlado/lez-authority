//! Macros for lez-authority. The `guard` attribute puts the guard in front of a plain LEZ
//! handler. `HasSlot` comes from fields marked `#[slot(Role)]`, and `Gated` from messages
//! marked `#[only(Role)]` or `#[from_config]`. The derives write only the trait impls a
//! program could write by hand.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote, quote_spanned};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Error, Expr, FnArg, ItemFn, Meta, Path, Token, Type};

/// Puts the guard in front of a plain LEZ handler, `fn(&ReceiveInput, Message) -> Response`,
/// which stays as it is under `lee_core::actor_entrypoint!`.
///
/// ```ignore
/// #[guard(roles(Admin), config = Config, seed = CONFIG_SEED)]
/// fn handle_message(input: &ReceiveInput, message: Message) -> Response {
///     // the program's own code
/// }
///
/// lee_core::actor_entrypoint!(handle_message);
/// ```
///
/// `roles(..)` names the program's roles, the first being the one whose `Init` creates the
/// config. `config` is the state the program keeps on the config account, and `seed` that
/// account's seed. `genesis = KEY` lets only `KEY` install the first holder, instead of the
/// deployer.
///
/// The handler becomes the body of an inner function of the same name. The outer one takes
/// `Wire<Message>`, so the library's messages decode next to the program's, and hands the
/// program's to the inner one through `lez_authority::guard`.
#[proc_macro_attribute]
pub fn guard(args: TokenStream, item: TokenStream) -> TokenStream {
    krate()
        .and_then(|krate| guard_handler(args.into(), item.into(), &krate))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// Implements `HasSlot<R>` for every field marked `#[slot(R)]`. The field holds the role's
/// slot type, `R::Slot`, which is `AuthoritySlot` for a role that keeps only its holder.
///
/// ```ignore
/// #[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
/// struct Config {
///     value: u64,
///     #[slot(Admin)]
///     admin: AuthoritySlot,
/// }
/// ```
#[proc_macro_derive(HasSlot, attributes(slot))]
pub fn derive_has_slot(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    krate()
        .and_then(|krate| has_slot(&input, &krate))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// Implements `Gated` from markers on the message enum's variants: `#[only(R)]` on a message
/// that needs role `R`'s holder, one role per message, and `#[from_config]` on one that only
/// the config account may send.
///
/// ```ignore
/// #[derive(BorshSerialize, BorshDeserialize, Gated)]
/// enum Message {
///     SetOpenValue(u64),
///     #[only(Admin)]
///     SetAdminValue(u64),
///     #[from_config]
///     TargetValue(u64),
/// }
/// ```
#[proc_macro_derive(Gated, attributes(only, from_config))]
pub fn derive_gated(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    krate()
        .and_then(|krate| gated(&input, &krate))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// The path to lez-authority from the program's crate.
fn krate() -> syn::Result<TokenStream2> {
    use proc_macro_crate::{FoundCrate, crate_name};
    let fallback = || Ok(quote!(::lez_authority));
    let Some((manifest, program, workspace)) = read_manifests() else {
        return fallback();
    };
    let Some(candidates) = candidates(&program, workspace.as_deref()) else {
        return fallback();
    };
    if candidates.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            format!(
                "lez-authority's macros found no dependency named lez-authority or lez-<role>-authority in {}",
                manifest.display()
            ),
        ));
    }
    let found = candidates.iter().find_map(|name| match crate_name(name) {
        Ok(FoundCrate::Itself) => Some(quote!(::lez_authority)),
        Ok(FoundCrate::Name(imported)) => {
            let imported = format_ident!("{imported}");
            Some(if name == "lez-authority" {
                quote!(::#imported)
            } else {
                quote!(::#imported::lez_authority)
            })
        }
        _ => None,
    });
    found.map_or_else(fallback, Ok)
}

/// The program's manifest path and text, and its workspace root's text if it has one.
fn read_manifests() -> Option<(std::path::PathBuf, String, Option<String>)> {
    let dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let manifest = dir.join("Cargo.toml");
    let program = std::fs::read_to_string(&manifest).ok()?;
    let workspace = dir.ancestors().find_map(|dir| {
        let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
        let parsed = text.parse::<toml_edit::DocumentMut>().ok()?;
        parsed.contains_key("workspace").then_some(text)
    });
    Some((manifest, program, workspace))
}

/// The program's dependencies that may lead to lez-authority, in the order the macros try them:
/// lez-authority itself, or a role crate, which is named `lez-<role>-authority` and re-exports
/// lez-authority at its root. `None` when the manifest does not parse.
fn candidates(program: &str, workspace: Option<&str>) -> Option<Vec<String>> {
    use toml_edit::{DocumentMut, Item, TableLike};
    let program = program.parse::<DocumentMut>().ok()?;
    let package = program
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Item::as_str);
    // Inside lez-authority itself, whose tests and examples do not list it as a dependency.
    if package == Some("lez-authority") {
        return Some(vec!["lez-authority".to_owned()]);
    }
    let workspace = workspace.and_then(|text| text.parse::<DocumentMut>().ok());
    let inherited = workspace
        .as_ref()
        .and_then(|root| root.get("workspace"))
        .and_then(|table| table.get("dependencies"))
        .and_then(Item::as_table_like);
    let mut owners: Vec<(&dyn TableLike, bool)> = vec![(program.as_table(), false)];
    if let Some(targets) = program.get("target").and_then(Item::as_table_like) {
        owners.extend(
            targets
                .iter()
                .filter_map(|(_, target)| Some((target.as_table_like()?, true))),
        );
    }
    let mut found: Vec<(u8, bool, String)> = Vec::new();
    for (owner, in_target) in owners {
        for kind in ["dependencies", "dev-dependencies"] {
            let Some(table) = owner.get(kind).and_then(Item::as_table_like) else {
                continue;
            };
            for (key, dependency) in table.iter() {
                // A renamed workspace dependency names its package only in the root manifest.
                let declared = match inherited {
                    Some(root)
                        if dependency.get("workspace").and_then(Item::as_bool) == Some(true) =>
                    {
                        root.get(key).unwrap_or(dependency)
                    }
                    _ => dependency,
                };
                let name = declared
                    .get("package")
                    .and_then(Item::as_str)
                    .unwrap_or(key);
                if name != "lez-authority" && !is_role_crate(name) {
                    continue;
                }
                let optional = dependency.get("optional").and_then(Item::as_bool) == Some(true);
                // A crate the normal build does not link must not shadow one it does.
                let tier = match (kind, optional, in_target) {
                    ("dev-dependencies", _, _) => 3,
                    (_, true, _) => 2,
                    (_, _, true) => 1,
                    _ => 0,
                };
                found.push((tier, name != "lez-authority", name.to_owned()));
            }
        }
    }
    found.sort();
    let mut seen = std::collections::BTreeSet::new();
    Some(
        found
            .into_iter()
            .filter_map(|(_, _, name)| seen.insert(name.clone()).then_some(name))
            .collect(),
    )
}

/// Whether a crate is named as a role crate is, `lez-<role>-authority`.
fn is_role_crate(name: &str) -> bool {
    name.strip_prefix("lez-")
        .and_then(|rest| rest.strip_suffix("-authority"))
        .is_some_and(|role| !role.is_empty())
}

#[derive(Default)]
struct GuardArgs {
    roles: Vec<Path>,
    config: Option<Type>,
    seed: Option<Expr>,
    genesis: Option<Expr>,
}

fn guard_args(args: TokenStream2) -> syn::Result<GuardArgs> {
    let mut parsed = GuardArgs::default();
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("roles") {
            if !parsed.roles.is_empty() {
                return Err(meta.error("roles(..) is given twice"));
            }
            meta.parse_nested_meta(|role| {
                parsed.roles.push(role.path);
                Ok(())
            })
        } else if meta.path.is_ident("config") {
            parsed.config = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("seed") {
            parsed.seed = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("genesis") {
            parsed.genesis = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error("expected roles(..), config = .., seed = .. or genesis = .."))
        }
    });
    syn::parse::Parser::parse2(parser, args)?;
    Ok(parsed)
}

fn guard_handler(
    args: TokenStream2,
    item: TokenStream2,
    krate: &TokenStream2,
) -> syn::Result<TokenStream2> {
    let span = args.span();
    let GuardArgs {
        roles,
        config,
        seed,
        genesis,
    } = guard_args(args)?;
    if roles.is_empty() {
        return Err(Error::new(
            span,
            "the guard needs roles(..), as in roles(Admin)",
        ));
    }
    if roles.len() > 2 {
        return Err(Error::new(span, "the guard takes one role or two"));
    }
    let config = config.ok_or_else(|| {
        Error::new(
            span,
            "the guard needs config = the state kept on the config account",
        )
    })?;
    let seed =
        seed.ok_or_else(|| Error::new(span, "the guard needs seed = the config account's seed"))?;
    let gate = match genesis {
        Some(key) => quote!(#krate::InitGate::Genesis(#key)),
        None => quote!(#krate::InitGate::Deployer),
    };

    let handler: ItemFn = syn::parse2(item)?;
    let signature = &handler.sig;
    if !signature.generics.params.is_empty() || signature.asyncness.is_some() {
        return Err(Error::new(
            signature.span(),
            "the guard goes on a plain handler, fn(&ReceiveInput, Message) -> Response",
        ));
    }
    let params: Vec<&FnArg> = signature.inputs.iter().collect();
    let [FnArg::Typed(input), FnArg::Typed(message)] = params.as_slice() else {
        return Err(Error::new(
            signature.inputs.span(),
            "the guard goes on a handler of two parameters, the input and the message",
        ));
    };
    let input_type = &input.ty;
    let message_type = &message.ty;
    let name = &signature.ident;
    let output = &signature.output;
    let visibility = &handler.vis;
    let attributes = &handler.attrs;
    let mut inner = handler.clone();
    inner.attrs.clear();
    inner.vis = syn::Visibility::Inherited;
    // cargo check skips the guard's post-monomorphization assert, so check the roles here too.
    let at_roles = Span::call_site().located_at(roles[0].span());
    let root_check = quote_spanned! {at_roles=>
        const _: () = ::core::assert!(
            !<(#(#roles,)*) as #krate::Roles<#config, #message_type>>::ROOT_BY_NOMINATION_ONLY,
            "the first role creates the config with its Init, so its holder cannot be appointed only by nomination"
        );
    };
    Ok(quote! {
        #(#attributes)*
        #visibility fn #name(
            __lez_input: #input_type,
            __lez_message: #krate::Wire<#message_type>,
        ) #output {
            #root_check
            #inner
            #krate::guard::<(#(#roles,)*), #config, #message_type>(
                __lez_input,
                &#seed,
                #gate,
                __lez_message,
                #name,
            )
        }
    })
}

fn has_slot(input: &DeriveInput, krate: &TokenStream2) -> syn::Result<TokenStream2> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.ident.span(),
            "HasSlot derives on the config struct",
        ));
    };
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let mut roles: Vec<String> = Vec::new();
    let mut impls = Vec::new();
    for (index, field) in data.fields.iter().enumerate() {
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("slot"))
        {
            let role: Path = attr.parse_args()?;
            let key = key(&role);
            if roles.contains(&key) {
                return Err(Error::new(attr.span(), format!("a second slot for {key}")));
            }
            roles.push(key);
            // Call-site hygiene so `self` resolves in macro_rules! configs, errors on the type.
            let ty = Span::call_site().located_at(field.ty.span());
            let member = match &field.ident {
                Some(ident) => quote!(#ident),
                None => {
                    let index = syn::Index {
                        span: ty,
                        ..syn::Index::from(index)
                    };
                    quote!(#index)
                }
            };
            let slot = quote_spanned!(ty=> &self.#member);
            let slot_mut = quote_spanned!(ty=> &mut self.#member);
            impls.push(quote! {
                impl #impl_generics #krate::HasSlot<#role> for #name #type_generics #where_clause {
                    fn slot(&self) -> &<#role as #krate::Role>::Slot {
                        #slot
                    }
                    fn slot_mut(&mut self) -> &mut <#role as #krate::Role>::Slot {
                        #slot_mut
                    }
                }
            });
        }
    }
    if impls.is_empty() {
        return Err(Error::new(
            name.span(),
            "HasSlot needs a field marked #[slot(Role)]",
        ));
    }
    Ok(quote!(#(#impls)*))
}

const ONE_ROLE: &str =
    "#[only] takes one role, as in #[only(Admin)]: a message for two roles is not supported yet";

fn gated(input: &DeriveInput, krate: &TokenStream2) -> syn::Result<TokenStream2> {
    let Data::Enum(data) = &input.data else {
        return Err(Error::new(
            input.ident.span(),
            "Gated derives on the message enum",
        ));
    };
    if data.variants.len() >= 255 {
        return Err(Error::new(
            input.ident.span(),
            "the message enum must stay below 255 variants: the first byte 255 marks the library's messages",
        ));
    }
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let mut needs = Vec::new();
    let mut from_config = Vec::new();
    for variant in &data.variants {
        let ident = &variant.ident;
        let mut role: Option<Path> = None;
        let mut config_only = false;
        for attr in &variant.attrs {
            if attr.path().is_ident("only") {
                let one_role = || Error::new(attr.span(), ONE_ROLE);
                let listed = attr
                    .parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)
                    .map_err(|_| one_role())?;
                let mut listed = listed.into_iter();
                match (listed.next(), listed.next(), &role) {
                    (Some(only), None, None) => role = Some(only),
                    _ => return Err(one_role()),
                }
            } else if attr.path().is_ident("from_config") {
                if !matches!(attr.meta, Meta::Path(_)) {
                    return Err(Error::new(attr.span(), "#[from_config] takes no arguments"));
                }
                config_only = true;
            }
        }
        if config_only && role.is_some() {
            return Err(Error::new(
                variant.span(),
                "a message is either #[only(..)], sent through the sender's seat, or #[from_config], sent by the config",
            ));
        }
        if let Some(role) = &role {
            needs.push(quote! {
                Self::#ident { .. } => role == ::core::any::TypeId::of::<#role>(),
            });
        }
        if config_only {
            from_config.push(ident);
        }
    }
    let needs = (!needs.is_empty()).then(|| {
        quote! {
            fn needs<__R: #krate::Role>(&self) -> bool {
                let role = ::core::any::TypeId::of::<__R>();
                #[allow(unreachable_patterns)]
                match self {
                    #(#needs)*
                    _ => false,
                }
            }
        }
    });
    let from_config = (!from_config.is_empty()).then(|| {
        quote! {
            fn only_from_config(&self) -> bool {
                ::core::matches!(self, #(Self::#from_config { .. })|*)
            }
        }
    });
    Ok(quote! {
        impl #impl_generics #krate::Gated for #name #type_generics #where_clause {
            #needs
            #from_config
        }
    })
}

/// A role named the way the program wrote it, for comparing markers.
fn key(role: &Path) -> String {
    quote!(#role).to_string().replace(' ', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(
        derive: fn(&DeriveInput, &TokenStream2) -> syn::Result<TokenStream2>,
        source: &str,
    ) -> String {
        let input: DeriveInput = syn::parse_str(source).unwrap();
        derive(&input, &quote!(::lez_authority))
            .expect_err("the derive refuses this input")
            .to_string()
    }

    fn guard_refusal(args: &str, item: &str) -> String {
        guard_handler(
            args.parse().unwrap(),
            item.parse().unwrap(),
            &quote!(::lez_authority),
        )
        .expect_err("the attribute refuses this input")
        .to_string()
    }

    const HANDLER: &str =
        "fn handle(input: &ReceiveInput, message: Message) -> Response { todo!() }";

    #[test]
    fn a_role_crate_is_named_lez_role_authority() {
        for name in [
            "lez-admin-authority",
            "lez-freeze-authority",
            "lez-pause-authority",
        ] {
            assert!(is_role_crate(name), "{name}");
        }
        for name in [
            "lez-authority",
            "lez--authority",
            "lez-admin",
            "admin-authority",
            "spel-admin-authority",
        ] {
            assert!(!is_role_crate(name), "{name}");
        }
    }

    #[test]
    fn the_candidates_come_in_the_order_the_build_links_them() {
        let program = r#"
            [package]
            name = "program"

            [dependencies]
            borsh = "1"
            lez-freeze-authority = { git = "https://example.org/freeze", tag = "v0.1.0" }
            kanon = { package = "lez-kanon-authority", path = "../kanon" }
            lez-maybe-authority = { version = "0.1", optional = true }
            adm.workspace = true

            [dev-dependencies]
            lez-authority = "0.3"
            lez-admin-authority = "0.3"

            [target.'cfg(unix)'.dependencies.lez-pause-authority]
            version = "0.1"

            [target.'cfg(unix)'.dev-dependencies]
            lez-test-authority = "0.1"

            [build-dependencies]
            lez-build-authority = "0.1"
        "#;
        let workspace = r#"
            [workspace]
            members = ["program"]

            [workspace.dependencies]
            adm = { package = "lez-admin-authority", path = "../admin" }
        "#;
        assert_eq!(
            candidates(program, Some(workspace)).unwrap(),
            [
                "lez-admin-authority",
                "lez-freeze-authority",
                "lez-kanon-authority",
                "lez-pause-authority",
                "lez-maybe-authority",
                "lez-authority",
                "lez-test-authority",
            ]
        );
    }

    #[test]
    fn an_inherited_dependency_is_named_by_the_workspace_root() {
        let program = "[dependencies]\nadm.workspace = true\n";
        let workspace = "[workspace.dependencies]\nadm = { package = \"lez-admin-authority\", path = \"../admin\" }\n";
        assert_eq!(
            candidates(program, Some(workspace)).unwrap(),
            ["lez-admin-authority"]
        );
        assert!(candidates(program, None).unwrap().is_empty());
        assert!(candidates("not toml [", None).is_none());
    }

    #[test]
    fn inside_the_engine_the_engine_is_the_only_candidate() {
        let engine = "[package]\nname = \"lez-authority\"\n[dependencies]\nlez-authority-macros = { path = \"macros\" }\n";
        assert_eq!(candidates(engine, None).unwrap(), ["lez-authority"]);
    }

    #[test]
    fn the_engine_comes_first_within_its_tier() {
        let program = "[dependencies]\nlez-admin-authority = \"0.3\"\nlez-authority = \"0.3\"\n";
        assert_eq!(
            candidates(program, None).unwrap(),
            ["lez-authority", "lez-admin-authority"]
        );
    }

    #[test]
    fn has_slot_needs_a_struct_with_a_marked_field() {
        assert_eq!(
            refusal(has_slot, "enum Config { A }"),
            "HasSlot derives on the config struct"
        );
        assert_eq!(
            refusal(has_slot, "struct Config { admin: AuthoritySlot }"),
            "HasSlot needs a field marked #[slot(Role)]"
        );
        assert_eq!(
            refusal(
                has_slot,
                "struct Config { #[slot(Admin)] a: AuthoritySlot, #[slot(Admin)] b: AuthoritySlot }"
            ),
            "a second slot for Admin"
        );
    }

    #[test]
    fn gated_refuses_misplaced_markers() {
        assert_eq!(
            refusal(gated, "struct Message;"),
            "Gated derives on the message enum"
        );
        assert_eq!(
            refusal(
                gated,
                "enum Message { #[only(Admin)] #[from_config] Set(u64) }"
            ),
            "a message is either #[only(..)], sent through the sender's seat, or #[from_config], sent by the config"
        );
        assert_eq!(
            refusal(gated, "enum Message { #[from_config(x)] Set(u64) }"),
            "#[from_config] takes no arguments"
        );
        let many: Vec<String> = (0..255).map(|i| format!("V{i}")).collect();
        assert_eq!(
            refusal(gated, &format!("enum Message {{ {} }}", many.join(", "))),
            "the message enum must stay below 255 variants: the first byte 255 marks the library's messages"
        );
    }

    #[test]
    fn only_takes_one_role() {
        let one_role = "#[only] takes one role, as in #[only(Admin)]: a message for two roles is not supported yet";
        assert_eq!(
            refusal(gated, "enum Message { #[only(Admin, Freeze)] Set(u64) }"),
            one_role
        );
        assert_eq!(
            refusal(gated, "enum Message { #[only] Set(u64) }"),
            one_role
        );
        assert_eq!(
            refusal(
                gated,
                "enum Message { #[only(Admin)] #[only(Freeze)] Set(u64) }"
            ),
            one_role
        );
    }

    #[test]
    fn the_guard_checks_its_first_role_in_a_const_cargo_check_evaluates() {
        let expanded: String = guard_handler(
            "roles(Admin, Freeze), config = Config, seed = SEED"
                .parse()
                .unwrap(),
            HANDLER.parse().unwrap(),
            &quote!(::lez_authority),
        )
        .unwrap()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
        assert!(
            expanded.contains("const_:()=::core::assert!(!<(Admin,Freeze,)as::lez_authority::Roles<Config,Message>>::ROOT_BY_NOMINATION_ONLY"),
            "{expanded}"
        );
    }

    #[test]
    fn the_guard_needs_its_arguments_and_a_plain_handler() {
        assert_eq!(
            guard_refusal("config = Config, seed = SEED", HANDLER),
            "the guard needs roles(..), as in roles(Admin)"
        );
        assert_eq!(
            guard_refusal("roles(Admin), seed = SEED", HANDLER),
            "the guard needs config = the state kept on the config account"
        );
        assert_eq!(
            guard_refusal("roles(Admin), config = Config", HANDLER),
            "the guard needs seed = the config account's seed"
        );
        assert_eq!(
            guard_refusal("roles(A, B, C), config = Config, seed = SEED", HANDLER),
            "the guard takes one role or two"
        );
        assert_eq!(
            guard_refusal(
                "roles(Admin), config = Config, seed = SEED, owner = X",
                HANDLER
            ),
            "expected roles(..), config = .., seed = .. or genesis = .."
        );
        assert_eq!(
            guard_refusal(
                "roles(Admin), config = Config, seed = SEED",
                "fn handle(message: Message) -> Response { todo!() }"
            ),
            "the guard goes on a handler of two parameters, the input and the message"
        );
        assert_eq!(
            guard_refusal(
                "roles(Admin), config = Config, seed = SEED",
                "fn handle<T>(input: &ReceiveInput, message: T) -> Response { todo!() }"
            ),
            "the guard goes on a plain handler, fn(&ReceiveInput, Message) -> Response"
        );
    }
}
