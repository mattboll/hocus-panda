//! Who the user is, for the footer's avatar and the session's popover
//! (5.5): their name as their account's entry says it, the first part of
//! its GECOS field ("Alex Martin,,," gives "Alex Martin"), else their
//! login.
//!
//! The entry is asked of the system's databases (`getent passwd UID`, so
//! that a directory's accounts answer too), else read in `/etc/passwd`,
//! on a thread of its own as the shell starts: a directory may take its
//! time. Until it answers, the login the session gives (`USER`) stands in.
//! `HOCUS_USER_NAME` names the user instead, for recordings and tests, as
//! `HOCUS_CLOCK_AT` sets the clock's time.

use std::sync::{Mutex, PoisonError};

static NAME: Mutex<Option<String>> = Mutex::new(None);

/// Looks the user's name up, off the loop.
pub fn start() {
    if let Some(name) = std::env::var("HOCUS_USER_NAME")
        .ok()
        .filter(|n| !n.trim().is_empty())
    {
        set(name.trim().to_owned());
        return;
    }
    let looked = std::thread::Builder::new()
        .name("user".to_owned())
        .spawn(|| {
            let uid = rustix::process::getuid().as_raw();
            match look_up(uid) {
                Some(name) => {
                    tracing::debug!("the user's name found");
                    set(name);
                    // The footer's avatar, and the session's popover if it
                    // shows, draw it.
                    crate::popover::tell(hocus_link::ToCompositor::Wake(
                        hocus_link::FOOTER_NAMESPACE.to_owned(),
                    ));
                    crate::popover::changed(crate::popover::Item::User);
                }
                None => tracing::info!(uid, "no account entry: the login stands for the name"),
            }
        });
    if let Err(err) = looked {
        tracing::warn!(%err, "cannot look the user's name up");
    }
}

fn set(name: String) {
    *NAME.lock().unwrap_or_else(PoisonError::into_inner) = Some(name);
}

/// The user's name: their entry's, else their login, else nothing.
pub fn name() -> String {
    let found = NAME.lock().unwrap_or_else(PoisonError::into_inner).clone();
    found
        .or_else(|| {
            ["USER", "LOGNAME"]
                .into_iter()
                .filter_map(|v| std::env::var(v).ok())
                .find(|v| !v.is_empty())
        })
        .unwrap_or_default()
}

/// The avatar's letter: the name's first, in capitals; `?` without one.
pub fn initial(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

/// The name in account `uid`'s entry, from the system's databases, else
/// from `/etc/passwd`.
fn look_up(uid: u32) -> Option<String> {
    let asked = std::process::Command::new("getent")
        .args(["passwd", &uid.to_string()])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    if let Ok(out) = asked
        && out.status.success()
        && let Some(name) = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| name_in(line, uid))
    {
        return Some(name);
    }
    std::fs::read_to_string("/etc/passwd")
        .ok()?
        .lines()
        .find_map(|line| name_in(line, uid))
}

/// The name an entry (`login:x:uid:gid:gecos:home:shell`) gives, if it is
/// account `uid`'s: its GECOS field's first part, else its login.
fn name_in(line: &str, uid: u32) -> Option<String> {
    let fields: Vec<&str> = line.split(':').collect();
    let [login, _, id, _, gecos, ..] = fields.as_slice() else {
        return None;
    };
    if id.parse::<u32>().ok()? != uid {
        return None;
    }
    let full = gecos.split(',').next().unwrap_or_default().trim();
    let name = if full.is_empty() { login.trim() } else { full };
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_the_entrys_gecos_else_its_login() {
        assert_eq!(
            name_in("alex:x:1000:1000:Alex Martin,,,:/home/alex:/bin/bash", 1000).as_deref(),
            Some("Alex Martin")
        );
        assert_eq!(
            name_in("alex:x:1000:1000::/home/alex:/bin/sh", 1000).as_deref(),
            Some("alex")
        );
        assert_eq!(
            name_in("alex:x:1000:1000: ,,,:/home/alex:/bin/sh", 1000).as_deref(),
            Some("alex")
        );
        // Another account's, or a line that is not an entry.
        assert_eq!(name_in("root:x:0:0:root:/root:/bin/sh", 1000), None);
        assert_eq!(name_in("# a comment", 1000), None);
    }

    #[test]
    fn the_avatar_shows_the_names_first_letter_in_capitals() {
        assert_eq!(initial("alex"), "A");
        assert_eq!(initial("Élodie"), "É");
        assert_eq!(initial(" (zoé)"), "Z");
        assert_eq!(initial(""), "?");
    }
}
