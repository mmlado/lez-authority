//! The `admin` attribute: lez-authority's `guard` with the admin as the program's role.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Error, Expr, Type};

/// Puts the guard in front of a plain LEZ handler, with the admin as the program's one role.
/// The handler stays as it is under `lee_core::actor_entrypoint!`.
///
/// ```ignore
/// #[lez_admin_authority::admin(config = Config, seed = CONFIG_SEED)]
/// fn handle_message(input: &ReceiveInput, message: Message) -> Response {
///     // the program's own code
/// }
///
/// lee_core::actor_entrypoint!(handle_message);
/// ```
///
/// `config` is the state the program keeps on the config account, with the admin slot marked
/// `#[slot(Admin)]`, and `seed` that account's seed. `genesis = KEY` lets only `KEY` install
/// the first admin, instead of the deployer. A program with a second role uses
/// `lez_authority::guard` and names both.
#[proc_macro_attribute]
pub fn admin(args: TokenStream, item: TokenStream) -> TokenStream {
    admin_attribute(args.into(), item.into(), &krate())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// The path to lez-admin-authority from the program's crate.
fn krate() -> TokenStream2 {
    use proc_macro_crate::{FoundCrate, crate_name};
    match crate_name("lez-admin-authority") {
        Ok(FoundCrate::Name(name)) => {
            let name = format_ident!("{name}");
            quote!(::#name)
        }
        _ => quote!(::lez_admin_authority),
    }
}

fn admin_attribute(
    args: TokenStream2,
    item: TokenStream2,
    krate: &TokenStream2,
) -> syn::Result<TokenStream2> {
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("config") {
            meta.value()?.parse::<Type>()?;
            Ok(())
        } else if meta.path.is_ident("seed") || meta.path.is_ident("genesis") {
            meta.value()?.parse::<Expr>()?;
            Ok(())
        } else if meta.path.is_ident("roles") {
            Err(meta.error(
                "the admin is the role here; a program with more roles uses lez_authority::guard",
            ))
        } else {
            Err(meta.error("expected config = .., seed = .. or genesis = .."))
        }
    });
    syn::parse::Parser::parse2(parser, args.clone())?;
    Ok(quote! {
        #[#krate::lez_authority::guard(roles(#krate::Admin), #args)]
        #item
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(args: &str) -> String {
        admin_attribute(
            args.parse().unwrap(),
            "fn handle(input: &ReceiveInput, message: Message) -> Response { todo!() }"
                .parse()
                .unwrap(),
            &quote!(::lez_admin_authority),
        )
        .expect_err("the attribute refuses these arguments")
        .to_string()
    }

    #[test]
    fn the_admin_attribute_names_its_role_itself() {
        assert_eq!(
            refusal("roles(Admin), config = Config, seed = SEED"),
            "the admin is the role here; a program with more roles uses lez_authority::guard"
        );
        assert_eq!(
            refusal("config = Config, seed = SEED, owner = X"),
            "expected config = .., seed = .. or genesis = .."
        );
    }

    #[test]
    fn it_hands_its_arguments_to_the_guard_with_the_admin_as_the_role() {
        let expanded = admin_attribute(
            "config = Config, seed = SEED".parse().unwrap(),
            "fn handle() {}".parse().unwrap(),
            &quote!(::lez_admin_authority),
        )
        .unwrap()
        .to_string();
        assert!(expanded.contains(
            "lez_authority :: guard (roles (:: lez_admin_authority :: Admin) , config = Config , seed = SEED)"
        ), "{expanded}");
    }
}
