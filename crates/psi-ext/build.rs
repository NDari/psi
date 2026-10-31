//! Links native/compat.c whole, so its definitions win over libm's (see
//! the comment there).

fn main() {
    println!("cargo:rerun-if-changed=native/compat.c");
    cc::Build::new()
        .file("native/compat.c")
        .cargo_metadata(false)
        .compile("psi_compat");
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static:+whole-archive=psi_compat");
}
