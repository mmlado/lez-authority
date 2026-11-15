//! Freeze authority for LEZ actor programs: the freeze role of lez-authority, answering to the
//! admin of lez-admin-authority. A program depends on this crate alone, which re-exports the
//! admin and the engine.

pub use lez_admin_authority;
pub use lez_admin_authority::lez_authority;
