//! Path-safety primitives (`Start.md` §16.1).
//!
//! All cleanup-boundary decisions go through these helpers. The design
//! rules implemented here:
//!
//! - *Component-based containment* — never string prefixes
//!   (`C:\Users\x` vs `C:\Users\xyz` must not match).
//! - *Strict descendant* — a candidate equal to the root is NOT within
//!   (§16.1 rejects over-broad roots like the user home itself).
//! - *Windows case-insensitivity* — NTFS treats `C:\Users` and
//!   `c:\users` as the same path (and WorkBuddy is observed with both
//!   `~/WorkBuddy` and `~/Workbuddy` spellings, see
//!   `docs/providers/workbuddy.md`).
//! - *macOS case-sensitivity* — compared exactly. This is deliberately
//!   the conservative direction: on a case-insensitive volume two paths
//!   differing only by case are the same file, so treating them as
//!   distinct can only *refuse* an operation (fail closed), never allow
//!   an out-of-root one.
//! - *Verbatim/UNC normalization* — `std::fs::canonicalize` on Windows
//!   returns `\\?\`-prefixed paths; comparisons unify the forms.
//! - *Reserved device names* — `CON`, `PRN`, … are rejected on Windows
//!   before any file operation.
//!
//! Platform branching is confined to small `eq` helpers (red line #11:
//! this crate *is* the platform layer).

use std::borrow::Cow;
use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Canonicalize for safety-critical use (§16.1 "all paths canonicalize
/// before execution").
///
/// Wraps [`std::fs::canonicalize`] and strips the Windows verbatim
/// (`\\?\`) prefix so the result is comparable and human-readable.
/// Non-existent paths are an error — the fail-closed direction: an
/// executor must never guess about a path it could not resolve.
pub fn safe_canonicalize(path: &Path) -> io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(strip_verbatim(&canonical).into_owned())
}

/// Strip the Windows verbatim prefix when present:
/// `\\?\C:\x` → `C:\x`, `\\?\UNC\s\share\x` → `\\s\share\x`.
///
/// Non-verbatim and non-UTF-16-representable paths pass through
/// unchanged. Kept as the shared normalization so canonicalized paths
/// (which *do* carry the prefix on Windows) and user-displayed paths
/// compare equal.
pub fn strip_verbatim(path: &Path) -> Cow<'_, Path> {
    let Some(s) = path.as_os_str().to_str() else {
        // Wtf-8 paths that cannot round-trip through &str are left alone;
        // callers that produced them (canonicalize) never do.
        return Cow::Borrowed(path);
    };
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        // Re-attach the plain UNC double slash — owned, because the
        // prefix letters are interposed, not just sliced off.
        Cow::Owned(PathBuf::from(format!(r"\\{rest}")))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        // Plain slice: `\\?\C:\x` → `C:\x`.
        Cow::Borrowed(Path::new(rest))
    } else {
        Cow::Borrowed(path)
    }
}

/// Normalize a Windows drive letter to uppercase (`c:\x` → `C:\x`).
///
/// Mostly cosmetic for storage/dedup of display strings; containment
/// comparisons are case-insensitive on Windows anyway (see
/// [`is_within`]). No-op for UNC, relative and non-Windows paths.
pub fn normalize_drive_letter(path: &Path) -> PathBuf {
    let Some(s) = path.as_os_str().to_str() else {
        return path.to_path_buf();
    };
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let mut out = String::with_capacity(s.len());
        out.push(bytes[0].to_ascii_uppercase() as char);
        out.push_str(&s[1..]);
        PathBuf::from(out)
    } else {
        path.to_path_buf()
    }
}

/// Is `candidate` strictly *below* `root`?
///
/// Component-based (never string prefixes), strict-descendant (equal
/// paths are NOT within — rejecting over-broad roots per §16.1), and
/// platform-aware on case. Relative inputs are rejected: an unresolved
/// path cannot be proven to be inside anything.
///
/// Inputs are expected to be canonical (or at least absolute); pass
/// [`safe_canonicalize`] output here in security-relevant flows.
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let root = strip_verbatim(root);
    let candidate = strip_verbatim(candidate);

    let root_components: Vec<Component<'_>> = root.components().collect();
    let candidate_components: Vec<Component<'_>> = candidate.components().collect();

    // Both must be absolute (Windows prefix or Unix root first) —
    // relative paths are unresolvable without a base, fail closed.
    let absolute =
        |c: &Component<'_>| matches!(c, Component::Prefix(_)) || matches!(c, Component::RootDir);
    if root_components.is_empty()
        || candidate_components.is_empty()
        || !absolute(&root_components[0])
        || !absolute(&candidate_components[0])
    {
        return false;
    }

    // `.` / `..` components mean the path was never normalized —
    // refuse rather than re-implement lexical resolution.
    let unresolved = |c: &Component<'_>| matches!(c, Component::CurDir | Component::ParentDir);
    if root_components.iter().any(unresolved) || candidate_components.iter().any(unresolved) {
        return false;
    }

    // Walk the shared prefix. The root must consume a prefix + root-dir
    // pair (both `C:\`-style and `\\server\share\`-style start with a
    // Prefix and RootDir component in std's model).
    if root_components.len() > candidate_components.len() {
        return false;
    }
    for (r, c) in root_components.iter().zip(candidate_components.iter()) {
        match (r, c) {
            (Component::Prefix(_), Component::Prefix(_)) if prefix_eq(r, c) => {}
            (Component::RootDir, Component::RootDir) => {}
            (Component::Normal(rn), Component::Normal(cn)) if eq_component(rn, cn) => {}
            _ => return false,
        }
    }
    // Strict descendant: at least one component of the candidate must be
    // below the root. `root == candidate` (same length) is rejected.
    candidate_components.len() > root_components.len()
}

/// Windows-only: does the path contain a reserved device-name
/// component (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`)?
///
/// Windows 11 relaxed this for some forms; we keep the conservative
/// legacy set — refusing a reserved-looking name can only fail closed.
/// On non-Windows platforms this is always `false` (no such concept).
#[cfg(windows)]
#[must_use]
pub fn has_reserved_component(path: &Path) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name = name.to_string_lossy();
            // `CON.txt` is reserved too — only the stem matters.
            let stem = name.split('.').next().unwrap_or("");
            if RESERVED.contains(&stem.to_ascii_uppercase().as_str()) {
                return true;
            }
        }
    }
    false
}

#[cfg(not(windows))]
#[must_use]
pub fn has_reserved_component(_path: &Path) -> bool {
    // Reserved device names are a DOS/Windows artifact.
    false
}

/// Compare two `Component::Prefix` values (drive letters and UNC
/// server/share pairs), case-insensitively on Windows.
fn prefix_eq(a: &Component<'_>, b: &Component<'_>) -> bool {
    let (Component::Prefix(pa), Component::Prefix(pb)) = (a, b) else {
        return false;
    };
    // PrefixComponent wraps Prefix behind `.kind()`.
    use std::path::Prefix as P;
    match (pa.kind(), pb.kind()) {
        (P::Disk(d1), P::Disk(d2)) => d1.eq_ignore_ascii_case(&d2),
        (P::UNC(s1, sh1), P::UNC(s2, sh2)) => eq_component(s1, s2) && eq_component(sh1, sh2),
        // Verbatim forms should not survive strip_verbatim, but be
        // robust if a caller skipped it.
        (P::VerbatimDisk(d1), P::VerbatimDisk(d2)) => d1.eq_ignore_ascii_case(&d2),
        (P::VerbatimUNC(s1, sh1), P::VerbatimUNC(s2, sh2)) => {
            eq_component(s1, s2) && eq_component(sh1, sh2)
        }
        _ => false,
    }
}

/// Component equality, platform-aware on case (see module docs for the
/// direction of the macOS/Windows asymmetry and why it is safe).
#[cfg(windows)]
fn eq_component(a: &OsStr, b: &OsStr) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[cfg(not(windows))]
fn eq_component(a: &OsStr, b: &OsStr) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn win_root() -> PathBuf {
        PathBuf::from(r"C:\Users\x")
    }

    #[test]
    fn is_within_accepts_strict_descendant() {
        // Unix-style (also exercises the shared comparison core on
        // Windows since the logic is platform-agnostic apart from case).
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b/c")));
        assert!(is_within(Path::new("/"), Path::new("/Users")));
        // Multi-component descendant.
        assert!(is_within(Path::new("/a"), Path::new("/a/b/c/d")));
    }

    #[test]
    fn is_within_rejects_equal_sibling_and_prefix_trap() {
        // Equal is NOT within — roots themselves must never be targets.
        assert!(!is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(!is_within(Path::new("/"), Path::new("/")));
        // Sibling.
        assert!(!is_within(Path::new("/a/b"), Path::new("/a/c")));
        // The classic string-prefix trap: "/a/bc" is not inside "/a/b".
        assert!(!is_within(Path::new("/a/b"), Path::new("/a/bc")));
        // Candidate above root.
        assert!(!is_within(Path::new("/a/b"), Path::new("/a")));
        assert!(!is_within(Path::new("/a/b"), Path::new("/")));
    }

    #[test]
    fn is_within_rejects_relative_and_empty() {
        assert!(!is_within(Path::new("/a/b"), Path::new("c")));
        assert!(!is_within(Path::new("/a/b"), Path::new("")));
        assert!(!is_within(Path::new("./a"), Path::new("./a/b")));
        assert!(!is_within(Path::new("/a/.."), Path::new("/a/b/c")));
    }

    #[test]
    fn is_within_case_handling_follows_platform() {
        if cfg!(windows) {
            // NTFS: case-insensitive (WorkBuddy WorkBuddy/Workbuddy case).
            assert!(is_within(
                &PathBuf::from(r"C:\Users\x"),
                &PathBuf::from(r"c:\USERS\X\.claude")
            ));
        } else {
            // macOS/Unix: exact compare — a case-only difference is
            // treated as distinct (conservative; can only refuse).
            assert!(!is_within(Path::new("/a/b"), Path::new("/A/B/c")));
        }
    }

    #[cfg(windows)]
    #[test]
    fn is_within_windows_drive_and_verbatim() {
        assert!(is_within(&win_root(), &win_root().join(r".claude")));
        assert!(is_within(
            &win_root(),
            &PathBuf::from(r"\\?\C:\Users\x\.claude")
        ));
        assert!(is_within(
            &PathBuf::from(r"\\?\C:\Users\x"),
            &win_root().join(r".claude")
        ));
        // Drive must match, not just share the rootdir shape.
        assert!(!is_within(&win_root(), &PathBuf::from(r"D:\Users\x\y")));
        // Root of a drive: descendants only.
        assert!(is_within(&PathBuf::from(r"C:\"), &win_root()));
        assert!(!is_within(&PathBuf::from(r"C:\"), &PathBuf::from(r"C:\")));
    }

    #[cfg(windows)]
    #[test]
    fn is_within_unc_paths() {
        assert!(is_within(
            &PathBuf::from(r"\\server\share"),
            &PathBuf::from(r"\\server\share\dir")
        ));
        assert!(is_within(
            &PathBuf::from(r"\\?\UNC\server\share"),
            &PathBuf::from(r"\\server\share\dir")
        ));
        assert!(!is_within(
            &PathBuf::from(r"\\server\share"),
            &PathBuf::from(r"\\server\other\dir")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn reserved_names_are_detected() {
        assert!(has_reserved_component(Path::new(r"C:\x\CON")));
        assert!(has_reserved_component(Path::new(r"C:\x\con.txt")));
        assert!(has_reserved_component(Path::new(r"C:\x\com7")));
        assert!(!has_reserved_component(Path::new(r"C:\x\console")));
        assert!(!has_reserved_component(Path::new(r"C:\x\com10")));
        assert!(!has_reserved_component(Path::new(r"C:\x\normal.md")));
    }

    #[cfg(not(windows))]
    #[test]
    fn reserved_names_are_windows_only() {
        assert!(!has_reserved_component(Path::new("/x/CON")));
    }

    #[cfg(windows)]
    #[test]
    fn drive_letter_is_normalized() {
        assert_eq!(
            normalize_drive_letter(Path::new(r"c:\users\x")),
            PathBuf::from(r"C:\users\x")
        );
        // Already uppercase / no drive: unchanged.
        assert_eq!(
            normalize_drive_letter(Path::new(r"C:\x")),
            PathBuf::from(r"C:\x")
        );
        assert_eq!(
            normalize_drive_letter(Path::new(r"relative")),
            PathBuf::from("relative")
        );
    }

    #[cfg(windows)]
    #[test]
    fn strip_verbatim_forms() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\Users\x")).as_ref(),
            Path::new(r"C:\Users\x")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\srv\share\x")).as_ref(),
            Path::new(r"\\srv\share\x")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"C:\plain")).as_ref(),
            Path::new(r"C:\plain")
        );
    }

    #[test]
    fn safe_canonicalize_strips_verbatim_and_fails_closed() {
        let tmp = std::env::temp_dir().join(format!(
            "agenttidy-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("f.txt");
        std::fs::write(&file, b"x").unwrap();

        let canonical = safe_canonicalize(&file).unwrap();
        // On Windows the raw canonicalize output has a \\?\ prefix; the
        // safe form must not.
        assert!(
            !canonical.as_os_str().to_str().unwrap().starts_with(r"\\?\"),
            "verbatim prefix survived: {}",
            canonical.display()
        );
        assert!(canonical.ends_with("f.txt"));
        // The canonical file must be within its own parent — and the
        // parent must NOT be within itself.
        let parent = canonical.parent().unwrap();
        assert!(is_within(parent, &canonical));
        assert!(!is_within(parent, parent));

        // Non-existent path: error, never a guess (fail closed).
        let missing = tmp.join("missing.txt");
        assert!(safe_canonicalize(&missing).is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }
}
