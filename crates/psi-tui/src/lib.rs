//! The interactive terminal UI: renderer, components and editor (psi-plan.md §11).
//!
//! Inline, no alternate screen: finished output is committed once above a live
//! region that is redrawn differentially at the bottom of the terminal.

pub mod editor;
pub mod markdown;
pub mod render;
pub mod style;

pub use editor::{Editor, EditorEvent};
pub use markdown::{MdStream, render_block};
pub use render::Renderer;
pub use style::{Color, Line, Span, Style};
