//! Macros for lez-authority. The `guard` attribute puts the guard in front of a plain LEZ
//! handler. `HasSlot` comes from fields marked `#[slot(Role)]`, and `Gated` from messages
//! marked `#[only(Role)]` or `#[from_config]`. The derives write only the trait impls a
//! program could write by hand.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
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
    guard_handler(args.into(), item.into(), &krate())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// Implements `HasSlot<R>` for every field marked `#[slot(R)]`.
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
    has_slot(&input, &krate())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// Implements `Gated` from markers on the message enum's variants: `#[only(R)]` on a message
/// that needs role `R`'s holder, `#[only(A, B)]` for both, and `#[from_config]` on one that
/// only the config account may send.
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
    gated(&input, &krate())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// The path to lez-authority from the program's crate.
fn krate() -> TokenStream2 {
    use proc_macro_crate::{FoundCrate, crate_name};
    // A program may depend on lez-authority only through a role crate that re-exports it.
    match crate_name("lez-authority") {
        Ok(FoundCrate::Name(name)) => {
            let name = format_ident!("{name}");
            quote!(::#name)
        }
        Ok(FoundCrate::Itself) => quote!(::lez_authority),
        Err(_) => match crate_name("lez-admin-authority") {
            Ok(FoundCrate::Name(name)) => {
                let name = format_ident!("{name}");
                quote!(::#name::lez_authority)
            }
            _ => quote!(::lez_authority),
        },
    }
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
    Ok(quote! {
        #(#attributes)*
        #visibility fn #name(
            __lez_input: #input_type,
            __lez_message: #krate::Wire<#message_type>,
        ) #output {
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
            let member = match &field.ident {
                Some(ident) => quote!(#ident),
                None => {
                    let index = syn::Index::from(index);
                    quote!(#index)
                }
            };
            impls.push(quote! {
                impl #impl_generics #krate::HasSlot<#role> for #name #type_generics #where_clause {
                    fn slot(&self) -> &#krate::AuthoritySlot {
                        &self.#member
                    }
                    fn slot_mut(&mut self) -> &mut #krate::AuthoritySlot {
                        &mut self.#member
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
        let mut roles: Vec<Path> = Vec::new();
        let mut config_only = false;
        for attr in &variant.attrs {
            if attr.path().is_ident("only") {
                let listed =
                    attr.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)?;
                if listed.is_empty() {
                    return Err(Error::new(
                        attr.span(),
                        "#[only] names a role, as in #[only(Admin)]",
                    ));
                }
                for role in listed {
                    if roles.iter().any(|known| key(known) == key(&role)) {
                        return Err(Error::new(
                            attr.span(),
                            format!("{} is named twice", key(&role)),
                        ));
                    }
                    roles.push(role);
                }
            } else if attr.path().is_ident("from_config") {
                if !matches!(attr.meta, Meta::Path(_)) {
                    return Err(Error::new(attr.span(), "#[from_config] takes no arguments"));
                }
                config_only = true;
            }
        }
        if config_only && !roles.is_empty() {
            return Err(Error::new(
                variant.span(),
                "a message is either #[only(..)], sent through the sender's seat, or #[from_config], sent by the config",
            ));
        }
        if !roles.is_empty() {
            needs.push(quote! {
                Self::#ident { .. } => #(role == ::core::any::TypeId::of::<#roles>())||*,
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
            refusal(gated, "enum Message { #[only(Admin, Admin)] Set(u64) }"),
            "Admin is named twice"
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
