//! Notifications per application (#899): the level the user chooses for
//! an application from its bubble or the history ("⋯", or a right click),
//! written into the configuration file; the applications that sent
//! notifications, counted per day and kept for the Settings' list; and the
//! sound a bubble asks for.
//!
//! The levels themselves are the stack's ([`super::stack`]): the
//! compositor tells them as the configuration says them, and a choice
//! made here applies at once, before the file says it back.
//!
//! The senders are kept in `$XDG_STATE_HOME/hocuspanda/notification-senders`
//! (JSON, readable by the user alone), only where notifications are served,
//! read back as the shell starts and written by the history's thread after
//! each change. Each is counted as it comes, whatever its level.

use std::sync::{Mutex, PoisonError};

use hocus_link::{NotifyLevel, NotifySenders};

use super::{Input, Write, input, send_write};

/// The senders, once read back; what came before, waiting.
#[derive(Debug)]
enum Senders {
    Reading(Vec<(String, String, Option<String>)>),
    Read(NotifySenders),
}

static SENDERS: Mutex<Senders> = Mutex::new(Senders::Reading(Vec::new()));

/// Today's number: days since 1970-01-01 on the wall clock's calendar.
pub fn today() -> i64 {
    day_of(super::wall().date())
}

/// `date`'s number of days since 1970-01-01.
fn day_of(date: jiff::civil::Date) -> i64 {
    date.to_zoned(jiff::tz::TimeZone::UTC)
        .map_or(0, |z| z.timestamp().as_second().div_euclid(86_400))
}

/// The senders' file, read as the shell starts (its text; empty: none).
pub(super) fn restore(text: &str) {
    let mut senders = NotifySenders::decode(text);
    let today = today();
    senders.prune(today);
    let waiting = {
        let mut kept = SENDERS.lock().unwrap_or_else(PoisonError::into_inner);
        let waiting = match &mut *kept {
            Senders::Reading(waiting) => std::mem::take(waiting),
            Senders::Read(_) => return,
        };
        for (id, name, icon) in &waiting {
            senders.count(id, name, icon.clone(), today);
        }
        *kept = Senders::Read(senders.clone());
        waiting
    };
    tracing::info!(
        senders = senders.apps.len(),
        came = waiting.len(),
        "the notifications' senders read"
    );
    if !waiting.is_empty() {
        send_write(Write::Senders(senders));
    }
}

/// Application `id` sent a notification, under `name`, its icon `icon` (a
/// name of the icon themes, or a path): counted for today.
pub fn counted(id: String, name: String, icon: Option<String>) {
    let written = {
        let mut kept = SENDERS.lock().unwrap_or_else(PoisonError::into_inner);
        match &mut *kept {
            Senders::Reading(waiting) => {
                waiting.push((id, name, icon));
                None
            }
            Senders::Read(senders) => {
                senders.count(&id, &name, icon, today());
                Some(senders.clone())
            }
        }
    };
    if let Some(senders) = written {
        send_write(Write::Senders(senders));
    }
}

/// Application `id`'s level, as the stack has it.
pub fn level_of(id: &str) -> NotifyLevel {
    super::with(|c| c.stack.level_of(id))
}

/// The user chose `level` for application `id` (named `name`) from a
/// bubble or the history: it applies at once, and is written into the
/// configuration file (`[notifications.apps]`, taken out for "all", what
/// applies to those not named), on a thread of its own.
pub fn choose(id: &str, name: &str, level: NotifyLevel) {
    tracing::info!(
        app = id,
        level = level.word(),
        "an application's level chosen"
    );
    input(Input::Level {
        id: id.to_owned(),
        level,
    });
    let Some(file) = hocus_config::file() else {
        tracing::warn!(
            app = id,
            "no configuration file: the application's level is not kept"
        );
        return;
    };
    let (id, name) = (id.to_owned(), name.to_owned());
    let wrote = std::thread::Builder::new()
        .name("notifications: a level".to_owned())
        .spawn(move || {
            let edited = hocus_config::edit(&file, |doc| set_level(doc, &id, level), |_| Ok(()));
            match edited {
                Ok(_) => tracing::info!(
                    app = id,
                    name,
                    level = level.word(),
                    "the application's level written"
                ),
                Err(err) => {
                    tracing::warn!(%err, app = id, "the application's level is not written");
                }
            }
        });
    if let Err(err) = wrote {
        tracing::warn!(%err, "cannot write the application's level");
    }
}

/// `doc` with application `id` at `level` in `[notifications.apps]`: its
/// key (or one naming it in another case) replaced; for "all", taken out,
/// and the tables it leaves empty with it.
fn set_level(
    doc: &mut hocus_config::toml_edit::DocumentMut,
    id: &str,
    level: NotifyLevel,
) -> Result<(), String> {
    use hocus_config::toml_edit::{Item, Table, value};
    let notifications = doc.entry("notifications").or_insert_with(|| {
        let mut made = Table::new();
        made.set_implicit(true);
        Item::Table(made)
    });
    let notifications = notifications
        .as_table_mut()
        .ok_or("[notifications] is not a table")?;
    let apps = notifications
        .entry("apps")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or("[notifications.apps] is not a table")?;
    let named: Vec<String> = apps
        .iter()
        .map(|(k, _)| k.to_owned())
        .filter(|k| k != id && k.eq_ignore_ascii_case(id))
        .collect();
    for other in named {
        apps.remove(&other);
    }
    if level == NotifyLevel::All {
        apps.remove(id);
        if apps.is_empty() {
            notifications.remove("apps");
        }
        if notifications.is_empty() {
            doc.remove("notifications");
        }
    } else {
        apps.insert(id, value(level.word()));
    }
    Ok(())
}

/// Plays `sound` (a file's path, else a name of the freedesktop sound
/// theme) on PipeWire, on a thread of its own: `pw-play`, never the shell's
/// loop.
pub fn play(sound: &str) {
    let sound = sound.to_owned();
    let started = std::thread::Builder::new()
        .name("notifications: a sound".to_owned())
        .spawn(move || {
            let file = if sound.starts_with('/') {
                Some(std::path::PathBuf::from(&sound))
            } else {
                hocus_sound::tester::theme_file(&sound)
            };
            let Some(file) = file.filter(|f| f.is_file()) else {
                tracing::debug!(sound, "a notification's sound not found");
                return;
            };
            let played = std::process::Command::new("pw-play")
                .arg(&file)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if let Err(err) = played {
                tracing::debug!(%err, "pw-play cannot play a notification's sound");
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot play a notification's sound");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_is_written_in_its_table_and_all_takes_it_out() -> Result<(), String> {
        let mut doc = hocus_config::parse("# Mine.\n[panel]\nform = \"full\"\n")
            .map_err(|e| e.to_string())?;
        set_level(&mut doc, "org.mozilla.firefox", NotifyLevel::None)?;
        set_level(&mut doc, "Slack", NotifyLevel::Silent)?;
        assert_eq!(
            doc.to_string(),
            "# Mine.\n[panel]\nform = \"full\"\n\n[notifications.apps]\n\
             \"org.mozilla.firefox\" = \"none\"\nSlack = \"silent\"\n"
        );
        // One written by hand in another case is replaced.
        let mut doc = hocus_config::parse("[notifications.apps]\nslack = \"none\"\n")
            .map_err(|e| e.to_string())?;
        set_level(&mut doc, "Slack", NotifyLevel::History)?;
        assert_eq!(
            doc.to_string(),
            "[notifications.apps]\nSlack = \"history\"\n"
        );
        // "all" takes it out, and the tables left empty.
        set_level(&mut doc, "Slack", NotifyLevel::All)?;
        assert_eq!(doc.to_string(), "");
        Ok(())
    }
}
