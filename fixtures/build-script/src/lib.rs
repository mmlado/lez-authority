//! A lib that derives through a role crate, while the package's build script derives through
//! the engine under another name.

use lez_sample_authority::Gated;

#[derive(Gated)]
pub enum Message {
    #[from_config]
    Credit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lib_reaches_the_engine_through_its_role_crate() {
        assert!(Message::Credit.only_from_config());
    }
}
