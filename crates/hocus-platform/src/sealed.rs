//! A file kept encrypted at rest (#71): the clipboard history. The key is a
//! file of its own beside it, readable by the user only: a copy of the
//! sealed file (a backup, a synced folder) is unreadable without it. It
//! does not protect the account from itself.
//!
//! Reading and writing are the caller's to do off the loop.

use std::{
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload},
};

const NONCE: usize = 12;
const KEY: usize = 32;
/// What is sealed is bound to this, so a file of another kind is refused.
const AAD: &[u8] = b"hocuspanda sealed v1";

/// A sealed file and its key.
#[derive(Debug, Clone)]
pub struct Sealed {
    path: PathBuf,
    key: PathBuf,
}

impl Sealed {
    /// `path`, sealed with the key at `path` + `.key`.
    pub fn new(path: &Path) -> Self {
        let mut key = path.as_os_str().to_owned();
        key.push(".key");
        Self {
            path: path.to_path_buf(),
            key: PathBuf::from(key),
        }
    }

    /// The opened content; `None` when there is no file yet.
    pub fn read(&self) -> Result<Option<Vec<u8>>, String> {
        let sealed = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", self.path.display())),
        };
        let key = std::fs::read(&self.key).map_err(|e| format!("{}: {e}", self.key.display()))?;
        let cipher =
            ChaCha20Poly1305::new_from_slice(&key).map_err(|_| "a key of the wrong size")?;
        if sealed.len() < NONCE {
            return Err("a sealed file too short".into());
        }
        let (nonce, body) = sealed.split_at(NONCE);
        let nonce = nonce.try_into().map_err(|_| "a nonce of the wrong size")?;
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: body,
                    aad: AAD,
                },
            )
            .map(Some)
            .map_err(|_| format!("{}: could not be opened", self.path.display()))
    }

    /// Seals `content` into the file, replacing it at once; the key is
    /// made the first time.
    pub fn write(&self, content: &[u8]) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let key = match std::fs::read(&self.key) {
            Ok(key) if key.len() == KEY => key,
            _ => self.make_key()?,
        };
        let cipher =
            ChaCha20Poly1305::new_from_slice(&key).map_err(|_| "a key of the wrong size")?;
        let mut nonce = [0u8; NONCE];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let body = cipher
            .encrypt(
                (&nonce).into(),
                Payload {
                    msg: content,
                    aad: AAD,
                },
            )
            .map_err(|_| "could not seal")?;
        let tmp = self.path.with_extension("new");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        file.write_all(&nonce)
            .and_then(|()| file.write_all(&body))
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("{}: {e}", self.path.display()))
    }

    fn make_key(&self) -> Result<Vec<u8>, String> {
        let mut key = vec![0u8; KEY];
        getrandom::fill(&mut key).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.key)
            .map_err(|e| format!("{}: {e}", self.key.display()))?;
        file.write_all(&key)
            .map_err(|e| format!("{}: {e}", self.key.display()))?;
        // Whatever the umask made of an older one.
        std::fs::set_permissions(&self.key, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", self.key.display()))?;
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_sealed_opens_with_its_key_only() {
        let dir = tempfile::tempdir().expect("a directory");
        let sealed = Sealed::new(&dir.path().join("clipboard"));
        assert_eq!(sealed.read(), Ok(None));
        sealed.write(b"hunter2 is not kept").expect("sealed");
        let raw = std::fs::read(dir.path().join("clipboard")).expect("written");
        assert!(!raw.windows(7).any(|w| w == b"hunter2"), "stored in clear");
        assert_eq!(sealed.read(), Ok(Some(b"hunter2 is not kept".to_vec())));
        let mode = std::fs::metadata(dir.path().join("clipboard.key"))
            .expect("a key")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        // Another key does not open it.
        std::fs::write(dir.path().join("clipboard.key"), [7u8; KEY]).expect("written");
        assert!(sealed.read().is_err());
    }
}
