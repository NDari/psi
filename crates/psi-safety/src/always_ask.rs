//! Always-ask detection (psi-plan.md §8.3, D-21): finds always-ask commands
//! anywhere inside a bash command, so the harness can ask before running it.
//! It doesn't decide that anything is safe; the boundary and checkpoints do that.

use serde::Deserialize;
use tree_sitter::{Node, Parser};

/// The shipped list. Repoint this path if the module moves.
const SHIPPED: &str = include_str!("../../../rules/always-ask.jsonc");

/// How many `sh -c` / `eval` scripts deep the classifier looks.
const MAX_DEPTH: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("always-ask entry is empty")]
    Empty,
    #[error("always-ask rules: {0}")]
    File(String),
}

/// One always-ask entry, e.g. "git push": a program and words that must all appear after it, in any order.
/// For git, a first word that isn't an option must be the subcommand, so `git commit -m clean`
/// isn't `git clean`.
/// A word written `!list` must not appear: "git stash !list" asks for `git stash` but not `git stash list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    program: String,
    words: Vec<String>,
    not: Vec<String>,
}

impl Rule {
    fn matches(&self, argv: &[Word]) -> bool {
        let Some((prog, args)) = argv.split_first() else {
            return false;
        };
        let has = |want: &str| {
            args.iter()
                .filter_map(|a| a.lit.as_deref())
                .any(|got| word_matches(want, got))
        };
        prog.lit.as_deref().map(basename) == Some(self.program.as_str())
            && self.subcommand_matches(args)
            && self.words.iter().all(|w| has(w))
            && !self.not.iter().any(|w| has(w))
    }

    /// A git entry's first word, unless it is an option, is git's subcommand: the
    /// first word after git's own options (`git -C DIR -c k=v push`). Other programs'
    /// global options vary too much to find theirs, and their words match anywhere.
    fn subcommand_matches(&self, args: &[Word]) -> bool {
        match self.words.first() {
            Some(sub) if self.program == "git" && !sub.starts_with('-') => {
                let long = [
                    "--git-dir",
                    "--work-tree",
                    "--namespace",
                    "--super-prefix",
                    "--config-env",
                    "--attr-source",
                ];
                let i = options(args, "Cc", &long).0;
                args.get(i).and_then(|w| w.lit.as_deref()) == Some(sub.as_str())
            }
            _ => true,
        }
    }
}

/// An entry word matches itself; `-D` also matches a short-flag cluster like `-fD`,
/// and `--print` also matches `--print=x`.
fn word_matches(want: &str, got: &str) -> bool {
    if want == got {
        return true;
    }
    if want.starts_with("--") {
        return got
            .strip_prefix(want)
            .is_some_and(|rest| rest.starts_with('='));
    }
    match want.strip_prefix('-') {
        Some(flag) if flag.chars().count() == 1 => {
            got.starts_with('-') && !got.starts_with("--") && got[1..].contains(flag)
        }
        _ => false,
    }
}

/// The always-ask list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    rules: Vec<Rule>,
    /// The built-in [`GIT_DIR_RULE`]; off only when settings remove it.
    git_dir: bool,
}

/// The built-in entry: removing or moving the repository's `.git` in any
/// shape (`rm -rf .git`, `./.git/`, an absolute or quoted path, a path
/// inside it, a dot-glob like `.*`, `find ... -delete`, `mv .git`).
/// Checkpoints leave `.git` alone, so nothing could undo it; Gemma 12B
/// rebuilt a task's repository with `rm -rf .git` again and again
/// (2026-10-31). Exact-word rules can't express it.
pub const GIT_DIR_RULE: &str = "rm .git";

/// `.git` as a path component of `s`: alone, as the last component, or
/// with more after it (`.git/objects`); not `.gitignore` or `.github`.
fn touches_git_dir(s: &str) -> bool {
    let b = s.as_bytes();
    let mut from = 0;
    while let Some(i) = s[from..].find(".git") {
        let i = from + i;
        let before = i == 0 || matches!(b[i - 1], b'/' | b'"' | b'\'' | b'=' | b' ');
        let after = b
            .get(i + 4)
            .is_none_or(|c| matches!(c, b'/' | b'"' | b'\'' | b'*' | b' '));
        if before && after {
            return true;
        }
        from = i + 4;
    }
    false
}

/// A glob over dot names (`.*`, `.g?t`) that could match `.git`.
fn dot_glob(s: &str) -> bool {
    let last = s.trim_end_matches('/').rsplit('/').next().unwrap_or(s);
    last.starts_with('.') && last.contains(['*', '?', '['])
}

/// Whether one simple command removes or moves `.git`.
fn removes_git_dir(argv: &[Word]) -> bool {
    let Some((prog, args)) = argv.split_first() else {
        return false;
    };
    let Some(name) = prog.lit.as_deref().map(basename) else {
        return false;
    };
    let hits = |w: &Word| {
        touches_git_dir(&w.raw)
            || w.lit
                .as_deref()
                .is_some_and(|l| touches_git_dir(l) || dot_glob(l))
    };
    match name {
        "rm" | "rmdir" | "unlink" | "shred" | "srm" | "trash" | "trash-put" | "gio" | "mv" => {
            args.iter().any(hits)
        }
        "find" => {
            let acts = args
                .iter()
                .filter_map(|w| w.lit.as_deref())
                .any(|a| matches!(a, "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir"));
            acts && args.iter().any(hits)
        }
        _ => false,
    }
}

#[derive(Deserialize)]
struct RulesFile {
    always_ask: Vec<String>,
}

impl Rules {
    /// The shipped list (rules/always-ask.jsonc, embedded with include_str!).
    pub fn shipped() -> Self {
        // Invariant: the embedded file is fixed at build time and the
        // `shipped_list` test parses it.
        #[allow(clippy::expect_used)]
        Self::from_jsonc(SHIPPED).expect("the shipped always-ask list parses")
    }

    /// Parse a rules file: `{ "always_ask": ["git push", ...] }`, comments allowed.
    pub fn from_jsonc(text: &str) -> Result<Self, RuleError> {
        let file: RulesFile = jsonc_parser::parse_to_serde_value(text, &Default::default())
            .map_err(|e| RuleError::File(e.to_string()))?;
        let mut rules = Self {
            rules: Vec::new(),
            git_dir: true,
        };
        for entry in &file.always_ask {
            rules.push(Self::parse_entry(entry)?);
        }
        Ok(rules)
    }

    /// Parse one entry string like "git branch -D" (program then words). Error on empty.
    /// Words written `!word` must not appear; the id leaves them out.
    pub fn parse_entry(s: &str) -> Result<Rule, RuleError> {
        let (not, parts): (Vec<&str>, Vec<&str>) =
            s.split_whitespace().partition(|w| w.starts_with('!'));
        let (program, words) = parts.split_first().ok_or(RuleError::Empty)?;
        Ok(Rule {
            id: parts.join(" "),
            program: basename(program).to_string(),
            words: words.iter().map(|w| w.to_string()).collect(),
            not: not.iter().map(|w| w[1..].to_string()).collect(),
        })
    }

    /// Layers (psi-plan.md §8.3): `add` appends entries (dedupe by id), `remove` drops
    /// shipped entries by id. Error on an unparseable entry.
    pub fn layered(add: &[String], remove: &[String]) -> Result<Self, RuleError> {
        let mut rules = Self::shipped();
        for r in remove {
            Self::parse_entry(r)?;
        }
        rules
            .rules
            .retain(|rule| !remove.iter().any(|r| Self::names(&rule.id, r)));
        if remove.iter().any(|r| Self::names(GIT_DIR_RULE, r)) {
            rules.git_dir = false;
        }
        for entry in add {
            rules.push(Self::parse_entry(entry)?);
        }
        Ok(rules)
    }

    fn push(&mut self, rule: Rule) {
        if !self.rules.iter().any(|r| r.id == rule.id) {
            self.rules.push(rule);
        }
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.rules
            .iter()
            .map(|r| r.id.as_str())
            .chain(self.git_dir.then_some(GIT_DIR_RULE))
    }

    /// Does a user flag like "git-push" or "git push" name this rule id? (hyphens and spaces equivalent)
    pub fn names(id: &str, flag: &str) -> bool {
        let parts = |s: &str| -> Vec<String> {
            s.split(|c: char| c == '-' || c.is_whitespace())
                .filter(|p| !p.is_empty())
                .map(String::from)
                .collect()
        };
        parts(id) == parts(flag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No always-ask entry found.
    Clear,
    /// The matched rule ids, deduplicated, in first-match order.
    Ask(Vec<String>),
    /// The command doesn't parse; bash runs lines before a syntax error, so this asks.
    Unparseable(String),
}

/// Find always-ask entries anywhere in `command`.
pub fn classify(command: &str, rules: &Rules) -> Verdict {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .is_err()
    {
        return Verdict::Unparseable("the bash parser failed to load".into());
    }
    let mut scan = Scan {
        parser,
        rules,
        hits: Vec::new(),
    };
    match scan.script(command, 0, None) {
        Err(why) => Verdict::Unparseable(why),
        Ok(()) if scan.hits.is_empty() => Verdict::Clear,
        Ok(()) => Verdict::Ask(scan.hits),
    }
}

/// A command word: its unquoted value when it is a literal, and its source text.
#[derive(Debug, Clone)]
pub(crate) struct Word {
    pub(crate) lit: Option<String>,
    pub(crate) raw: String,
}

impl Word {
    fn literal(s: &str) -> Self {
        Self {
            lit: Some(s.to_string()),
            raw: s.to_string(),
        }
    }
}

struct Scan<'r> {
    parser: Parser,
    rules: &'r Rules,
    hits: Vec<String>,
}

impl Scan<'_> {
    /// Parse `src` and check every simple command in it. `via` names the program that runs a nested script.
    fn script(&mut self, src: &str, depth: usize, via: Option<&str>) -> Result<(), String> {
        let what = via.map_or("it".to_string(), |p| format!("the script given to {p}"));
        let Some(tree) = self.parser.parse(src, None) else {
            return Err(format!("bash could not parse {what}"));
        };
        if let Some(line) = error_line(tree.root_node()) {
            return Err(format!("bash could not parse {what} near line {line}"));
        }
        let mut commands = Vec::new();
        collect(tree.root_node(), src, &mut commands);
        for argv in &commands {
            self.command(argv, depth)?;
        }
        Ok(())
    }

    /// Check one simple command, then whatever it runs.
    fn command(&mut self, argv: &[Word], depth: usize) -> Result<(), String> {
        let rules = self.rules;
        if rules.git_dir && removes_git_dir(argv) && !self.hits.iter().any(|h| h == GIT_DIR_RULE) {
            self.hits.push(GIT_DIR_RULE.to_owned());
        }
        for rule in &rules.rules {
            if rule.matches(argv) && !self.hits.contains(&rule.id) {
                self.hits.push(rule.id.clone());
            }
        }
        let Some((prog, args)) = argv.split_first() else {
            return Ok(());
        };
        let Some(prog) = prog.lit.as_deref().map(basename) else {
            return Ok(());
        };
        for inner in runs(prog, args) {
            match inner {
                Inner::Command(words) => self.command(&words, depth)?,
                Inner::Script(_) if depth >= MAX_DEPTH => {
                    return Err(format!("scripts nested more than {MAX_DEPTH} deep"));
                }
                Inner::Script(s) => self.script(&s, depth + 1, Some(prog))?,
            }
        }
        Ok(())
    }
}

/// The line of the first ERROR or MISSING node, if any.
pub(crate) fn error_line(node: Node) -> Option<usize> {
    if node.is_missing() {
        return Some(node.start_position().row + 1);
    }
    if node.is_error() {
        // An ERROR node can wrap the valid commands before the bad token;
        // point at the first piece that isn't one.
        let bad = node
            .children(&mut node.walk())
            .find(|c| !c.is_named() || c.has_error());
        return match bad {
            Some(c) if c.has_error() => error_line(c),
            Some(c) => Some(c.start_position().row + 1),
            None => Some(node.start_position().row + 1),
        };
    }
    if !node.has_error() {
        return None;
    }
    let found = node.children(&mut node.walk()).find_map(error_line);
    found.or(Some(node.start_position().row + 1))
}

/// Every simple command in the tree, in source order. Walking every node
/// reaches lists, pipelines, groups, bodies and substitutions alike.
pub(crate) fn collect(node: Node, src: &str, out: &mut Vec<Vec<Word>>) {
    if node.kind() == "command"
        && let Some(name) = node.child_by_field_name("name")
    {
        let mut argv = vec![word(name, src)];
        let mut cursor = node.walk();
        argv.extend(
            node.children_by_field_name("argument", &mut cursor)
                .map(|a| word(a, src)),
        );
        out.push(argv);
    }
    for child in node.children(&mut node.walk()) {
        collect(child, src, out);
    }
}

fn word(node: Node, src: &str) -> Word {
    Word {
        lit: literal(node, src),
        raw: src[node.byte_range()].to_string(),
    }
}

/// The value of a word made only of literal text and quotes, or None if it expands anything.
fn literal(node: Node, src: &str) -> Option<String> {
    let text = &src[node.byte_range()];
    match node.kind() {
        "word" | "number" => Some(unescape(text, false)),
        "raw_string" => Some(text.strip_prefix('\'')?.strip_suffix('\'')?.to_string()),
        "string" => node
            .named_children(&mut node.walk())
            .map(|c| (c.kind() == "string_content").then(|| unescape(&src[c.byte_range()], true)))
            .collect(),
        "command_name" | "concatenation" => node
            .children(&mut node.walk())
            .map(|c| literal(c, src))
            .collect(),
        _ => None,
    }
}

/// Remove backslash escapes. Inside double quotes only `$`, `` ` ``, `"`, `\` and newline are escapable.
fn unescape(s: &str, quoted: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\n') => {}
            Some(n) if !quoted || matches!(n, '$' | '`' | '"' | '\\') => out.push(n),
            Some(n) => {
                out.push('\\');
                out.push(n);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The script a word gives `sh -c` or `eval`: its value, or for one that
/// expands something (`"cd $d && git push"`) its text with the quotes
/// removed and the expansions left as written, so the literal commands in
/// it are still checked; the expansions stay expansions.
pub(crate) fn script_text(w: &Word) -> String {
    if let Some(lit) = &w.lit {
        return lit.clone();
    }
    let mut out = String::with_capacity(w.raw.len());
    let mut chars = w.raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => out.extend(chars.by_ref().take_while(|&c| c != '\'')),
            '"' => {
                let mut inner = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            inner.push('\\');
                            inner.extend(chars.next());
                        }
                        c => inner.push(c),
                    }
                }
                out.push_str(&unescape(&inner, true));
            }
            '\\' => match chars.next() {
                Some('\n') | None => {}
                Some(n) => out.push(n),
            },
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// What a wrapper or runner runs.
pub(crate) enum Inner {
    Command(Vec<Word>),
    Script(String),
}

/// The commands or scripts `prog args` runs: `timeout 5 git push` runs `git push`.
pub(crate) fn runs(prog: &str, args: &[Word]) -> Vec<Inner> {
    let rest = |i: usize| args.get(i..).unwrap_or_default();
    let cmd = |words: &[Word]| match words {
        [] => vec![],
        _ => vec![Inner::Command(words.to_vec())],
    };
    match prog {
        "timeout" => cmd(rest(
            options(args, "ks", &["--kill-after", "--signal"]).0 + 1,
        )),
        "nice" => cmd(rest(options(args, "n", &["--adjustment"]).0)),
        "nohup" | "builtin" => cmd(rest(options(args, "", &[]).0)),
        // bash's `time -p`, and /usr/bin/time's -f and -o.
        "time" => cmd(rest(options(args, "fo", &["--format", "--output"]).0)),
        "exec" => cmd(rest(options(args, "a", &[]).0)),
        "command" => {
            let (i, opts) = options(args, "", &[]);
            if opts.iter().any(|(o, _)| o == "v" || o == "V") {
                vec![] // a lookup, not a run
            } else {
                cmd(rest(i))
            }
        }
        "env" => {
            let (i, opts) = options(args, "uCS", &["--unset", "--chdir", "--split-string"]);
            let mut words: Vec<Word> = opts
                .iter()
                .filter(|(o, _)| o == "S" || o == "--split-string")
                .filter_map(|(_, v)| v.as_deref())
                .flat_map(str::split_whitespace)
                .map(Word::literal)
                .collect();
            words.extend_from_slice(rest(i));
            cmd(skip_assignments(&words))
        }
        "sudo" => {
            let long = [
                "--chdir",
                "--chroot",
                "--close-from",
                "--command-timeout",
                "--group",
                "--host",
                "--other-user",
                "--prompt",
                "--role",
                "--type",
                "--user",
            ];
            cmd(skip_assignments(rest(options(args, "CDgprRtTuU", &long).0)))
        }
        "doas" => cmd(rest(options(args, "uC", &[]).0)),
        "xargs" => {
            let long = [
                "--arg-file",
                "--delimiter",
                "--max-args",
                "--max-chars",
                "--max-procs",
                "--process-slot-var",
            ];
            cmd(rest(options(args, "adEILnPs", &long).0))
        }
        "sh" | "bash" | "zsh" | "dash" => {
            let (i, opts) = options(args, "oO", &["--rcfile", "--init-file"]);
            match args.get(i).map(script_text) {
                Some(script) if opts.iter().any(|(o, _)| o == "c") => vec![Inner::Script(script)],
                _ => vec![],
            }
        }
        "eval" => {
            let args = match args.first().and_then(|w| w.lit.as_deref()) {
                Some("--") => &args[1..],
                _ => args,
            };
            let words: Vec<String> = args.iter().map(script_text).collect();
            vec![Inner::Script(words.join(" "))]
        }
        "find" => find_exec(args),
        _ => vec![],
    }
}

/// Skip leading options (`-x`, clusters like `-lc`, `+o`, `--long`, `--long=v`, up to `--`).
/// `short` and `long` list the options that take a value. Returns the index of the
/// first operand and each option seen with its value. A word that expands but starts
/// with `-` (`--work-tree=$PWD`, `-C"$d"`) is still an option; its value is `None`.
pub(crate) fn options(
    args: &[Word],
    short: &str,
    long: &[&str],
) -> (usize, Vec<(String, Option<String>)>) {
    let mut seen = Vec::new();
    let mut i = 0;
    while let Some(w) = args.get(i) {
        let (arg, literal) = match &w.lit {
            Some(l) => (l.as_str(), true),
            None if w.raw.starts_with('-') => (w.raw.as_str(), false),
            None => break,
        };
        let next = |i: usize| args.get(i + 1).and_then(|w| w.lit.clone());
        if literal && arg == "--" {
            i += 1;
            break;
        } else if let Some(name) = arg.strip_prefix("--") {
            match name.split_once('=') {
                Some((name, value)) => {
                    seen.push((format!("--{name}"), literal.then(|| value.to_string())));
                }
                None if long.contains(&arg) => {
                    seen.push((arg.to_string(), next(i)));
                    i += 1;
                }
                None => seen.push((arg.to_string(), None)),
            }
        } else if arg.len() > 1 && (arg.starts_with('-') || arg.starts_with('+')) {
            let cluster = &arg[1..];
            for (k, c) in cluster.char_indices() {
                // Where a quote or an expansion starts, the letters end.
                if !literal && !c.is_ascii_alphanumeric() {
                    break;
                }
                if short.contains(c) {
                    let attached = &cluster[k + c.len_utf8()..];
                    let value = if attached.is_empty() {
                        i += 1;
                        args.get(i).and_then(|w| w.lit.clone())
                    } else {
                        literal.then(|| attached.to_string())
                    };
                    seen.push((c.to_string(), value));
                    break;
                }
                seen.push((c.to_string(), None));
            }
        } else {
            break;
        }
        i += 1;
    }
    (i.min(args.len()), seen)
}

/// Drop leading `NAME=value` words (`env FOO=1 git push`).
fn skip_assignments(args: &[Word]) -> &[Word] {
    let is_assignment = |w: &Word| {
        let text = w.lit.as_deref().unwrap_or(&w.raw);
        text.split_once('=').is_some_and(|(name, _)| {
            name.starts_with(|c: char| c == '_' || c.is_ascii_alphabetic())
                && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
        })
    };
    let n = args.iter().take_while(|w| is_assignment(w)).count();
    &args[n..]
}

/// The commands after `-exec`, `-execdir`, `-ok` and `-okdir`, up to `;` or `+`.
fn find_exec(args: &[Word]) -> Vec<Inner> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(w) = it.next() {
        if matches!(
            w.lit.as_deref(),
            Some("-exec" | "-execdir" | "-ok" | "-okdir")
        ) {
            let words: Vec<Word> = it
                .by_ref()
                .take_while(|w| !matches!(w.lit.as_deref(), Some(";" | "+")))
                .cloned()
                .collect();
            if !words.is_empty() {
                out.push(Inner::Command(words));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(ids: &[&str]) -> Verdict {
        Verdict::Ask(ids.iter().map(|s| s.to_string()).collect())
    }

    /// Run a table and report every mismatch at once.
    fn check(cases: &[(&str, Verdict)], rules: &Rules) {
        let failures: Vec<String> = cases
            .iter()
            .filter_map(|(cmd, want)| {
                let got = classify(cmd, rules);
                (got != *want).then(|| format!("{cmd:?}: want {want:?}, got {got:?}"))
            })
            .collect();
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    /// Quote `s` as one single-quoted shell word.
    fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', r"'\''"))
    }

    #[test]
    fn shipped_list() {
        let rules = Rules::shipped();
        let ids: Vec<&str> = rules.ids().collect();
        assert_eq!(
            ids,
            [
                "git push",
                "git reset --hard",
                "git clean",
                "git branch -D",
                "git branch --delete --force",
                "git stash drop",
                "git stash clear",
                "git stash",
                "git checkout",
                "git restore",
                "npm publish",
                "cargo publish",
                "docker push",
                "podman push",
                "terraform apply",
                "kubectl",
                "sudo",
                "doas",
                "psi -p",
                "psi --print",
                "psi --json",
                "rm .git",
            ]
        );
    }

    #[test]
    fn git_commands_that_set_aside_or_discard_work_ask() {
        // A sub-agent stashed and checked out files in the owner's tree to
        // compare old and new behavior, and left two stashes behind with
        // its parent's changes in them (cog trial, 2026-11-04).
        let rules = Rules::shipped();
        for (cmd, want) in [
            ("git stash", ask(&["git stash"])),
            ("git stash push -m wip", ask(&["git stash"])),
            ("git stash -u && go test ./...", ask(&["git stash"])),
            ("git -C repo stash save", ask(&["git stash"])),
            ("git checkout env.go", ask(&["git checkout"])),
            ("git checkout -- .", ask(&["git checkout"])),
            ("git restore src/a.rs", ask(&["git restore"])),
            ("git stash drop", ask(&["git stash drop"])),
            ("git stash list", Verdict::Clear),
            ("git stash show -p stash@{0}", Verdict::Clear),
            ("git stash pop", Verdict::Clear),
            ("git stash apply", Verdict::Clear),
            ("git show HEAD:env.go", Verdict::Clear),
            ("git status", Verdict::Clear),
        ] {
            assert_eq!(classify(cmd, &rules), want, "{cmd}");
        }
    }

    #[test]
    fn not_words_are_left_out_of_the_id() {
        let r = Rules::parse_entry("git stash !list !show").unwrap();
        assert_eq!(r.id, "git stash");
        assert!(Rules::names(&r.id, "git-stash"));
    }

    #[test]
    fn hidden_entries_ask() {
        let push = || ask(&["git push"]);
        let cases = [
            // Structure.
            ("git push", push()),
            ("ls | xargs git push", push()),
            ("make && git push origin main", push()),
            ("false || git push", push()),
            ("cd x; git push", push()),
            ("(cd x && git push)", push()),
            ("{ make; git push; }", push()),
            ("echo $(git push)", push()),
            ("echo `git push`", push()),
            ("echo \"pushed: $(git push)\"", push()),
            ("diff <(git push) x", push()),
            ("cat > >(git push)", push()),
            ("x=$(git push)", push()),
            ("export x=$(git push)", push()),
            ("local x=`git push`", push()),
            ("f() { git push; }; f", push()),
            ("function f { git push; }", push()),
            ("if true; then git push; fi", push()),
            ("if git push; then :; fi", push()),
            ("for i in 1 2; do git push; done", push()),
            ("while true; do git push; done", push()),
            ("until false; do git push; done", push()),
            ("case $x in a) git push;; esac", push()),
            ("! git push", push()),
            ("git push > log 2>&1", push()),
            ("cat <<EOF\n$(git push)\nEOF", push()),
            ("[[ $(git push) ]]", push()),
            // Wrappers and runners.
            ("timeout 10 git push", push()),
            ("timeout -s INT 10 git push", push()),
            ("timeout -k 5 --signal=KILL 10 git push", push()),
            ("timeout --kill-after 5 10 git push", push()),
            ("env FOO=1 git push", push()),
            ("env -i -u HOME -C /tmp FOO=1 BAR=$x git push", push()),
            ("env -S 'git push'", push()),
            ("env -S'FOO=1 git push'", push()),
            ("nice -n 5 git push", push()),
            ("nice -5 git push", push()),
            ("nohup git push &", push()),
            ("time git push", push()),
            ("time -p git push", push()),
            ("command git push", push()),
            ("command -p git push", push()),
            ("exec git push", push()),
            ("sudo -u bob git status", ask(&["sudo"])),
            ("sudo -E git push", ask(&["sudo", "git push"])),
            ("doas -u root git push", ask(&["doas", "git push"])),
            ("xargs -I{} git push {}", push()),
            ("xargs -I {} -n 1 -P 4 git push {}", push()),
            ("xargs -d '\\n' -a list git push", push()),
            ("xargs --max-args 1 git push", push()),
            ("timeout 5 nice env A=1 git push", push()),
            ("sh -c 'git push'", push()),
            ("bash -lc \"cd x && git push\"", push()),
            ("bash -e -x -c 'git push'", push()),
            ("bash -o pipefail -c 'git push'", push()),
            ("/bin/zsh -c 'git push'", push()),
            ("dash -c 'make; git push'", push()),
            ("sh -c \"bash -c 'git push'\"", push()),
            ("eval git push", push()),
            ("eval 'git push'", push()),
            ("eval \"git\" push", push()),
            // A script that expands something still has literal commands.
            ("sh -c \"cd $dir && git push\"", push()),
            ("bash -c \"git push origin $branch\"", push()),
            (
                "sh -c \"git push origin HEAD:$(git branch --show-current)\"",
                push(),
            ),
            ("bash -c \"echo \\\"$x\\\" && git push\"", push()),
            ("eval \"git push $remote\"", push()),
            ("eval git push \"$remote\"", push()),
            ("find . -name x -exec git push \\;", push()),
            ("find . -execdir git push {} +", push()),
            ("find . -ok git push ';' -print", push()),
            // Reordered, clustered and long flags.
            ("git reset HEAD~1 --hard", ask(&["git reset --hard"])),
            ("git branch x -D", ask(&["git branch -D"])),
            ("git branch -fD x", ask(&["git branch -D"])),
            (
                "git branch --force x --delete",
                ask(&["git branch --delete --force"]),
            ),
            ("git -C repo push", push()),
            ("git -c user.name=x push", push()),
            ("git --git-dir=x push", push()),
            // Options whose value expands (2026-11-05 review).
            ("git --work-tree=$PWD push", push()),
            ("git --git-dir=\"$g\" push", push()),
            ("git -C\"$d\" push", push()),
            ("git -c\"$k\"=v push", push()),
            ("/usr/bin/time -f %e git push", push()),
            ("/usr/bin/time -o t.txt git push", push()),
            ("eval -- git push", push()),
            ("builtin command git push", push()),
            ("git stash drop stash@{1}", ask(&["git stash drop"])),
            ("git clean -fdx", ask(&["git clean"])),
            ("psi -p hi", ask(&["psi -p"])),
            ("psi -cp hi", ask(&["psi -p"])),
            ("psi --json -p x", ask(&["psi -p", "psi --json"])),
            ("psi --print=x", ask(&["psi --print"])),
            ("./psi -p x", ask(&["psi -p"])),
            // Quoting and paths.
            ("'git' \"push\"", push()),
            ("\\g\\i\\t pu\\sh", push()),
            ("g\"i\"'t' push", push()),
            ("/usr/bin/git push", push()),
            // Program-only entries and others.
            ("kubectl get pods", ask(&["kubectl"])),
            ("cargo publish --dry-run", ask(&["cargo publish"])),
            ("npm publish", ask(&["npm publish"])),
            ("docker push img", ask(&["docker push"])),
            ("podman push img", ask(&["podman push"])),
            ("terraform apply -auto-approve", ask(&["terraform apply"])),
            // Several, deduplicated, in first-match order.
            (
                "kubectl apply && git push; git push",
                ask(&["kubectl", "git push"]),
            ),
        ];
        check(&cases, &Rules::shipped());
    }

    #[test]
    fn harmless_commands_are_clear() {
        let cases = [
            "git status",
            "git pushd",
            "echo git push",
            "grep -r \"git push\" .",
            "git commit -m \"push it\"",
            "git stash list",
            "git branch -d x",
            "git branch --delete x",
            "git reset --soft HEAD~1",
            "npm install",
            "cargo build",
            "command -v sudo",
            "command -V git push",
            "type sudo",
            "psi --version",
            "psi --printer",
            "echo $(echo hi)",
            "ls -la",
            "sh script.sh push",
            "bash -c 'echo git push'",
            "find . -name push -print",
            "timeout 10",
            "env",
            "xargs",
            // Dynamic forms aren't resolved: they run, and the boundary contains them.
            "$CMD push",
            "git $sub",
            "eval \"$x\"",
            "sh -c \"$script\"",
            "bash -c \"echo $x git push\"",
            "eval \"echo $x\"",
        ];
        let cases: Vec<_> = cases.iter().map(|c| (*c, Verdict::Clear)).collect();
        check(&cases, &Rules::shipped());
    }

    #[test]
    fn known_false_positives_ask() {
        let cases = [
            // A short-flag cluster with an attached value can contain the letter.
            ("psi -mllama-pro", ask(&["psi -p"])),
        ];
        check(&cases, &Rules::shipped());
    }

    #[test]
    fn git_entries_match_the_subcommand() {
        // The 27B's hook test committed a file named `clean` and was denied
        // five times as `git clean` (code eval, 2026-11-05).
        let cases = [
            ("git commit -m clean", Verdict::Clear),
            ("git add push.py && git commit -m push", Verdict::Clear),
            ("git log --grep push", Verdict::Clear),
            ("git checkout push", ask(&["git checkout"])),
            ("git branch -D push", ask(&["git branch -D"])),
            ("git -C /p push", ask(&["git push"])),
            (
                "git -c core.pager=cat -C repo --no-pager push",
                ask(&["git push"]),
            ),
            (
                "git --git-dir .git --work-tree=. clean -fd",
                ask(&["git clean"]),
            ),
            (
                "/usr/bin/git reset HEAD~1 --hard",
                ask(&["git reset --hard"]),
            ),
            ("git -- push", ask(&["git push"])),
            // Other programs still match anywhere.
            ("npm --registry r publish", ask(&["npm publish"])),
        ];
        check(&cases, &Rules::shipped());
        // A user entry whose first word is an option matches anywhere.
        let rules = Rules::layered(&["git --force".into()], &[]).unwrap();
        assert_eq!(
            classify("git push --force", &rules),
            ask(&["git push", "git --force"])
        );
    }

    #[test]
    fn known_false_negatives_are_clear() {
        // Not covered: scripts on stdin, values fed through xargs, unlisted
        // wrappers, and equivalent spellings that aren't on the list.
        let cases = [
            "echo 'git push' | sh",
            "bash <<EOF\ngit push\nEOF",
            "echo push | xargs git",
            "stdbuf -oL git push",
            "git branch -d -f x",
        ];
        let cases: Vec<_> = cases.iter().map(|c| (*c, Verdict::Clear)).collect();
        check(&cases, &Rules::shipped());
    }

    #[test]
    fn unparseable_asks() {
        let rules = Rules::shipped();
        for cmd in [
            "echo \"unterminated",
            "if then fi",
            "git push (",
            "echo $(ls",
            "sh -c 'echo ('",
            "eval 'if then'",
        ] {
            assert!(
                matches!(classify(cmd, &rules), Verdict::Unparseable(_)),
                "{cmd:?}: {:?}",
                classify(cmd, &rules)
            );
        }
        for (cmd, line) in [
            ("echo hi\nif then fi", 2),
            ("echo hi\necho \"unterminated", 2),
            ("ls\nfoo )\nls", 2),
            ("ls\nls\ngit x (", 3),
        ] {
            let want = format!("bash could not parse it near line {line}");
            assert_eq!(classify(cmd, &rules), Verdict::Unparseable(want), "{cmd:?}");
        }
        assert_eq!(
            classify("sh -c 'echo ('", &rules),
            Verdict::Unparseable("bash could not parse the script given to sh near line 1".into())
        );
    }

    #[test]
    fn known_misparses_ask() {
        // Valid bash that tree-sitter-bash 0.25 rejects, so it asks. Relax with
        // fixtures if these turn up often in practice.
        let rules = Rules::shipped();
        for cmd in [
            "cat <<EOF; echo after\nhi\nEOF",
            "cat <<EOF & echo after\nhi\nEOF",
            "cat <<A <<B\na\nA\nb\nB",
            "case $x in a) echo ;& b) echo ;;& esac",
            "shopt -s extglob; ls !(x)",
            "for f in *.rs; { echo $f; }",
        ] {
            assert!(
                matches!(classify(cmd, &rules), Verdict::Unparseable(_)),
                "{cmd:?} now parses: move it to valid_bash_parses"
            );
        }
    }

    #[test]
    fn valid_bash_parses() {
        let cases = [
            "cat <<EOF\nhi\nEOF",
            "cat <<'EOF'\nhi $x\nEOF",
            // A quoted heredoc is literal text, so this doesn't run git push.
            "cat <<'EOF'\n$(git push)\nEOF",
            "cat <<-EOF\n\thi\n\tEOF",
            "cat <<< \"hi\"",
            "[[ -f x ]] && echo y",
            "[[ $a == b* || -n $c ]]",
            "[ -f x ] && echo y",
            "echo $((1+2))",
            "((i++))",
            "case $x in a) echo;; b|c) echo;; *) ;; esac",
            "for i in 1 2; do echo $i; done",
            "for ((i=0; i<3; i++)); do echo $i; done",
            "arr=(a b); echo ${arr[@]}",
            "echo ${x:-default} ${#x} ${x%%.*} ${x/a/b}",
            "x=1 y=2 make",
            "echo 'it'\\''s'",
            "echo $'a\\tb'",
            "a=$(cat <<EOF\nhi\nEOF\n)",
            "select x in a b; do break; done",
            "exec 3>&1; echo hi >&3",
            "echo hi |& cat",
            "coproc cat",
            "trap 'echo bye' EXIT",
            "while read -r l; do echo \"$l\"; done < f",
        ];
        let cases: Vec<_> = cases.iter().map(|c| (*c, Verdict::Clear)).collect();
        check(&cases, &Rules::shipped());
    }

    #[test]
    fn nested_scripts_stop_at_the_depth_limit() {
        let rules = Rules::shipped();
        let nest = |n: usize| {
            (0..n).fold("git push".to_string(), |s, _| {
                format!("sh -c {}", quote(&s))
            })
        };
        assert_eq!(classify(&nest(3), &rules), ask(&["git push"]));
        assert_eq!(
            classify(&nest(4), &rules),
            Verdict::Unparseable("scripts nested more than 3 deep".into())
        );
        let evals = format!(
            "eval {}",
            quote(&format!("sh -c {}", quote("eval git push")))
        );
        assert_eq!(classify(&evals, &rules), ask(&["git push"]));
    }

    #[test]
    fn layers_add_remove_and_dedupe() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let rules = Rules::layered(&s(&["git  rebase ", "git push"]), &s(&["git-clean"])).unwrap();
        assert_eq!(rules.ids().filter(|id| *id == "git push").count(), 1);
        assert!(rules.ids().any(|id| id == "git rebase"));
        assert!(!rules.ids().any(|id| id == "git clean"));
        assert_eq!(classify("git rebase -i main", &rules), ask(&["git rebase"]));
        assert_eq!(classify("git clean -fdx", &rules), Verdict::Clear);
        assert_eq!(Rules::layered(&[], &[]).unwrap(), Rules::shipped());

        assert_eq!(Rules::layered(&s(&[""]), &[]), Err(RuleError::Empty));
        assert_eq!(Rules::layered(&[], &s(&["  "])), Err(RuleError::Empty));
        assert_eq!(Rules::parse_entry("  "), Err(RuleError::Empty));
        let rule = Rules::parse_entry("/usr/bin/git branch -D").unwrap();
        assert_eq!(rule.id, "/usr/bin/git branch -D");
        assert_eq!(rule.program, "git");
        assert!(matches!(
            Rules::from_jsonc("{ \"always_ask\": 3 }"),
            Err(RuleError::File(_))
        ));
    }

    #[test]
    fn flags_name_rule_ids() {
        assert!(Rules::names("git push", "git-push"));
        assert!(Rules::names("git push", "git push"));
        assert!(Rules::names("git branch -D", "git-branch-D"));
        assert!(Rules::names("psi --json", "psi-json"));
        assert!(!Rules::names("git push", "git-pushd"));
        assert!(!Rules::names("git branch -D", "git-branch-d"));
        assert!(!Rules::names("psi -p", "psi-print"));
    }

    #[test]
    fn entry_words() {
        assert!(word_matches("push", "push"));
        assert!(!word_matches("push", "pushd"));
        assert!(word_matches("-D", "-fD"));
        assert!(!word_matches("-D", "--D"));
        assert!(!word_matches("-D", "--Delete"));
        assert!(word_matches("--print", "--print=x"));
        assert!(!word_matches("--print", "--printer"));
    }

    #[test]
    fn removing_git_asks_in_every_shape() {
        let rules = Rules::shipped();
        for c in [
            "rm -rf .git",
            "rm -rf ./.git/",
            "rm -r /home/u/proj/.git",
            "rm -rf \".git\"",
            "rm -rf '.git'",
            "rm -rf .git/objects",
            "rm -rf .git/*",
            "rm -rf .*",
            "rm -rf \"$(pwd)/.git\"",
            "cd sub && rm -rf ../.git",
            "rmdir .git",
            "unlink .git/index",
            "mv .git /tmp/old-git",
            "find . -name .git -exec rm -rf {} +",
            "find . -path ./.git -delete",
            "sh -c 'rm -rf .git'",
            "git init; rm -rf .git; git init",
            "trash .git",
            "gio trash .git",
        ] {
            assert_eq!(classify(c, &rules), ask(&[GIT_DIR_RULE]), "{c}");
        }
        for c in [
            "rm -rf build",
            "rm .gitignore",
            "rm -r .github/workflows",
            "rm .gitmodules",
            "rm -rf node_modules/*",
            "find . -name .git",
            "ls .git",
            "cat .git/HEAD",
            "git status",
            "mv a.txt b.txt",
        ] {
            assert_eq!(classify(c, &rules), Verdict::Clear, "{c}");
        }
        // Settings can remove it like any shipped entry.
        let off = Rules::layered(&[], &["rm .git".into()]).unwrap();
        assert_eq!(classify("rm -rf .git", &off), Verdict::Clear);
        assert!(Rules::shipped().ids().any(|i| i == GIT_DIR_RULE));
    }
}
