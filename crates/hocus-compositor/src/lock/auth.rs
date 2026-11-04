//! Who checks the lock's password: PAM, always, in the program people
//! run; a test's password in the compositor's own tests only.
//!
//! The test authenticator exists only when the crate is built with its
//! `test-auth` feature, which its own tests turn on (a dev-dependency of
//! the crate on itself) and nothing else: a release build
//! (`cargo build --release -p hocus-compositor`, the package's) has no
//! such code, which `packaging/deb/build.sh` checks in the binary
//! (docs/design/lock.md, "The test authenticator").
//!
//! Each check runs on a thread of its own, the password moved there and
//! zeroed as it goes; its outcome comes back to the loop.

use zeroize::Zeroizing;

/// PAM's service: the package's own file (`/etc/pam.d/hocuspanda`), else
/// the system's login, which every distribution has.
const SERVICE: &str = "hocuspanda";
const FALLBACK: &str = "login";
/// Where PAM looks for its services' files.
const SERVICE_DIRS: [&str; 3] = ["/etc/pam.d", "/usr/lib/pam.d", "/usr/share/pam.d"];

/// What a check says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Right,
    Wrong,
    /// PAM said there were too many attempts (pam_faillock): the field
    /// waits.
    TooMany,
    /// PAM could not check (no libpam, a broken service): the session
    /// stays locked, as for a wrong password.
    Failed(String),
}

/// Who checks.
#[derive(Debug, Clone, Default)]
pub enum Authenticator {
    #[default]
    Pam,
    /// The tests' own, never PAM nor anyone's password: `password` is
    /// right; `deny` wrong ones in a row and the next check is refused as
    /// pam_faillock refuses, then counting starts again.
    #[cfg(feature = "test-auth")]
    Test {
        password: String,
        deny: u32,
        tally: std::sync::Arc<std::sync::Mutex<u32>>,
    },
}

impl PartialEq for Authenticator {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pam, Self::Pam) => true,
            #[cfg(feature = "test-auth")]
            (Self::Test { password: a, .. }, Self::Test { password: b, .. }) => a == b,
            #[cfg(feature = "test-auth")]
            _ => false,
        }
    }
}

impl Authenticator {
    /// The tests' authenticator: `password` right, three wrong in a row
    /// and the next attempt is refused.
    #[cfg(feature = "test-auth")]
    pub fn test(password: &str) -> Self {
        Self::Test {
            password: password.to_owned(),
            deny: 3,
            tally: std::sync::Arc::default(),
        }
    }

    /// `HOCUS_TEST_AUTH=PASSWORD` in a build with the `test-auth` feature
    /// (a nested or headless compositor run by hand, for screenshots);
    /// PAM otherwise, and always without the feature.
    pub fn from_env() -> Self {
        #[cfg(feature = "test-auth")]
        if let Ok(password) = std::env::var("HOCUS_TEST_AUTH")
            && !password.is_empty()
        {
            tracing::warn!("HOCUS_TEST_AUTH: the lock takes a test password, not PAM");
            return Self::test(&password);
        }
        Self::Pam
    }

    /// Checks `password` for `user`, on this thread.
    pub fn check(&self, user: &str, password: &Zeroizing<String>) -> Outcome {
        match self {
            Self::Pam => super::pam::check(&service(), user, password),
            #[cfg(feature = "test-auth")]
            Self::Test {
                password: right,
                deny,
                tally,
            } => {
                let mut wrong = tally
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if *wrong >= *deny {
                    *wrong = 0;
                    return Outcome::TooMany;
                }
                if password.as_str() == right {
                    *wrong = 0;
                    Outcome::Right
                } else {
                    *wrong += 1;
                    Outcome::Wrong
                }
            }
        }
    }
}

/// PAM's service: the package's when its file is there.
fn service() -> String {
    let ours = SERVICE_DIRS
        .iter()
        .any(|dir| std::path::Path::new(dir).join(SERVICE).exists());
    if ours {
        SERVICE.to_owned()
    } else {
        tracing::info!(
            "no PAM service {SERVICE} (the package installs it): the lock checks with {FALLBACK}'s"
        );
        FALLBACK.to_owned()
    }
}

/// The login of the user the compositor runs as, from the system's
/// account databases; `None`: not found, and the lock cannot open.
pub fn user() -> Option<String> {
    let uid = rustix::process::getuid().as_raw();
    let mut buffer = vec![0u8; 4096];
    loop {
        match account_name(uid, &mut buffer) {
            Lookup::Found(name) => return Some(name),
            Lookup::Missing => return None,
            Lookup::Small if buffer.len() < 1 << 20 => buffer.resize(buffer.len() * 2, 0),
            Lookup::Small => return None,
        }
    }
}

enum Lookup {
    Found(String),
    Missing,
    Small,
}

#[allow(unsafe_code)]
fn account_name(uid: u32, buffer: &mut [u8]) -> Lookup {
    // SAFETY: `getpwuid_r` fills `entry` with pointers into `buffer`,
    // which outlives their use here; `found` says whether it did.
    unsafe {
        let mut entry: libc::passwd = std::mem::zeroed();
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        );
        if rc == libc::ERANGE {
            return Lookup::Small;
        }
        if rc != 0 || found.is_null() || entry.pw_name.is_null() {
            return Lookup::Missing;
        }
        let name = std::ffi::CStr::from_ptr(entry.pw_name);
        match name.to_str() {
            Ok(name) if !name.is_empty() => Lookup::Found(name.to_owned()),
            _ => Lookup::Missing,
        }
    }
}

#[cfg(all(test, feature = "test-auth"))]
mod tests {
    use super::*;

    #[test]
    fn the_test_authenticator_refuses_once_after_three_wrong() {
        let auth = Authenticator::test("panda");
        let typed = |s: &str| Zeroizing::new(s.to_owned());
        assert_eq!(auth.check("alex", &typed("koala")), Outcome::Wrong);
        assert_eq!(auth.check("alex", &typed("panda")), Outcome::Right);
        for _ in 0..3 {
            assert_eq!(auth.check("alex", &typed("koala")), Outcome::Wrong);
        }
        assert_eq!(auth.check("alex", &typed("panda")), Outcome::TooMany);
        assert_eq!(auth.check("alex", &typed("panda")), Outcome::Right);
    }
}
