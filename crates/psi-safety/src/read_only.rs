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

fn script(parser: &mut Parser, src: &str, depth: usize) -> Result<(), String> {
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| "bash could not parse it".to_owned())?;
    if error_line(tree.root_node()).is_some() {
        return Err("bash could not parse it".into());
    }
    redirects(tree.root_node(), src)?;
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
            let (_, opts) = options(args, "efl", &["--expression", "--file"]);
            if opts
                .iter()
                .any(|(o, _)| o == "i" || o.starts_with("--in-place"))
            {
                return Err("sed -i edits files".into());
            }
        }
        "git" => git(args)?,
        _ => {}
    }
    Ok(())
}

fn git(args: &[Word]) -> Result<(), String> {
    let (i, _) = options(args, "Cc", &["--git-dir", "--work-tree", "--namespace"]);
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
        "tag" => !positional || has(&["-l", "--list"]),
        "remote" => more
            .first()
            .is_none_or(|a| matches!(*a, "-v" | "--verbose" | "show" | "get-url")),
        "config" => has(&["--get", "--get-all", "--get-regexp", "--list", "-l"]),
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
        ] {
            assert_eq!(check(c), Ok(()), "{c}");
        }
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
        ] {
            let got = check(c).unwrap_err();
            assert!(got.starts_with(why), "{c}: {got}");
        }
    }
}
