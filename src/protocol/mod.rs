//! Shared wire protocol and presentation encoding code.

pub mod endpoint;
pub(crate) mod render_ansi;
pub(crate) mod surface_delta;
pub(crate) mod surface_reuse;
pub(crate) mod surface_scroll;
pub(crate) mod surface_underline;
mod wire;

pub use wire::*;
