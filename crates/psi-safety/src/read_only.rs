//! Plan mode's bash check (psi-plan.md §10, D-34): a command runs only when
//! every simple command in it, including those in pipelines, substitutions
//! and `sh -c` scripts, is a known read-only program, and nothing is
//! redirected into a file. Pi checks the whole string against regexes;
//! the parser here also sees `ls; rm x` and `cat x > y` for what they are.

use tree_sitter::{Node, Parser};

use crate::always_ask::{Inner, Word, basename, collect, error_line, options, runs};

/// How many `sh -c` scripts deep the check looks.
const MAX_DEPTH: usize = 3;

/// Programs that only read (pi's plan-mode list, plus a few of the same kind).
const READ_ONLY: &[&str] = &[
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "find",
    "fd",
    "ls",
    "tree",
    "pwd",
    // Change only the shell's own directory; models start many commands with them.
    "cd",
    "pushd",
    "popd",
    "echo",
    "printf",
    "wc",
    "sort",
    "uniq",
    "diff",
    "cmp",
    "file",
    "stat",
    "du",
    "df",
    "which",
    "whereis",
    "type",
    "printenv",
    "uname",
    "whoami",
    "id",
    "date",
    "cal",
    "uptime",
    "ps",
    "free",
    "jq",
    "awk",
    "sed",
    "bat",
    "eza",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "cut",
    "tr",
    "column",
    "nl",
    "od",
    "xxd",
    "hexdump",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "true",
    "false",
    "test",
    "[",
    "git",
];

/// Programs that run another command; that command is checked instead.
const WRAPPERS: &[&str] = &["timeout", "nice", "time", "env", "command", "xargs"];

/// `git` subcommands that only read. Some need a closer look (`branch`).
const GIT_READ: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "ls-files",
    "ls-tree",
    "rev-parse",
    "blame",
    "grep",
    "describe",
    "shortlog",
    "cat-file",
    "branch",
    "remote",
    "tag",
    "config",
    "stash",
    "worktree",
    "rev-list",
    "name-rev",
    "show-ref",
    "merge-base",
];

/// Why a command isn't read-only, or `Ok` when it is.
pub fn check(command: &str) -> Result<(), String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .map_err(|_| "the bash parser failed to load".to_owned())?;
    script(&mut parser, command, 0)
}

/// The program of the last simple command in `command`, through wrappers
/// (`timeout 5 grep x` is `grep`), with git's subcommand (`git grep`): the
/// command whose exit status bash reports in the common shapes
/// (`cd x && grep y f`, `ls | grep y`). None when it can't be told.
pub fn last_program(command: &str) -> Option<String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(command, None)?;
    if error_line(tree.root_node()).is_some() {
        return None;
    }
    let mut commands = Vec::new();
    collect(tree.root_node(), command, &mut commands);
    let mut argv = commands.pop()?;
    loop {
        let (prog, args) = argv.split_first()?;
        let name = basename(prog.lit.as_deref()?).to_owned();
        match runs(&name, args).as_slice() {
            [Inner::Command(inner)] if WRAPPERS.contains(&name.as_str()) => argv = inner.clone(),
            _ if name == "git" => {
                let (i, _) = options(args, "Cc", &[]);
                let sub = args.get(i)?.lit.clone()?;
                return Some(format!("git {sub}"));
            }
            _ => return Some(name),
        }
    }
}

fn script(parser: &mut Parser, src: &str, depth: usize) -> Result<(), String> {
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| "bash could not parse it".to_owned())?;
    if error_line(tree.root_node()).is_some() {
        return Err("bash could not parse it".into());
    }
    redirects(tree.root_node(), src)?;
    assignments(tree.root_node(), src)?;
    let mut commands = Vec::new();
    collect(tree.root_node(), src, &mut commands);
    for argv in &commands {
        simple(parser, argv, depth)?;
    }
    Ok(())
}

/// Output redirected into a file writes it; `/dev/null` and `2>&1` don't.
fn redirects(node: Node, src: &str) -> Result<(), String> {
    if node.kind() == "file_redirect" {
        let text = &src[node.byte_range()];
        let op_writes = text.contains('>');
        let dest = node
            .child_by_field_name("destination")
            .map(|d| src[d.byte_range()].trim().to_owned())
            .unwrap_or_default();
        let dup = text.contains(">&") && dest.chars().all(|c| c.is_ascii_digit() || c == '-');
        if op_writes && !dup && dest != "/dev/null" {
            return Err(format!("it writes to {dest}"));
        }
    }
    for c in node.children(&mut node.walk()) {
        redirects(c, src)?;
    }
    Ok(())
}

/// `NAME=value cmd` can change what `cmd` runs (`GIT_EXTERNAL_DIFF`,
/// `LD_PRELOAD`); a plain assignment on its own line doesn't.
fn assignments(node: Node, src: &str) -> Result<(), String> {
    if node.kind() == "command"
        && let Some(a) = node
            .children(&mut node.walk())
            .find(|c| c.kind() == "variable_assignment")
    {
        let text = &src[a.byte_range()];
        let name = text.split('=').next().unwrap_or(text);
        return Err(format!(
            "{name}=... before a command can change what it runs"
        ));
    }
    for c in node.children(&mut node.walk()) {
        assignments(c, src)?;
    }
    Ok(())
}

/// Operands after the options (words not starting with `-`).
fn operands(args: &[Word], from: usize) -> Vec<&str> {
    args[from..]
        .iter()
        .filter_map(|w| w.lit.as_deref())
        .filter(|a| !a.starts_with('-'))
        .collect()
}

/// Whether a sed script writes files or runs commands: the `w`, `W` and
/// `e` commands, and the `w` and `e` flags of `s`.
fn sed_writes(script: &str) -> bool {
    /// The index after the closing `d`, skipping escapes.
    fn skip(cs: &[char], mut i: usize, d: char) -> usize {
        while i < cs.len() {
            if cs[i] == '\\' {
                i += 2;
                continue;
            }
            if cs[i] == d {
                return i + 1;
            }
            i += 1;
        }
        cs.len()
    }
    let cs: Vec<char> = script.chars().collect();
    let to = |i: usize, stop: &[char]| {
        cs[i..]
            .iter()
            .position(|c| stop.contains(c))
            .map_or(cs.len(), |p| i + p)
    };
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            // Separators and addresses.
            ';' | '{' | '}' | '!' | '$' | ',' | '~' | '+' => i += 1,
            c if c.is_whitespace() || c.is_ascii_digit() => i += 1,
            '/' => i = skip(&cs, i + 1, '/'),
            '\\' if i + 1 < cs.len() => i = skip(&cs, i + 2, cs[i + 1]),
            'w' | 'W' | 'e' => return true,
            's' | 'y' => {
                let Some(&d) = cs.get(i + 1) else {
                    return false;
                };
                i = skip(&cs, skip(&cs, i + 2, d), d);
                if c == 's' {
                    while i < cs.len()
                        && !matches!(cs[i], ';' | '}' | '\n')
                        && !cs[i].is_whitespace()
                    {
                        if matches!(cs[i], 'w' | 'e') {
                            return true;
                        }
                        i += 1;
                    }
                }
            }
            // Text or a file name to the end of the line.
            'a' | 'i' | 'c' | 'r' | 'R' => i = to(i, &['\n']),
            // A label.
            'b' | 't' | 'T' | ':' => i = to(i, &[';', '\n']),
            _ => i += 1,
        }
    }
    false
}

fn simple(parser: &mut Parser, argv: &[Word], depth: usize) -> Result<(), String> {
    let Some((prog, args)) = argv.split_first() else {
        return Ok(());
    };
    let Some(name) = prog.lit.as_deref().map(basename) else {
        return Err(format!("{} isn't a plain command name", prog.raw));
    };
    if matches!(name, "sh" | "bash" | "zsh" | "dash") {
        let inner = runs(name, args);
        let [Inner::Script(s)] = inner.as_slice() else {
            return Err(format!("{name} without -c runs a script file"));
        };
        if depth >= MAX_DEPTH {
            return Err(format!("scripts nested more than {MAX_DEPTH} deep"));
        }
        return script(parser, s, depth + 1);
    }
    if name == "env"
        && args.iter().any(|w| {
            w.lit
                .as_deref()
                .is_some_and(|a| a.contains('=') && !a.starts_with('-'))
        })
    {
        return Err("env NAME=value can change what the command runs".into());
    }
    if WRAPPERS.contains(&name) {
        for inner in runs(name, args) {
            match inner {
                Inner::Command(words) => simple(parser, &words, depth)?,
                Inner::Script(s) => script(parser, &s, depth + 1)?,
            }
        }
        return Ok(());
    }
    if !READ_ONLY.contains(&name) {
        return Err(format!("{name} isn't on the read-only list"));
    }
    let lits: Vec<&str> = args.iter().filter_map(|w| w.lit.as_deref()).collect();
    match name {
        "find" => {
            for bad in ["-delete", "-fprint", "-fprint0", "-fprintf", "-fls"] {
                if lits.contains(&bad) {
                    return Err(format!("find {bad} writes"));
                }
            }
            for inner in runs(name, args) {
                if let Inner::Command(words) = inner {
                    simple(parser, &words, depth)?;
                }
            }
        }
        "sed" => {
            let (i, opts) = options(args, "efl", &["--expression", "--file"]);
            if opts
                .iter()
                .any(|(o, _)| o == "i" || o.starts_with("--in-place"))
            {
                return Err("sed -i edits files".into());
            }
            if opts.iter().any(|(o, _)| o == "f" || o == "--file") {
                return Err("sed -f runs a script file".into());
            }
            let mut scripts: Vec<&str> = opts
                .iter()
                .filter(|(o, _)| o == "e" || o == "--expression")
                .filter_map(|(_, v)| v.as_deref())
                .collect();
            if scripts.is_empty() {
                scripts.extend(args.get(i).and_then(|w| w.lit.as_deref()));
            }
            if scripts.iter().any(|s| sed_writes(s)) {
                return Err("this sed script writes files or runs commands (w, W, e)".into());
            }
        }
        "awk" => {
            let (i, opts) = options(
                args,
                "fvFe",
                &[
                    "--file",
                    "--assign",
                    "--field-separator",
                    "--source",
                    "--include",
                    "--load",
                ],
            );
            if opts
                .iter()
                .any(|(o, _)| matches!(o.as_str(), "f" | "--file" | "--include" | "--load"))
            {
                return Err("awk -f runs a program file".into());
            }
            let mut programs: Vec<&str> = opts
                .iter()
                .filter(|(o, _)| o == "e" || o == "--source")
                .filter_map(|(_, v)| v.as_deref())
                .collect();
            if programs.is_empty() {
                programs.extend(args.get(i).and_then(|w| w.lit.as_deref()));
            }
            if programs.iter().any(|p| {
                p.contains("system") || p.contains('>') || p.contains('|') || p.contains("@load")
            }) {
                return Err(
                    "this awk program may write files or run commands (>, |, system)".into(),
                );
            }
        }
        "sort" => {
            let (_, opts) = options(
                args,
                "oktST",
                &[
                    "--output",
                    "--key",
                    "--field-separator",
                    "--buffer-size",
                    "--temporary-directory",
                    "--compress-program",
                    "--files0-from",
                    "--random-source",
                ],
            );
            if opts
                .iter()
                .any(|(o, _)| matches!(o.as_str(), "o" | "--output" | "--compress-program"))
            {
                return Err("sort -o writes a file".into());
            }
        }
        "uniq" => {
            let (i, _) = options(
                args,
                "fsw",
                &["--skip-fields", "--skip-chars", "--check-chars"],
            );
            if operands(args, i).len() > 1 {
                return Err("uniq writes its second file".into());
            }
        }
        "tree" if lits.contains(&"-o") => {
            return Err("tree -o writes a file".into());
        }
        "file" if lits.iter().any(|a| *a == "-C" || *a == "--compile") => {
            return Err("file -C writes a file".into());
        }
        "xxd" => {
            let (i, opts) = options(args, "cglosn", &[]);
            if opts.iter().any(|(o, _)| o == "r") || lits.contains(&"-revert") {
                return Err("xxd -r writes binary".into());
            }
            if operands(args, i).len() > 1 {
                return Err("xxd writes its second file".into());
            }
        }
        "git" => git(args)?,
        _ => {}
    }
    Ok(())
}

fn git(args: &[Word]) -> Result<(), String> {
    let (i, opts) = options(
        args,
        "Cc",
        &[
            "--git-dir",
            "--work-tree",
            "--namespace",
            "--config-env",
            "--exec-path",
        ],
    );
    if opts
        .iter()
        .any(|(o, v)| o == "c" || o == "--config-env" || (o == "--exec-path" && v.is_some()))
    {
        return Err("git -c can make git run commands".into());
    }
    let rest: Vec<&str> = args[i..].iter().filter_map(|w| w.lit.as_deref()).collect();
    let Some((&sub, more)) = rest.split_first() else {
        return Ok(());
    };
    if !GIT_READ.contains(&sub) {
        return Err(format!("git {sub} isn't read-only"));
    }
    let positional = more.iter().any(|a| !a.starts_with('-'));
    let has = |flags: &[&str]| more.iter().any(|a| flags.contains(a));
    let ok = match sub {
        // Listing only: a name creates, -d/-m/-c change.
        "branch" => {
            !positional
                && !has(&[
                    "-d",
                    "-D",
                    "-m",
                    "-M",
                    "-c",
                    "-C",
                    "--delete",
                    "--move",
                    "--copy",
                    "--set-upstream-to",
                    "-u",
                    "--unset-upstream",
                    "-f",
                    "--force",
                ])
        }
        "tag" => {
            (!positional || has(&["-l", "--list"]))
                && !has(&[
                    "-d",
                    "--delete",
                    "-a",
                    "--annotate",
                    "-s",
                    "--sign",
                    "-u",
                    "-f",
                    "--force",
                    "-m",
                    "-F",
                    "-e",
                    "--edit",
                ])
        }
        // Listing only: the first word that isn't an option says what it does.
        "remote" => more
            .iter()
            .find(|a| !a.starts_with('-'))
            .is_none_or(|a| matches!(*a, "show" | "get-url")),
        "config" => {
            has(&["--get", "--get-all", "--get-regexp", "--list", "-l"])
                && !has(&[
                    "--add",
                    "--unset",
                    "--unset-all",
                    "--replace-all",
                    "--rename-section",
                    "--remove-section",
                    "-e",
                    "--edit",
                ])
        }
        "diff" | "log" | "show" => !more
            .iter()
            .any(|a| a.starts_with("--output") || *a == "--ext-diff"),
        "grep" => !more
            .iter()
            .any(|a| a.starts_with("-O") || a.starts_with("--open-files-in-pager")),
        "stash" => matches!(more.first(), Some(&"list" | &"show")),
        "worktree" => more.first() == Some(&"list"),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "git {sub} with these arguments changes the repository"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_commands_pass() {
        for c in [
            "ls -la",
            "cat a.txt | sort | uniq -c",
            "grep -rn foo src/ && wc -l src/*.rs",
            "find . -name '*.rs' -exec grep -l TODO {} +",
            "git status && git log --oneline -5 && git diff HEAD~1",
            "git -C sub branch -a",
            "git config --get user.name",
            "sed -n '1,20p' file",
            "head -5 x 2>/dev/null; tail x 2>&1",
            "echo $(git rev-parse HEAD)",
            "timeout 5 rg foo",
            "bash -c 'ls | head'",
            "env | sort",
            "  ls",
            // Still fine after the stricter checks.
            "sort -n -k2 f",
            "uniq -c f",
            "awk '{print $1}' f",
            "awk -F: '$3 == 0 {print $1}' /etc/passwd",
            "sed -n '/fn main/,/^}/p' src/main.rs",
            "sed 's/old/new/g' f",
            "sed -e '1d' -e 's/a/b/' f",
            "xxd f",
            "git tag -l",
            "git remote -v",
            "git remote show origin",
            "git config --list",
            "git diff --stat",
            "FOO=1",
            "cd /home/x/project && ls -la",
            "cd sub; git status",
            "pushd a && ls && popd",
        ] {
            assert_eq!(check(c), Ok(()), "{c}");
        }
        assert_eq!(
            check("cd /x && go build ./..."),
            Err("go isn't on the read-only list".into())
        );
    }

    #[test]
    fn writes_and_unknown_programs_are_refused() {
        for (c, why) in [
            ("rm -rf x", "rm isn't on the read-only list"),
            ("ls; rm x", "rm isn't on the read-only list"),
            ("echo hi > f.txt", "it writes to f.txt"),
            ("cat a >> b", "it writes to b"),
            ("sed -i s/a/b/ f", "sed -i edits files"),
            ("find . -delete", "find -delete writes"),
            ("find . -exec rm {} ;", "rm isn't on the read-only list"),
            ("git commit -m x", "git commit isn't read-only"),
            ("git branch new", "git branch with these arguments"),
            ("git stash", "git stash with these arguments"),
            ("echo $(touch x)", "touch isn't on the read-only list"),
            ("sh -c 'mkdir d'", "mkdir isn't on the read-only list"),
            ("sh script.sh", "sh without -c runs a script file"),
            ("$CMD x", "$CMD isn't a plain command name"),
            ("xargs rm < list", "rm isn't on the read-only list"),
            (
                "python3 -c 'print(1)'",
                "python3 isn't on the read-only list",
            ),
            ("ls (", "bash could not parse it"),
            ("sudo ls", "sudo isn't on the read-only list"),
            // Read-only programs that can write or run code (2026-11-01 audit).
            ("sort -o out in", "sort -o writes a file"),
            ("sort --output=out in", "sort -o writes a file"),
            ("sort --compress-program=sh in", "sort -o writes a file"),
            ("uniq in out", "uniq writes its second file"),
            ("tree -o out", "tree -o writes a file"),
            ("file -C -m x", "file -C writes a file"),
            ("xxd -r in out", "xxd -r writes binary"),
            ("xxd in out", "xxd writes its second file"),
            ("awk '{print > \"f\"}' in", "this awk program"),
            ("awk 'BEGIN{system(\"id\")}'", "this awk program"),
            ("awk -f prog.awk in", "awk -f runs a program file"),
            ("sed -n 'w out' in", "this sed script"),
            ("sed '1e id' in", "this sed script"),
            ("sed 's/a/b/w out' in", "this sed script"),
            ("sed -e 's/a/b/e' in", "this sed script"),
            ("sed -f script.sed in", "sed -f runs a script file"),
            ("git diff --output=f", "git diff with these arguments"),
            (
                "git -c core.fsmonitor=id status",
                "git -c can make git run commands",
            ),
            ("git grep -Oid x", "git grep with these arguments"),
            ("git tag -l -d v1", "git tag with these arguments"),
            ("git remote -v add o url", "git remote with these arguments"),
            (
                "git config --list --add a.b c",
                "git config with these arguments",
            ),
            (
                "GIT_EXTERNAL_DIFF=x git diff",
                "GIT_EXTERNAL_DIFF=... before a command",
            ),
            ("LD_PRELOAD=x.so cat f", "LD_PRELOAD=... before a command"),
            ("env LD_PRELOAD=x.so cat f", "env NAME=value"),
        ] {
            let got = check(c).unwrap_err();
            assert!(got.starts_with(why), "{c}: {got}");
        }
    }

    /// Every file under `dir` but `.git`, with its content.
    fn snapshot(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.clone(), std::fs::read(&p).unwrap_or_default()));
                }
            }
        }
        out.sort();
        out
    }

    /// The accepted commands, run for real by bash in a git repository:
    /// none may change a file (git's own index refresh aside).
    #[test]
    fn accepted_commands_change_nothing_when_run() {
        let d = tempfile::tempdir().unwrap();
        let w = d.path();
        let sh = |c: &str| {
            std::process::Command::new("bash")
                .args(["-c", c])
                .current_dir(w)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap()
        };
        std::fs::create_dir_all(w.join("src")).unwrap();
        std::fs::write(w.join("f"), "b 2\na 1\na 1\nold\n").unwrap();
        std::fs::write(w.join("src/main.rs"), "fn main() {\n}\n").unwrap();
        let init = "git init -q && git -c user.name=t -c user.email=t@t add -A && git -c user.name=t -c user.email=t@t commit -qm x && git tag v1";
        assert!(sh(init).status.success());
        let before = snapshot(w);
        for c in [
            "ls -la",
            "cat f | sort | uniq -c",
            "grep -rn main src/ && wc -l src/*.rs",
            "find . -name '*.rs' -exec grep -l main {} +",
            "git status && git log --oneline -5 && git diff HEAD",
            "git branch -a",
            "git tag -l",
            "git remote -v",
            "git config --list",
            "git diff --stat",
            "sed -n '1,2p' f",
            "sed 's/old/new/g' f",
            "sed -e '1d' -e 's/a/b/' f",
            "awk '{print $1}' f",
            "sort -n -k2 f",
            "uniq -c f",
            "xxd f",
            "head -5 f 2>/dev/null; tail f 2>&1",
            "echo $(git rev-parse HEAD)",
            "bash -c 'ls | head'",
            "env | sort",
        ] {
            assert_eq!(check(c), Ok(()), "{c}");
            sh(c);
            assert_eq!(snapshot(w), before, "{c} changed a file");
        }
    }

    #[test]
    fn last_program_is_the_one_whose_status_bash_reports() {
        for (cmd, want) in [
            ("grep -n x f", Some("grep")),
            ("cd src && grep -rn x .", Some("grep")),
            ("cat f | grep x", Some("grep")),
            ("grep x f | head -5", Some("head")),
            ("timeout 5 rg x", Some("rg")),
            ("/usr/bin/grep x f", Some("grep")),
            ("git -C repo grep -n x", Some("git grep")),
            ("git status", Some("git status")),
            ("$CMD x", None),
            ("grep 'x", None),
        ] {
            assert_eq!(last_program(cmd).as_deref(), want, "{cmd}");
        }
    }
}
