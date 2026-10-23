//! What document search never reads, whatever the configuration says
//! (docs/robustness.md, "Search never reads secrets"): keys, keyrings,
//! password stores, cloud credentials, browser profiles. Checked on a
//! name before anything else is asked of it, its type included: a denied
//! entry is never opened, not even for its first bytes.

use std::path::{Component, Path};

/// Places under the home never read, as paths from it.
const HOME: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".local/share/keyrings",
    ".local/share/kwalletd",
    ".password-store",
    ".aws",
    ".kube",
    ".azure",
    ".docker",
    ".config/gcloud",
    ".config/gh",
    ".config/Bitwarden",
    ".config/1Password",
    ".1password",
    ".config/keepassxc",
    ".local/share/keepassxc",
    // Browser and mail profiles: cookies, saved passwords, sessions.
    ".mozilla",
    ".thunderbird",
    ".librewolf",
    ".waterfox",
    ".config/google-chrome",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/vivaldi",
    ".config/microsoft-edge",
    ".config/opera",
    ".var/app/org.mozilla.firefox",
    ".var/app/org.mozilla.Thunderbird",
    ".var/app/com.google.Chrome",
    ".var/app/org.chromium.Chromium",
    ".var/app/com.brave.Browser",
    ".var/app/com.bitwarden.desktop",
    ".var/app/org.keepassxc.KeePassXC",
    "snap/firefox",
    "snap/chromium",
    "snap/thunderbird",
];

/// What a hidden directory's name holds when it keeps secrets.
const SECRET_WORDS: &[&str] = &[
    "ssh",
    "gnupg",
    "gpg",
    "pass",
    "secret",
    "keyring",
    "kwallet",
    "wallet",
    "vault",
    "credential",
    "token",
    "key",
    "cert",
    "pki",
    "aws",
    "kube",
    "gcloud",
    "azure",
    "docker",
    "mozilla",
    "thunderbird",
    "chrome",
    "chromium",
    "bitwarden",
    "1password",
];

/// Names never read, in lower case.
const NAMES: &[&str] = &[
    ".netrc",
    "_netrc",
    ".pgpass",
    ".git-credentials",
    ".htpasswd",
    ".npmrc",
    ".pypirc",
    ".vault-token",
    "known_hosts",
    "authorized_keys",
    "wallet.dat",
];

/// Starts of names never read, in lower case.
const PREFIXES: &[&str] = &[".env", "id_", "credentials", "secrets"];

/// Extensions never read, in lower case.
const EXTENSIONS: &[&str] = &[
    "pem",
    "key",
    "p12",
    "pfx",
    "p8",
    "kdbx",
    "kdb",
    "jks",
    "keystore",
    "keychain",
    "keychain-db",
    "ppk",
    "gpg",
    "pgp",
    "asc",
    "age",
    "ovpn",
];

/// Whether an entry called `name` is never read, a file or a directory.
pub fn denied_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if NAMES.contains(&lower.as_str()) || PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return true;
    }
    if let Some((stem, extension)) = lower.rsplit_once('.')
        && !stem.is_empty()
        && EXTENSIONS.contains(&extension)
    {
        return true;
    }
    // A hidden directory (or file) named for what it keeps.
    lower
        .strip_prefix('.')
        .is_some_and(|rest| SECRET_WORDS.iter().any(|w| rest.contains(w)))
}

/// Whether `path` (absolute) is never read: one of its parts has a denied
/// name, or it is in a denied place of `home`.
pub fn denied(path: &Path, home: Option<&Path>) -> bool {
    let named = path.components().any(|c| match c {
        Component::Normal(name) => denied_name(&name.to_string_lossy()),
        _ => false,
    });
    named || home.is_some_and(|home| HOME.iter().any(|place| path.starts_with(home.join(place))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_keyrings_and_password_stores_are_denied_by_name() {
        for name in [
            ".ssh",
            ".gnupg",
            ".password-store",
            ".env",
            ".env.local",
            ".envrc",
            "id_ed25519",
            "id_rsa.pub",
            "server.pem",
            "server.KEY",
            "cert.p12",
            "Passwords.kdbx",
            "credentials",
            "credentials.json",
            "secrets.yaml",
            "Secrets",
            ".netrc",
            ".my-vault",
            ".work-keys",
            "known_hosts",
        ] {
            assert!(denied_name(name), "{name} is denied");
        }
        for name in [
            "Report.odt",
            "notes.md",
            "keynote.pdf",
            ".pem",
            "monkey.txt",
            "environment.md",
            ".config",
            ".local",
            "Documents",
        ] {
            assert!(!denied_name(name), "{name} is not");
        }
    }

    #[test]
    fn places_of_the_home_are_denied_with_all_they_hold() {
        let home = Path::new("/home/alex");
        for path in [
            "/home/alex/.ssh/config",
            "/home/alex/.local/share/keyrings/login.keyring",
            "/home/alex/.mozilla/firefox/abc.default/places.sqlite",
            "/home/alex/.config/google-chrome/Default/Cookies",
            "/home/alex/.config/gcloud/application_default_credentials.json",
            "/home/alex/.aws/config",
            "/home/alex/Documents/.ssh/notes.txt",
            "/home/alex/Documents/project/.env",
        ] {
            assert!(denied(Path::new(path), Some(home)), "{path} is denied");
        }
        for path in [
            "/home/alex/Documents/Report.odt",
            "/home/alex/.config/hocuspanda/notes.md",
            "/home/alex/Desktop/keynote.pdf",
        ] {
            assert!(!denied(Path::new(path), Some(home)), "{path} is not");
        }
    }
}
