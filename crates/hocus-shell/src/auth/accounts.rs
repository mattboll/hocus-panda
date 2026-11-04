//! Who is who, for the dialog: the accounts polkit offers (a user by its
//! id; a group by its members), their names, and the asking process (its
//! parents, its program, who started it). Read on the agent's thread, from
//! `/etc/passwd`, `/etc/group` and `/proc`, never on the one that draws.
//! `HOCUS_AUTH_ETC` names another folder than `/etc` for both files, for
//! tests.

use std::path::{Path, PathBuf};

/// An account that can answer: its id, its login, and the name people
/// know it by (its GECOS field's first part, else its login).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub uid: u32,
    pub login: String,
    pub name: String,
}

/// A line of `/etc/passwd`: its login, its id, its primary group, and its
/// name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    login: String,
    uid: u32,
    gid: u32,
    name: String,
}

fn etc() -> PathBuf {
    std::env::var_os("HOCUS_AUTH_ETC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc"))
}

fn passwd(text: &str) -> Vec<Entry> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split(':');
            let login = fields.next()?.to_owned();
            let _password = fields.next()?;
            let uid = fields.next()?.parse().ok()?;
            let gid = fields.next()?.parse().ok()?;
            let gecos = fields.next().unwrap_or_default();
            let name = gecos.split(',').next().unwrap_or_default().trim();
            let name = if name.is_empty() {
                login.clone()
            } else {
                name.to_owned()
            };
            Some(Entry {
                login,
                uid,
                gid,
                name,
            })
        })
        .collect()
}

/// `/etc/group`'s groups: their id and their members' logins.
fn groups(text: &str) -> Vec<(u32, Vec<String>)> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split(':');
            let _name = fields.next()?;
            let _password = fields.next()?;
            let gid = fields.next()?.parse().ok()?;
            let members = fields
                .next()
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_owned)
                .collect();
            Some((gid, members))
        })
        .collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// What polkit offers to answer with, as it says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    User(u32),
    Group(u32),
}

/// The accounts for `identities`, in polkit's order, each once: a group
/// stands for its members (named in `/etc/group`, or whose primary group
/// it is). A user without an entry is left out: the helper needs a login.
pub fn accounts(identities: &[Identity]) -> Vec<Account> {
    let dir = etc();
    let entries = passwd(&read(&dir.join("passwd")));
    let groups = groups(&read(&dir.join("group")));
    let account = |e: &Entry| Account {
        uid: e.uid,
        login: e.login.clone(),
        name: e.name.clone(),
    };
    let mut out: Vec<Account> = Vec::new();
    let mut add = |a: Account| {
        if !out.iter().any(|o| o.uid == a.uid) {
            out.push(a);
        }
    };
    for identity in identities {
        match identity {
            Identity::User(uid) => {
                if let Some(e) = entries.iter().find(|e| e.uid == *uid) {
                    add(account(e));
                }
            }
            Identity::Group(gid) => {
                let named = groups
                    .iter()
                    .filter(|(g, _)| g == gid)
                    .flat_map(|(_, m)| m.iter());
                for login in named {
                    if let Some(e) = entries.iter().find(|e| &e.login == login) {
                        add(account(e));
                    }
                }
                for e in entries.iter().filter(|e| e.gid == *gid) {
                    add(account(e));
                }
            }
        }
    }
    out
}

/// The name of the account `uid`, else its number.
pub fn name_of(uid: u32) -> String {
    let entries = passwd(&read(&etc().join("passwd")));
    entries
        .iter()
        .find(|e| e.uid == uid)
        .map_or_else(|| uid.to_string(), |e| e.name.clone())
}

/// Process `pid`'s parent, from `/proc/PID/stat` (after the name's `)`).
fn parent(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, after) = stat.rsplit_once(')')?;
    after.split_whitespace().nth(1)?.parse().ok()
}

/// `pid`, then its parents, nearest first, up to but without process 1.
pub fn ancestry(pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut at = pid;
    while at > 1 && out.len() < 64 && !out.contains(&at) {
        out.push(at);
        match parent(at) {
            Some(p) => at = p,
            None => break,
        }
    }
    out
}

/// Who started process `pid` (its real user's id).
pub fn owner(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Process `pid`'s program: its executable, else its command's first
/// word, else its name.
pub fn program(pid: u32) -> Option<String> {
    if let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        return Some(exe.display().to_string());
    }
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let first = cmdline.split(|b| *b == 0).next().unwrap_or_default();
    if !first.is_empty() {
        return Some(String::from_utf8_lossy(first).into_owned());
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|c| c.trim().to_owned())
        .filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n\
        alex:x:1000:1000:Alex Martin,,,:/home/alex:/bin/bash\n\
        sam:x:1001:1001::/home/sam:/bin/bash\n\
        # a comment\n\
        broken line\n";
    const GROUP: &str = "root:x:0:\nsudo:x:27:alex,sam\nalex:x:1000:\n";

    #[test]
    fn its_names_and_a_groups_members() {
        let entries = passwd(PASSWD);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].name, "Alex Martin");
        assert_eq!(entries[2].name, "sam");
        let groups = groups(GROUP);
        assert_eq!(groups[1], (27, vec!["alex".to_owned(), "sam".to_owned()]));
    }

    #[test]
    fn the_parents_of_this_process() {
        let me = std::process::id();
        let chain = ancestry(me);
        assert_eq!(chain.first(), Some(&me));
        assert!(!chain.contains(&1));
        assert_eq!(owner(me), Some(rustix::process::getuid().as_raw()));
        assert!(program(me).is_some());
    }
}
