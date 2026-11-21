//! Blobs: a content-addressed side store for what is too big for a line — a file's text, a command's whole output.
//! An event carries the id and the JSONL never bloats. The id is the SHA-256 of the bytes as
//! stored, which are the bytes **after** redaction, so a blob never holds a secret any more than a line does.

use crate::redact::Redactor;
use crate::state::private_dir;
use crate::table::Problem;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The biggest blob.
pub const BLOB_MAX: usize = 64 << 20;

#[derive(Debug, Clone)]
pub struct Blobs {
    dir: PathBuf,
    red: Redactor,
}

/// A blob id is 64 lowercase hex digits: nothing else names a path here.
pub fn is_blob_id(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Blobs {
    pub fn new(dir: &Path, red: Redactor) -> Blobs {
        Blobs { dir: dir.to_path_buf(), red }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(&id[..2]).join(&id[2..])
    }

    /// Store `bytes` (redacted) and give their id. The same bytes are stored once.
    pub fn put(&self, bytes: &[u8]) -> Result<String, Problem> {
        let at = || format!("blobs {}", self.dir.display());
        if bytes.len() > BLOB_MAX {
            return Err(Problem::refused(at(), format!("{} bytes is more than a blob holds ({BLOB_MAX})", bytes.len())));
        }
        let clean = self.red.scrub_bytes(bytes);
        let id = crate::text::hex_hash(&clean);
        let path = self.path(&id);
        if path.is_file() {
            return Ok(id);
        }
        let io = |e: std::io::Error| Problem::refused(at(), format!("cannot store a blob: {e}"));
        let parent = path.parent().unwrap_or(&self.dir);
        private_dir(parent).map_err(io)?;
        static N: AtomicU64 = AtomicU64::new(0);
        let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let mut f = File::options().write(true).create_new(true).mode(0o600).open(&tmp).map_err(io)?;
        let written = f.write_all(&clean).and_then(|()| f.sync_all());
        drop(f);
        if let Err(e) = written.and_then(|()| fs::rename(&tmp, &path)) {
            let _ = fs::remove_file(&tmp);
            return Err(io(e));
        }
        Ok(id)
    }

    pub fn has(&self, id: &str) -> bool {
        is_blob_id(id) && self.path(id).is_file()
    }

    /// The bytes of blob `id`, checked against the id: a blob that has changed on disk is an error, not data.
    pub fn get(&self, id: &str) -> Result<Vec<u8>, Problem> {
        let at = format!("blob {id}");
        if !is_blob_id(id) {
            return Err(Problem::refused(at, "not a blob id (64 lowercase hex digits)"));
        }
        let bytes = fs::read(self.path(id)).map_err(|e| Problem::refused(at.clone(), format!("cannot read: {e}")))?;
        if crate::text::hex_hash(&bytes) != id {
            return Err(Problem::refused(at, "its bytes no longer match its id — the file was changed"));
        }
        Ok(bytes)
    }
}
