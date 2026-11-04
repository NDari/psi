//! Shows a markdown file as Psi draws an answer: `just md-preview <file>`
//! (stdin when no file is given), at the terminal's width or `--width N`.

use std::io::{Read, Write};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut width = crossterm::terminal::size().map_or(100, |(w, _)| w as usize);
    let mut path = None;
    while let Some(a) = args.next() {
        if a == "--width" {
            width = args.next().and_then(|w| w.parse().ok()).unwrap_or(width);
        } else {
            path = Some(a);
        }
    }
    let text = match &path {
        Some(p) => std::fs::read_to_string(p).unwrap_or_else(|e| {
            eprintln!("md-preview: {p}: {e}");
            std::process::exit(1)
        }),
        None => {
            let mut s = String::new();
            let _ = std::io::stdin().read_to_string(&mut s);
            s
        }
    };
    let mut out = std::io::stdout().lock();
    for line in psi_tui::render_text(&text, width) {
        let _ = line.write_ansi(&mut out);
        let _ = out.write_all(b"\n");
    }
}
