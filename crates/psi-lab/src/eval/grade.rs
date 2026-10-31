//! Grading a project with a task's hidden checks.

use std::path::Path;

use super::podman::Container;
use super::tasks::Task;
use crate::error::other;
use crate::{Result, fsutil};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grade {
    pub passed: u32,
    pub total: u32,
}

impl Grade {
    pub fn all_passed(&self) -> bool {
        self.total > 0 && self.passed == self.total
    }
}

/// Grades the project `src` (without its `.git`) in a fresh container with
/// no network: the copy at `/grade`, the task's `hidden/` at `/hidden` and
/// `lib_dir` at `/evallib`. Output goes to `out/checks.txt`, ending with
/// "checks: N passed of TOTAL". A check that never printed counts as failed.
pub fn grade(task: &Task, lib_dir: &Path, src: &Path, out: &Path) -> Result<Grade> {
    let project = out.join("project");
    let home = out.join("home");
    fsutil::create_dir_all(&home)?;
    fsutil::create_dir_all(&project)?;
    fsutil::copy_dir(src, &project, &[".git"])?;
    let checks = out.join("checks.txt");
    let stderr = out.join("checks.stderr");
    let code = Container::new("none", &home)
        .mount(&project, "/grade", false)
        .mount(&task.dir.join("hidden"), "/hidden", true)
        .mount(lib_dir, "/evallib", true)
        .workdir("/grade")
        .cmd(["timeout", "-k", "10", "300", "bash", "/hidden/check.sh"])
        .run(&checks, &stderr)?;
    // 125 and up: podman, timeout or bash could not start the checks.
    if !(0..125).contains(&code) {
        let err = fsutil::read_to_string(&stderr).unwrap_or_default();
        return Err(other(format!(
            "the grading container failed (exit {code}): {}",
            err.trim()
        )));
    }
    let text = fsutil::read_to_string(&checks)?;
    let passed = count_passes(&text).min(task.total);
    fsutil::write(
        &checks,
        format!("{text}checks: {passed} passed of {}\n", task.total),
    )?;
    Ok(Grade {
        passed,
        total: task.total,
    })
}

fn count_passes(text: &str) -> u32 {
    text.lines().filter(|l| l.starts_with("PASS ")).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_pass_lines_only() {
        let t = "PASS a\nFAIL b: exit 1\nPASS c\n  PASS not-a-line\nPASSING\n";
        assert_eq!(count_passes(t), 2);
    }

    #[test]
    fn all_passed_needs_checks() {
        assert!(
            Grade {
                passed: 3,
                total: 3
            }
            .all_passed()
        );
        assert!(
            !Grade {
                passed: 2,
                total: 3
            }
            .all_passed()
        );
        assert!(
            !Grade {
                passed: 0,
                total: 0
            }
            .all_passed()
        );
    }
}
