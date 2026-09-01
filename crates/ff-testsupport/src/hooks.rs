//! Installing git hooks into a fixture, and reading back what one saw.
//!
//! Three suites assert on fufu's hook behavior — the close, the absorb, the
//! session landing — and they need the same two things: a hook on disk, and
//! a record of what git thought was staged when it ran.

use crate::Fixture;

/// Write an executable hook of `name` into the fixture's `.git/hooks`.
pub fn install_hook(fx: &Fixture, name: &str, body: &str) {
    let dir = fx.path().join(".git/hooks");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    // Windows has no exec bit; hook discovery there is existence-only,
    // and the `#!/bin/sh` body runs via Git Bash's sh, as under git itself.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Records what git thinks is staged at hook time, into the git dir so the
/// marker never becomes part of the open change. This is exactly lefthook's
/// `{staged_files}`.
pub const STAGED_HOOK: &str = "#!/bin/sh\ngit diff --name-only --cached --diff-filter=ACMR \
                               > \"$(git rev-parse --git-dir)/staged.txt\"\n";

/// What [`STAGED_HOOK`] recorded, one path per line.
pub fn staged_marker(fx: &Fixture) -> Vec<String> {
    let text = std::fs::read_to_string(fx.path().join(".git/staged.txt"))
        .expect("the hook ran and recorded what was staged");
    text.lines().map(String::from).collect()
}
