//! Escape sequences that tell the terminal what Psi is doing: the window
//! title, a progress indicator (OSC 9;4) and desktop notifications. Terminals
//! that don't know one ignore it. Inside tmux each is wrapped so tmux hands it
//! to the outer terminal, which needs `set -g allow-passthrough on`.

/// Which desktop-notification sequence the terminal understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    /// `OSC 777;notify;title;body`: ghostty, wezterm, foot, VTE terminals.
    Osc777,
    /// kitty's `OSC 99`, which also takes a title and a body.
    Osc99,
    /// `OSC 9;body`: iTerm2 (ghostty and kitty read it too).
    Osc9,
}

/// What Psi is doing, as the title and progress show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Working,
    /// An approval, a question or a dialog waits for the user.
    Waiting,
}

/// The terminal Psi runs in, as far as the environment tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Term {
    pub notify: NotifyKind,
    /// Inside tmux: sequences for the outer terminal go through passthrough.
    pub tmux: bool,
}

impl Term {
    /// From the environment: `TMUX`, kitty's `KITTY_WINDOW_ID`, iTerm2's
    /// `LC_TERMINAL` or `TERM_PROGRAM`. Inside tmux, `TERM_PROGRAM` names
    /// tmux, but the outer terminal's own variables are usually inherited.
    pub fn detect(env: impl Fn(&str) -> Option<String>) -> Self {
        let is = |k: &str, v: &str| env(k).is_some_and(|x| x == v);
        let notify = if env("KITTY_WINDOW_ID").is_some() {
            NotifyKind::Osc99
        } else if is("LC_TERMINAL", "iTerm2") || is("TERM_PROGRAM", "iTerm.app") {
            NotifyKind::Osc9
        } else {
            NotifyKind::Osc777
        };
        Term {
            notify,
            tmux: env("TMUX").is_some_and(|t| !t.is_empty()),
        }
    }

    /// A desktop notification.
    pub fn notification(&self, title: &str, body: &str) -> Vec<u8> {
        let (title, body) = (clean(title).replace(';', ","), clean(body));
        let seq = match self.notify {
            NotifyKind::Osc777 => format!("\x1b]777;notify;{title};{body}\x1b\\"),
            NotifyKind::Osc99 => format!(
                "\x1b]99;i=psi:d=0:p=title;{title}\x1b\\\x1b]99;i=psi:d=1:p=body;{body}\x1b\\"
            ),
            NotifyKind::Osc9 => format!("\x1b]9;{title}: {body}\x1b\\"),
        };
        self.outer(seq.as_bytes())
    }

    /// OSC 9;4: indeterminate while working, paused while waiting, removed
    /// when idle.
    pub fn progress(&self, a: Activity) -> Vec<u8> {
        let state = match a {
            Activity::Idle => 0,
            Activity::Working => 3,
            Activity::Waiting => 4,
        };
        self.outer(format!("\x1b]9;4;{state}\x1b\\").as_bytes())
    }

    /// The window title (OSC 2). tmux takes it as the pane title itself, so
    /// it isn't wrapped.
    pub fn title(&self, title: &str) -> Vec<u8> {
        format!("\x1b]2;{}\x1b\\", clean(title)).into_bytes()
    }

    /// OSC 52: puts `text` on the system clipboard, through the terminal
    /// (works over SSH too). ghostty and kitty allow writes by default.
    pub fn clipboard(&self, text: &str) -> Vec<u8> {
        self.outer(format!("\x1b]52;c;{}\x1b\\", base64(text.as_bytes())).as_bytes())
    }

    /// Saves the title on the terminal's title stack, to put back on exit.
    pub fn push_title(&self) -> Vec<u8> {
        b"\x1b[22;0t".to_vec()
    }

    pub fn pop_title(&self) -> Vec<u8> {
        b"\x1b[23;0t".to_vec()
    }

    /// A sequence for the terminal outside tmux.
    fn outer(&self, seq: &[u8]) -> Vec<u8> {
        if !self.tmux {
            return seq.to_vec();
        }
        let mut out = b"\x1bPtmux;".to_vec();
        for &b in seq {
            if b == 0x1b {
                out.push(0x1b);
            }
            out.push(b);
        }
        out.extend_from_slice(b"\x1b\\");
        out
    }
}

/// The title for an activity in a directory.
pub fn title_for(dir: &str, a: Activity) -> String {
    match a {
        Activity::Idle => format!("ψ {dir}"),
        Activity::Working => format!("ψ {dir} · working"),
        Activity::Waiting => format!("ψ {dir} · waiting"),
    }
}

/// Standard base64 with padding (OSC 52's encoding).
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Text safe inside an OSC string: no control characters, one line, short.
fn clean(s: &str) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    one.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(vars: &[(&str, &str)]) -> Term {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Term::detect(move |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))
    }

    #[test]
    fn the_terminal_picks_the_notification_kind() {
        assert_eq!(term(&[]).notify, NotifyKind::Osc777);
        assert_eq!(
            term(&[("TERM_PROGRAM", "ghostty")]).notify,
            NotifyKind::Osc777
        );
        assert_eq!(term(&[("KITTY_WINDOW_ID", "1")]).notify, NotifyKind::Osc99);
        assert_eq!(term(&[("LC_TERMINAL", "iTerm2")]).notify, NotifyKind::Osc9);
        assert!(term(&[("TMUX", "/tmp/tmux-1000/default,1,0")]).tmux);
        assert!(!term(&[("TMUX", "")]).tmux);
    }

    #[test]
    fn notifications_are_one_clean_line() {
        let t = term(&[]);
        let n = String::from_utf8(t.notification("psi; x", "done\n\x1b[31mred")).unwrap();
        assert_eq!(n, "\x1b]777;notify;psi, x;done [31mred\x1b\\");
        let k =
            String::from_utf8(term(&[("KITTY_WINDOW_ID", "1")]).notification("a", "b")).unwrap();
        assert_eq!(
            k,
            "\x1b]99;i=psi:d=0:p=title;a\x1b\\\x1b]99;i=psi:d=1:p=body;b\x1b\\"
        );
    }

    #[test]
    fn tmux_passthrough_doubles_escapes() {
        let t = term(&[("TMUX", "/tmp/s,1,0")]);
        assert_eq!(
            t.progress(Activity::Working),
            b"\x1bPtmux;\x1b\x1b]9;4;3\x1b\x1b\\\x1b\\".to_vec()
        );
        // The title goes to tmux itself.
        assert_eq!(t.title("ψ psi"), "\x1b]2;ψ psi\x1b\\".as_bytes().to_vec());
    }

    #[test]
    fn the_clipboard_gets_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(term(&[]).clipboard("hi"), b"\x1b]52;c;aGk=\x1b\\".to_vec());
    }

    #[test]
    fn progress_follows_the_activity() {
        let t = term(&[]);
        assert_eq!(t.progress(Activity::Idle), b"\x1b]9;4;0\x1b\\".to_vec());
        assert_eq!(t.progress(Activity::Waiting), b"\x1b]9;4;4\x1b\\".to_vec());
        assert_eq!(title_for("psi", Activity::Working), "ψ psi · working");
    }
}
