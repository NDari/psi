#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code.
//! How real answers render (psi-plan.md §11.2): each `fixtures/markdown/*.md`
//! drawn at 100 columns, compared with the `.ansi` file beside it. `cat` an
//! `.ansi` file in a terminal to see the look; after a deliberate change,
//! `env PSI_BLESS=1 cargo test -p psi-tui --test markdown_snapshots`.

use std::path::Path;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/markdown");

fn ansi(text: &str) -> String {
    let mut out = Vec::new();
    for line in psi_tui::render_text(text, 100) {
        line.write_ansi(&mut out).unwrap();
        out.push(b'\n');
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn answers_render_as_their_snapshots() {
    let mut checked = 0;
    let mut stale = Vec::new();
    for e in std::fs::read_dir(DIR).unwrap().flatten() {
        let md = e.path();
        if md.extension().is_none_or(|x| x != "md") {
            continue;
        }
        let got = ansi(&std::fs::read_to_string(&md).unwrap());
        let snap = md.with_extension("ansi");
        if std::env::var_os("PSI_BLESS").is_some() {
            std::fs::write(&snap, &got).unwrap();
        } else if std::fs::read_to_string(&snap).unwrap_or_default() != got {
            stale.push(name(&md));
        }
        checked += 1;
    }
    assert!(checked >= 4, "only {checked} fixtures in {DIR}");
    assert!(
        stale.is_empty(),
        "{stale:?} render differently from their .ansi snapshots; if that is \
         meant, run `env PSI_BLESS=1 cargo test -p psi-tui --test markdown_snapshots`"
    );
}

fn name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}
