//! Which command's exit status bash reports, so the bash tool can tell a
//! search that found nothing from a failure.

use tree_sitter::Parser;

use crate::refuse::{Inner, basename, collect, error_line, options, runs};

/// Programs that run another command; that command is checked instead.
const WRAPPERS: &[&str] = &["timeout", "nice", "time", "env", "command", "xargs"];

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

/// Whether the last statement in `command` runs whatever ran before it:
/// after `;` or a newline, or the whole command, but not the right side of
/// `&&` or `||` (`echo ---; grep x f` yes, `cd x && grep y f` no). Then the
/// exit status bash reports is that statement's own. False when it can't
/// be told.
pub fn last_always_runs(command: &str) -> bool {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .is_err()
    {
        return false;
    }
    let Some(tree) = parser.parse(command, None) else {
        return false;
    };
    let root = tree.root_node();
    if error_line(root).is_some() {
        return false;
    }
    let mut cursor = root.walk();
    let last = root
        .named_children(&mut cursor)
        .filter(|n| n.kind() != "comment")
        .last();
    last.is_some_and(|n| n.kind() != "list")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn last_always_runs_after_a_semicolon_not_after_and() {
        for (cmd, want) in [
            ("grep x f", true),
            ("echo ===; grep x f", true),
            ("echo ===\ngrep x f # done", true),
            ("cat f | grep x", true),
            ("a && b; grep x f 2>&1", true),
            ("cd src && grep x f", false),
            ("echo ===; cd src && grep x f", false),
            ("false || grep x f", false),
            ("grep 'x", false),
        ] {
            assert_eq!(last_always_runs(cmd), want, "{cmd}");
        }
    }
}
