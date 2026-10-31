//! Small filesystem and time helpers.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// UTC time as `YYYY-MM-DD-HHMM`, for run directory names.
pub fn utc_stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}-{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamp_formats_known_times() {
        assert_eq!(utc_stamp(0), "1970-01-01-0000");
        // 2026-10-31T09:16:40Z
        assert_eq!(utc_stamp(1_793_438_200_000), "2026-10-31-0916");
        // 2000-02-29T23:59:00Z, a leap day
        assert_eq!(utc_stamp(951_868_740_000), "2000-02-29-2359");
    }

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
