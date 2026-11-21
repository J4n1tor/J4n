//! J4n's own state directory, `~/.j4n` or `<project>/.j4n`, and what it must not do to the work around it: a project's `.j4n/` writes its own `.gitignore` (`*`) when it is made, so `git status`
//! stays clean and a check that lists the folder does not see J4n's logs.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// What a project's `.gitignore` holds.
pub const GITIGNORE: &str = "*\n";

/// Make `dir` if it is not there. A project's directory that did not exist gets its `.gitignore`; one that did is left as
/// it is — somebody made it, and what it ignores is theirs to say.
pub fn ensure_root(dir: &Path, project: bool) -> io::Result<()> {
    let existed = dir.is_dir();
    fs::create_dir_all(dir)?;
    if project && !existed {
        match File::options().write(true).create_new(true).open(dir.join(".gitignore")) {
            Ok(mut f) => f.write_all(GITIGNORE.as_bytes())?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A state directory (log, folds, blobs) that only its owner can read: it holds the operator's conversations.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

/// The key a project's per-user state is filed under (`<home>/checkpoints/<key>/`): the first 16 hex digits of the SHA-256
/// of its root path.
pub fn project_key(root: &Path) -> String {
    crate::change::digest(&root.to_string_lossy())[..16].to_string()
}
