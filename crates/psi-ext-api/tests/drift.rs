#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! The checked-in LuaLS stubs and reference match the API definition (D-33).

use std::path::Path;
use std::{env, fs};

/// Where the checked-in docs live. In the repo: the root `docs/`, two levels up.
const DOCS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs");

fn check(rel: &str, generated: &str) {
    let path = Path::new(DOCS_DIR).join(rel);
    if env::var_os("PSI_BLESS").is_some() {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(&path, generated).unwrap();
        return;
    }
    let on_disk = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == generated,
        "docs/{rel} is out of date with api/*.toml; regenerate it with \
         `env PSI_BLESS=1 cargo test -p psi-ext-api`"
    );
}

#[test]
fn docs_match_the_api() {
    check("api/psi.lua", psi_ext_api::LUALS_STUBS);
    check("api.md", psi_ext_api::API_MARKDOWN);
}
