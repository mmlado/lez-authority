//! A program that depends on a role crate alone, renamed in the workspace root.

use inherited_role as role;

include!("../../program.rs");
