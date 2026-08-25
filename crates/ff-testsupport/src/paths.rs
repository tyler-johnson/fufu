//! Comparing a path a test built against a path something else printed.
//!
//! Three writers spell the same directory three ways. fufu records the
//! resolved path, so on macOS a fixture's `/var/folders/…` comes back as
//! `/private/var/folders/…`. git prints forward slashes on Windows whatever
//! separator it was handed. A test that compares either against
//! `Path::display` passes on Linux and nowhere else, which is how a landing
//! reaches CI green on one platform and broken on the other two.

use std::path::Path;

/// A path spelled the way fufu records and prints it.
pub fn real(path: &Path) -> String {
    ff_core::linked::path::real(path).display().to_string()
}

/// Whether a whole field — one line, one `--porcelain` value — is this path.
///
/// Prefer this to [`names`] wherever a substring match could succeed on the
/// wrong answer: `…/bay/.git` contains `…/bay`, so a `contains` assertion
/// about a worktree's path passes on output that names the admin file
/// instead, which is the exact bug git's separator handling produces.
pub fn is(field: &str, path: &Path) -> bool {
    slashes(field.trim()) == slashes(&real(path))
}

/// Whether some output names this path, whatever separator its writer chose.
pub fn names(haystack: &str, path: &Path) -> bool {
    let real = real(path);
    haystack.contains(&real) || slashes(haystack).contains(&slashes(&real))
}

fn slashes(text: &str) -> String {
    text.replace('\\', "/")
}
