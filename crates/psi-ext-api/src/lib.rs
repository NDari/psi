//! The Lua extension API (psi-plan.md §9.5, D-14). `api/*.toml` defines every
//! function, event and type once; build.rs generates the [`ExtHost`] trait, the
//! [`register`] glue, the signature tables, the LuaLS stubs and the reference.

// Which checks the generated glue uses depends on the API definition.
#[allow(dead_code)]
mod glue;
pub mod types;

include!(concat!(env!("OUT_DIR"), "/ext_api.rs"));

/// LuaLS stubs (`---@meta`), checked in as `docs/api/psi.lua`.
pub const LUALS_STUBS: &str = include_str!(concat!(env!("OUT_DIR"), "/psi.lua"));

/// The markdown reference, checked in as `docs/api.md`.
pub const API_MARKDOWN: &str = include_str!(concat!(env!("OUT_DIR"), "/api.md"));

/// A function's signature, as written in the API definition.
#[derive(Debug)]
pub struct FnSig {
    /// `ui.confirm` for `psi.ui.confirm`.
    pub name: &'static str,
    pub doc: &'static str,
    pub params: &'static [ParamSig],
    pub returns: Option<&'static str>,
    pub is_async: bool,
}

#[derive(Debug)]
pub struct ParamSig {
    pub name: &'static str,
    /// The type string, like `string?` (see [`types::parse`]).
    pub ty: &'static str,
    /// Optional params are trailing and may be omitted.
    pub optional: bool,
}

#[derive(Debug)]
pub struct EventSig {
    pub name: &'static str,
    /// `notify`, `chain` or `gate`.
    pub kind: &'static str,
    pub doc: &'static str,
    pub payload: &'static str,
    pub result: Option<&'static str>,
}

#[derive(Debug)]
pub struct TypeSig {
    pub name: &'static str,
    pub doc: &'static str,
    pub fields: &'static [FieldSig],
    /// `events` for the type that is any event name.
    pub alias: Option<&'static str>,
}

#[derive(Debug)]
pub struct FieldSig {
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static str,
}
