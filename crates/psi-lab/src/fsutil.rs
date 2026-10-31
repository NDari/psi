//! Small filesystem and time helpers.

use std::path::Path;

use crate::Result;
use crate::error::IoContext;

/// Copies the tree `src` into `dst` (created if missing), keeping file
/// modes and symlinks. Top-level names in `skip` are left out.
pub(crate) fn copy_dir(src: &Path, dst: &Path, skip: &[&str]) -> Result<()> {
    std::fs::create_dir_all(dst).ctx(|| format!("creating {}", dst.display()))?;
    let entries = std::fs::read_dir(src).ctx(|| format!("reading {}", src.display()))?;
    for entry in entries {
        let entry = entry.ctx(|| format!("reading {}", src.display()))?;
        let name = entry.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let ty = entry
            .file_type()
            .ctx(|| format!("reading {}", from.display()))?;
        if ty.is_symlink() {
            let target = std::fs::read_link(&from).ctx(|| format!("reading {}", from.display()))?;
            std::os::unix::fs::symlink(&target, &to)
                .ctx(|| format!("creating {}", to.display()))?;
        } else if ty.is_dir() {
            copy_dir(&from, &to, &[])?;
        } else {
            std::fs::copy(&from, &to).ctx(|| format!("copying {}", from.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    std::fs::write(path, contents).ctx(|| format!("writing {}", path.display()))
}

pub(crate) fn read_to_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).ctx(|| format!("reading {}", path.display()))
}

pub(crate) fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).ctx(|| format!("creating {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_dir_keeps_modes_and_skips_top_level_names() {
        use std::os::unix::fs::PermissionsExt;
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("a/.git")).unwrap();
        std::fs::create_dir_all(src.path().join(".git")).unwrap();
        std::fs::write(src.path().join("a/run.sh"), "x").unwrap();
        std::fs::set_permissions(
            src.path().join("a/run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        copy_dir(src.path(), dst.path(), &[".git"]).unwrap();
        assert!(!dst.path().join(".git").exists());
        assert!(dst.path().join("a/.git").exists());
        let mode = std::fs::metadata(dst.path().join("a/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}
