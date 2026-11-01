//! Keybindings (psi-plan.md §11.4, pi parity): actions with pi's ids, each
//! bound to one or more keys like `ctrl+a` or `alt+enter`. A user file maps
//! ids to a key or a list of keys; a list replaces the defaults, `[]`
//! unbinds.

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

/// The actions Psi has, pi's id first. Pi's other ids are accepted and do
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    CursorWordLeft,
    CursorWordRight,
    CursorLineStart,
    CursorLineEnd,
    DeleteCharBackward,
    DeleteCharForward,
    DeleteWordBackward,
    DeleteToLineStart,
    DeleteToLineEnd,
    NewLine,
    Submit,
    Interrupt,
    Clear,
    Exit,
    FollowUp,
    PlanToggle,
}

const ALL: &[(Action, &str, &[&str], &str)] = &[
    (
        Action::CursorUp,
        "tui.editor.cursorUp",
        &["up"],
        "cursor up; at the top, older prompts",
    ),
    (
        Action::CursorDown,
        "tui.editor.cursorDown",
        &["down"],
        "cursor down; at the bottom, newer prompts",
    ),
    (
        Action::CursorLeft,
        "tui.editor.cursorLeft",
        &["left", "ctrl+b"],
        "cursor left",
    ),
    (
        Action::CursorRight,
        "tui.editor.cursorRight",
        &["right", "ctrl+f"],
        "cursor right",
    ),
    (
        Action::CursorWordLeft,
        "tui.editor.cursorWordLeft",
        &["alt+left", "ctrl+left", "alt+b"],
        "a word left",
    ),
    (
        Action::CursorWordRight,
        "tui.editor.cursorWordRight",
        &["alt+right", "ctrl+right", "alt+f"],
        "a word right",
    ),
    (
        Action::CursorLineStart,
        "tui.editor.cursorLineStart",
        &["home", "ctrl+a"],
        "start of the line",
    ),
    (
        Action::CursorLineEnd,
        "tui.editor.cursorLineEnd",
        &["end", "ctrl+e"],
        "end of the line",
    ),
    (
        Action::DeleteCharBackward,
        "tui.editor.deleteCharBackward",
        &["backspace"],
        "delete the character before",
    ),
    (
        Action::DeleteCharForward,
        "tui.editor.deleteCharForward",
        &["delete", "ctrl+d"],
        "delete the character after",
    ),
    (
        Action::DeleteWordBackward,
        "tui.editor.deleteWordBackward",
        &["ctrl+w", "alt+backspace", "ctrl+backspace"],
        "delete the word before",
    ),
    (
        Action::DeleteToLineStart,
        "tui.editor.deleteToLineStart",
        &["ctrl+u"],
        "delete to the start of the line",
    ),
    (
        Action::DeleteToLineEnd,
        "tui.editor.deleteToLineEnd",
        &["ctrl+k"],
        "delete to the end of the line",
    ),
    (
        Action::NewLine,
        "tui.input.newLine",
        &["shift+enter", "ctrl+j"],
        "new line",
    ),
    (
        Action::Submit,
        "tui.input.submit",
        &["enter"],
        "send; while running, steer the run",
    ),
    (
        Action::Interrupt,
        "app.interrupt",
        &["escape"],
        "abort the run, close a list",
    ),
    (
        Action::Clear,
        "app.clear",
        &["ctrl+c"],
        "clear the input; twice to quit",
    ),
    (
        Action::Exit,
        "app.exit",
        &["ctrl+d"],
        "quit, when the input is empty",
    ),
    (
        Action::FollowUp,
        "app.message.followUp",
        &["alt+enter"],
        "queue a follow-up for when the run ends",
    ),
    (
        Action::PlanToggle,
        "app.plan.toggle",
        &["ctrl+alt+p"],
        "plan mode on or off",
    ),
];

/// One key: modifiers and a base key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    mods: KeyModifiers,
    code: KeyCode,
}

impl Key {
    /// Parses `ctrl+shift+x`: modifiers ctrl, shift, alt and super, each
    /// at most once, then one base key.
    pub fn parse(s: &str) -> Result<Key, String> {
        let lower = s.trim().to_ascii_lowercase();
        let mut parts: Vec<&str> = lower.split('+').collect();
        // "ctrl++" is ctrl and the plus key.
        if lower.ends_with("++") {
            parts.pop();
            parts.pop();
            parts.push("+");
        }
        let Some(base) = parts.pop().filter(|b| !b.is_empty()) else {
            return Err(format!("'{s}' has no key"));
        };
        let mut mods = KeyModifiers::NONE;
        for m in parts {
            let bit = match m {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                "super" => KeyModifiers::SUPER,
                _ => return Err(format!("'{s}': {m} is not ctrl, alt, shift or super")),
            };
            if mods.contains(bit) {
                return Err(format!("'{s}' repeats {m}"));
            }
            mods |= bit;
        }
        let code = match base {
            "escape" | "esc" => KeyCode::Esc,
            "enter" | "return" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "space" => KeyCode::Char(' '),
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            f if f.starts_with('f') && f.len() > 1 && f[1..].parse::<u8>().is_ok() => {
                match f[1..].parse::<u8>() {
                    Ok(n @ 1..=12) => KeyCode::F(n),
                    _ => return Err(format!("'{s}': {f} is not f1 to f12")),
                }
            }
            c if c.chars().count() == 1 => KeyCode::Char(c.chars().next().unwrap_or(' ')),
            other => return Err(format!("'{s}': {other} is not a key")),
        };
        Ok(Key { mods, code })
    }

    pub fn matches(&self, ev: &KeyEvent) -> bool {
        let mut mods = ev.modifiers & !(KeyModifiers::HYPER | KeyModifiers::META);
        let mut code = ev.code;
        if let KeyCode::Char(c) = code {
            // Terminals send Shift+a as 'A'; a spec without shift matches
            // the character however it was typed.
            if c.is_ascii_uppercase() {
                code = KeyCode::Char(c.to_ascii_lowercase());
                mods |= KeyModifiers::SHIFT;
            }
            if !self.mods.contains(KeyModifiers::SHIFT) {
                mods.remove(KeyModifiers::SHIFT);
            }
        }
        if code == KeyCode::BackTab {
            code = KeyCode::Tab;
            mods |= KeyModifiers::SHIFT;
        }
        code == self.code && mods == self.mods
    }
}

/// Which keys do what.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: BTreeMap<Action, Vec<Key>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let bindings = ALL
            .iter()
            .map(|(a, _, keys, _)| {
                let keys = keys.iter().filter_map(|k| Key::parse(k).ok()).collect();
                (*a, keys)
            })
            .collect();
        Self { bindings }
    }
}

impl Keymap {
    /// The defaults with a user file's bindings on top. Returns the
    /// problems found too; a bad entry keeps that action's defaults.
    pub fn from_json(v: &Value) -> (Self, Vec<String>) {
        let mut km = Keymap::default();
        let mut problems = Vec::new();
        let Some(obj) = v.as_object() else {
            problems.push("keybindings.json must be an object of action ids".into());
            return (km, problems);
        };
        for (id, val) in obj {
            if id == "$schema" {
                continue;
            }
            let Some((action, ..)) = ALL.iter().find(|(_, i, _, _)| i == id) else {
                if !id.starts_with("tui.") && !id.starts_with("app.") {
                    problems.push(format!("keybindings.json: unknown action '{id}'"));
                }
                continue;
            };
            let specs: Vec<&str> = match val {
                Value::String(s) => vec![s.as_str()],
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                _ => {
                    problems.push(format!(
                        "keybindings.json: {id} needs a key or a list of keys"
                    ));
                    continue;
                }
            };
            let parsed: Result<Vec<Key>, String> = specs.iter().map(|s| Key::parse(s)).collect();
            match parsed {
                Ok(mut keys) => {
                    keys.dedup();
                    km.bindings.insert(*action, keys);
                }
                Err(e) => problems.push(format!("keybindings.json: {id}: {e}")),
            }
        }
        (km, problems)
    }

    pub fn is(&self, action: Action, ev: &KeyEvent) -> bool {
        self.bindings
            .get(&action)
            .is_some_and(|keys| keys.iter().any(|k| k.matches(ev)))
    }

    /// For /help: id, keys and what it does.
    pub fn describe(&self) -> Vec<(String, String)> {
        ALL.iter()
            .map(|(a, _, _, what)| {
                let keys: Vec<String> = self
                    .bindings
                    .get(a)
                    .into_iter()
                    .flatten()
                    .map(key_label)
                    .collect();
                let keys = if keys.is_empty() {
                    "(unbound)".to_owned()
                } else {
                    keys.join(", ")
                };
                (keys, (*what).to_owned())
            })
            .collect()
    }

    /// The first key for an action, for hints ("Esc aborts").
    pub fn first(&self, action: Action) -> Option<String> {
        self.bindings.get(&action)?.first().map(key_label)
    }
}

fn key_label(k: &Key) -> String {
    let mut s = String::new();
    for (bit, name) in [
        (KeyModifiers::CONTROL, "Ctrl+"),
        (KeyModifiers::ALT, "Alt+"),
        (KeyModifiers::SHIFT, "Shift+"),
        (KeyModifiers::SUPER, "Super+"),
    ] {
        if k.mods.contains(bit) {
            s.push_str(name);
        }
    }
    let base = match k.code {
        KeyCode::Char(' ') => "Space".to_owned(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Esc => "Esc".into(),
        other => format!("{other:?}"),
    };
    s.push_str(&base);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn parse_and_match() {
        let k = Key::parse("ctrl+alt+p").unwrap();
        assert!(k.matches(&ev(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL | KeyModifiers::ALT
        )));
        assert!(!k.matches(&ev(KeyCode::Char('p'), KeyModifiers::CONTROL)));
        let k = Key::parse("shift+enter").unwrap();
        assert!(k.matches(&ev(KeyCode::Enter, KeyModifiers::SHIFT)));
        assert!(!k.matches(&ev(KeyCode::Enter, KeyModifiers::NONE)));
        // Typed characters match without their shift.
        let k = Key::parse("?").unwrap();
        assert!(k.matches(&ev(KeyCode::Char('?'), KeyModifiers::SHIFT)));
        assert!(
            Key::parse("ctrl+shift+x")
                .unwrap()
                .matches(&ev(KeyCode::Char('X'), KeyModifiers::CONTROL))
        );
        assert_eq!(Key::parse("PageUp").unwrap().code, KeyCode::PageUp);
        assert!(Key::parse("ctrl+ctrl+a").is_err());
        assert!(Key::parse("hyper+a").is_err());
        assert!(Key::parse("f13").is_err());
        assert!(Key::parse("ctrl+").is_err());
    }

    #[test]
    fn user_file_replaces_defaults() {
        let (km, problems) = Keymap::from_json(&json!({
            "$schema": "x",
            "app.interrupt": ["ctrl+g", "escape"],
            "tui.editor.cursorLeft": [],
            "app.model.select": "ctrl+l",
            "app.exit": "ctrl+ctrl+q",
            "nonsense": "a",
        }));
        assert!(km.is(
            Action::Interrupt,
            &ev(KeyCode::Char('g'), KeyModifiers::CONTROL)
        ));
        assert!(km.is(Action::Interrupt, &ev(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(!km.is(Action::CursorLeft, &ev(KeyCode::Left, KeyModifiers::NONE)));
        // A bad entry keeps the defaults.
        assert!(km.is(Action::Exit, &ev(KeyCode::Char('d'), KeyModifiers::CONTROL)));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("app.exit"));
        assert!(problems[1].contains("unknown action 'nonsense'"));
        assert_eq!(km.first(Action::Interrupt).as_deref(), Some("Ctrl+g"));
    }
}
