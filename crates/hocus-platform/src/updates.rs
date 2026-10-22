//! System updates (#84): which package manager the system has, what it
//! would update, and installing it through polkit (`pkexec`, answered by
//! the session's own dialog). Everything here blocks: the compositor runs
//! it on worker threads.

use std::{
    path::Path,
    process::{Command, Stdio},
};

/// An update waiting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub name: String,
    /// The version installed, when the manager says.
    pub from: Option<String>,
    pub to: String,
}

/// How updates are found and installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manager {
    Apt,
    Dnf,
    /// `checkupdates` (pacman-contrib) to look without root.
    Pacman,
    /// FreeBSD's pkg.
    Pkg,
    /// Commands of the user's own: `check` prints `NAME VERSION` (or
    /// `NAME OLD -> NEW`) per update; `install` installs them all.
    Custom {
        check: String,
        install: String,
    },
}

/// Whether `program` is on the `PATH`.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// The system's package manager, if it has one we know.
pub fn detect() -> Option<Manager> {
    if on_path("apt-get") && on_path("apt") {
        Some(Manager::Apt)
    } else if on_path("dnf") {
        Some(Manager::Dnf)
    } else if on_path("pacman") && on_path("checkupdates") {
        Some(Manager::Pacman)
    } else if on_path("pkg") && Path::new("/usr/sbin/pkg").exists() {
        Some(Manager::Pkg)
    } else {
        None
    }
}

/// Runs `argv`; its output, or what it said went wrong.
fn run(argv: &[&str], ok_codes: &[i32]) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("no command")?;
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    let code = out.status.code().unwrap_or(-1);
    if out.status.success() || ok_codes.contains(&code) {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let said = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned();
        Err(if said.is_empty() {
            format!("{program} ended with {}", out.status)
        } else {
            said
        })
    }
}

/// A command line split on spaces.
fn words(command: &str) -> Vec<&str> {
    command.split_whitespace().collect()
}

/// What would be updated now.
pub fn check(manager: &Manager) -> Result<Vec<Update>, String> {
    match manager {
        Manager::Apt => run(&["apt", "list", "--upgradable"], &[]).map(|o| parse_apt(&o)),
        // 100: updates there are.
        Manager::Dnf => run(&["dnf", "-q", "check-update"], &[100]).map(|o| parse_dnf(&o)),
        // 2: none.
        Manager::Pacman => run(&["checkupdates"], &[2]).map(|o| parse_arrows(&o)),
        Manager::Pkg => run(&["pkg", "version", "-vRL="], &[]).map(|o| parse_pkg(&o)),
        Manager::Custom { check, .. } => run(&words(check), &[]).map(|o| parse_arrows(&o)),
    }
}

/// Installs every update, as root through polkit; what went wrong, in
/// the manager's words.
pub fn install(manager: &Manager) -> Result<(), String> {
    let argv: Vec<&str> = match manager {
        Manager::Apt => vec!["pkexec", "apt-get", "-y", "upgrade"],
        Manager::Dnf => vec!["pkexec", "dnf", "-y", "upgrade"],
        Manager::Pacman => vec!["pkexec", "pacman", "-Syu", "--noconfirm"],
        Manager::Pkg => vec!["pkexec", "pkg", "upgrade", "-y"],
        Manager::Custom { install, .. } => words(install),
    };
    run(&argv, &[]).map(drop)
}

/// `apt list --upgradable`: `name/suite version arch [upgradable from: old]`.
pub fn parse_apt(out: &str) -> Vec<Update> {
    out.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once('/')?;
            let mut fields = rest.split_whitespace();
            let to = fields.nth(1)?.to_owned();
            let from = l
                .split_once("upgradable from: ")
                .map(|(_, f)| f.trim_end_matches(']').trim().to_owned());
            Some(Update {
                name: name.to_owned(),
                from,
                to,
            })
        })
        .collect()
}

/// `dnf check-update`: `name.arch version repo`, after a blank line.
pub fn parse_dnf(out: &str) -> Vec<Update> {
    out.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (pkg, to, _repo) = (f.next()?, f.next()?, f.next()?);
            if f.next().is_some() || l.starts_with(' ') || pkg.ends_with(':') {
                return None;
            }
            let name = pkg.rsplit_once('.').map_or(pkg, |(n, _)| n);
            Some(Update {
                name: name.to_owned(),
                from: None,
                to: to.to_owned(),
            })
        })
        .collect()
}

/// `name old -> new`, or `name new`.
pub fn parse_arrows(out: &str) -> Vec<Update> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            match f.as_slice() {
                [name, from, "->", to] => Some(Update {
                    name: (*name).to_owned(),
                    from: Some((*from).to_owned()),
                    to: (*to).to_owned(),
                }),
                [name, to] => Some(Update {
                    name: (*name).to_owned(),
                    from: None,
                    to: (*to).to_owned(),
                }),
                _ => None,
            }
        })
        .collect()
}

/// `pkg version -vRL=`: `name-old < needs updating (remote has new)`.
pub fn parse_pkg(out: &str) -> Vec<Update> {
    out.lines()
        .filter_map(|l| {
            let (pkg, rest) = l.split_once(' ')?;
            let to = rest
                .split_once("remote has ")?
                .1
                .trim_end_matches(')')
                .to_owned();
            let (name, from) = pkg.rsplit_once('-')?;
            Some(Update {
                name: name.to_owned(),
                from: Some(from.to_owned()),
                to,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_manager_reads() {
        let apt = "Listing...\nfirefox-esr/stable-security 128.4.0esr-1~deb12u1 amd64 [upgradable from: 128.3.1esr-1~deb12u1]\nlibc6/stable 2.36-9+deb12u9 amd64 [upgradable from: 2.36-9+deb12u8]\n";
        let a = parse_apt(apt);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].name, "firefox-esr");
        assert_eq!(a[0].from.as_deref(), Some("128.3.1esr-1~deb12u1"));
        assert_eq!(a[1].to, "2.36-9+deb12u9");

        let dnf = "\nkernel.x86_64    6.11.4-301.fc41    updates\nvim-enhanced.x86_64   2:9.1.785-1.fc41   updates\nObsoleting Packages\n";
        let d = parse_dnf(dnf);
        assert_eq!(
            d.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
            ["kernel", "vim-enhanced"]
        );

        let arch = "linux 6.11.3.arch1-1 -> 6.11.4.arch1-1\nmesa 1:24.2.4-1 -> 1:24.2.5-1\n";
        assert_eq!(parse_arrows(arch)[1].to, "1:24.2.5-1");

        let pkg = "curl-8.10.1 < needs updating (remote has 8.11.0)\n";
        let p = parse_pkg(pkg);
        assert_eq!((p[0].name.as_str(), p[0].to.as_str()), ("curl", "8.11.0"));
    }

    #[test]
    fn a_manager_failing_says_why() {
        let m = Manager::Custom {
            check: "sh -c true".into(),
            install: "sh -c false".into(),
        };
        assert_eq!(check(&m), Ok(Vec::new()));
        assert!(install(&m).is_err());
    }
}
