//! The recent git repositories the project's form proposes
//! (docs/design/projects.md §2.3), found on a thread of the shell's, never
//! on its loop, and again only when the last look is a minute old:
//!
//! 1. the folders the terminals on the desktop are in (their processes'
//!    children that have a controlling terminal, the shells, and their
//!    working directories), "ses terminaux y sont";
//! 2. the folders the agents' hooks named in the last 30 days, kept in
//!    `$XDG_STATE_HOME/hocuspanda/folders` (a line each: the path, a tab,
//!    the last time in seconds of the Unix epoch), 200 at most;
//! 3. the directories holding a `.git` under `$HOME` two levels down, and
//!    three levels under `~/github`, `~/gitlab`, `~/src`, `~/code`,
//!    `~/projects`, `~/dev`, `~/work`, `~/git`: no symbolic link
//!    followed, no hidden directory, `node_modules` or `target` entered,
//!    4000 directories at most.
//!
//! Each is a repository's top (the folder holding `.git`, a file for a
//! worktree), "when" the newest of its `logs/HEAD` and `index` (a
//! worktree's own, in the main repository's `.git/worktrees/`). The
//! terminals' come first, then the newest. A project's folder is not
//! listed.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant, SystemTime},
};

/// The directories looked into at most.
const MOST_DIRS: usize = 4000;
/// The folders file's lines kept at most, and how old they may be.
const MOST_FOLDERS: usize = 200;
const FOLDERS_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// A look this recent is not done again.
const AGAIN_AFTER: Duration = Duration::from_secs(60);
/// The folders looked three levels under.
const DEEP: [&str; 8] = [
    "github", "gitlab", "src", "code", "projects", "dev", "work", "git",
];
/// Never entered.
const SKIPPED: [&str; 2] = ["node_modules", "target"];

/// A repository found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// Its top, absolute.
    pub path: PathBuf,
    /// Its last commit or staging, wall clock.
    pub when: Option<SystemTime>,
    /// A terminal of the desktop is in it.
    pub terminals: bool,
}

impl Repo {
    /// Its folder's last part.
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }
}

/// Where to look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    pub home: PathBuf,
    /// `$XDG_STATE_HOME/hocuspanda`.
    pub state: Option<PathBuf>,
    /// `/proc`.
    pub proc: PathBuf,
}

impl Places {
    pub fn from_env() -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/state"))
            .join("hocuspanda");
        Some(Self {
            home,
            state: Some(state),
            proc: PathBuf::from("/proc"),
        })
    }
}

/// The last look at the folders (2 and 3), and when.
static LAST: Mutex<Option<(Instant, Places, Vec<Repo>)>> = Mutex::new(None);

/// The repositories for a form whose desktop's terminals are `terminals`,
/// leaving `projects`' folders out: the terminals' looked at now, the rest
/// again only when the last look is a minute old. Blocks: on a thread.
pub fn find(places: &Places, terminals: &[u32], projects: &[PathBuf]) -> Vec<Repo> {
    let kept = {
        let last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
        last.as_ref()
            .filter(|(at, p, _)| at.elapsed() < AGAIN_AFTER && p == places)
            .map(|(_, _, repos)| repos.clone())
    };
    let looked = kept.unwrap_or_else(|| {
        let mut repos = from_folders(places);
        repos.extend(scan(&places.home));
        *LAST.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((Instant::now(), places.clone(), repos.clone()));
        repos
    });
    let mut all: Vec<Repo> = terminal_folders(&places.proc, terminals)
        .into_iter()
        .filter_map(|folder| top_of(&folder))
        .map(|path| Repo {
            when: when(&path),
            path,
            terminals: true,
        })
        .collect();
    all.extend(looked);
    ordered(all, projects)
}

/// `repos` the terminals' first then the newest, each once, without
/// `projects`' folders.
pub fn ordered(mut repos: Vec<Repo>, projects: &[PathBuf]) -> Vec<Repo> {
    repos.sort_by(|a, b| {
        b.terminals
            .cmp(&a.terminals)
            .then_with(|| b.when.cmp(&a.when))
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut seen = HashSet::new();
    repos.retain(|r| seen.insert(r.path.clone()) && !projects.contains(&r.path));
    repos
}

/// The folders the agents' hooks named in the last 30 days, as
/// repositories.
fn from_folders(places: &Places) -> Vec<Repo> {
    let Some(state) = &places.state else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(state.join("folders")) else {
        return Vec::new();
    };
    let now = SystemTime::now();
    text.lines()
        .rev()
        .take(MOST_FOLDERS)
        .filter_map(|line| {
            let (path, at) = line.split_once('\t')?;
            let at = SystemTime::UNIX_EPOCH + Duration::from_secs(at.trim().parse().ok()?);
            now.duration_since(at)
                .is_ok_and(|age| age <= FOLDERS_AGE)
                .then(|| PathBuf::from(path))
        })
        .filter_map(|folder| top_of(&folder))
        .map(|path| Repo {
            when: when(&path),
            path,
            terminals: false,
        })
        .collect()
}

/// The repository `folder` is in: the nearest folder up holding `.git`.
pub fn top_of(folder: &Path) -> Option<PathBuf> {
    folder
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The directories holding a `.git` under `home`: two levels down, three
/// under its code folders, within the limits.
pub fn scan(home: &Path) -> Vec<Repo> {
    let mut budget = MOST_DIRS;
    let mut found = Vec::new();
    let deep: Vec<PathBuf> = DEEP.iter().map(|d| home.join(d)).collect();
    walk(home, 2, &deep, &mut budget, &mut found);
    for dir in &deep {
        walk(dir, 3, &[], &mut budget, &mut found);
    }
    found
        .into_iter()
        .map(|path| Repo {
            when: when(&path),
            path,
            terminals: false,
        })
        .collect()
}

/// `dir`'s directories `levels` down that hold a `.git`, into `found`;
/// `skip`'s are looked at elsewhere.
fn walk(dir: &Path, levels: usize, skip: &[PathBuf], budget: &mut usize, found: &mut Vec<PathBuf>) {
    if levels == 0 || *budget == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.starts_with('.') && !SKIPPED.contains(&n))
        })
        .collect();
    dirs.sort();
    for sub in dirs {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        if skip.contains(&sub) {
            continue;
        }
        if sub.join(".git").exists() {
            found.push(sub);
            continue;
        }
        walk(&sub, levels - 1, skip, budget, found);
    }
}

/// When repository `top` last changed: the newest of its `logs/HEAD` and
/// `index`, a worktree's own.
pub fn when(top: &Path) -> Option<SystemTime> {
    let git = top.join(".git");
    let dir = if git.is_file() {
        let text = std::fs::read_to_string(&git).ok()?;
        let to = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
        let to = PathBuf::from(to);
        if to.is_absolute() { to } else { top.join(to) }
    } else {
        git
    };
    let modified = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    modified(dir.join("logs/HEAD")).max(modified(dir.join("index")))
}

/// The working directories of the shells the terminals `pids` run: their
/// processes' children that have a controlling terminal.
pub fn terminal_folders(proc: &Path, pids: &[u32]) -> Vec<PathBuf> {
    if pids.is_empty() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(proc) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some((parent, tty)) = parent_and_tty(&stat) else {
            continue;
        };
        if tty == 0 || !pids.contains(&parent) || pids.contains(&pid) {
            continue;
        }
        if let Ok(cwd) = std::fs::read_link(entry.path().join("cwd")) {
            out.push(cwd);
        }
    }
    out
}

/// A process's parent and controlling terminal, from its `stat`.
fn parent_and_tty(stat: &str) -> Option<(u32, i64)> {
    // After the command, in parentheses that it may hold itself.
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // state, ppid, pgrp, session, tty_nr.
    let parent = fields.get(1)?.parse().ok()?;
    let tty = fields.get(4)?.parse().ok()?;
    Some((parent, tty))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(dir: &Path) {
        std::fs::create_dir_all(dir.join(".git/logs")).expect("made");
        std::fs::write(dir.join(".git/logs/HEAD"), "x").expect("written");
    }

    #[test]
    fn the_scan_finds_repositories_within_its_limits() {
        let home = tempfile::tempdir().expect("a home");
        let home = home.path();
        repo(&home.join("notes"));
        repo(&home.join("a/b"));
        // Three levels down under $HOME: too deep, but under ~/github.
        repo(&home.join("a/b/c/d"));
        repo(&home.join("github/team/x/site"));
        repo(&home.join("github/team/x/y/too-deep"));
        // Never entered.
        repo(&home.join(".hidden/repo"));
        repo(&home.join("code/node_modules/dep"));
        repo(&home.join("code/target/out"));
        // A worktree: a `.git` file naming its own directory.
        repo(&home.join("code/main"));
        let wt = home.join("code/main/.git/worktrees/fix");
        std::fs::create_dir_all(&wt).expect("made");
        std::fs::write(wt.join("index"), "x").expect("written");
        std::fs::create_dir_all(home.join("code/fix")).expect("made");
        std::fs::write(
            home.join("code/fix/.git"),
            format!("gitdir: {}\n", wt.display()),
        )
        .expect("written");
        let found: HashSet<PathBuf> = scan(home).into_iter().map(|r| r.path).collect();
        let want: HashSet<PathBuf> = [
            "notes",
            "a/b",
            "github/team/x/site",
            "code/main",
            "code/fix",
        ]
        .iter()
        .map(|p| home.join(p))
        .collect();
        assert_eq!(found, want);
        assert!(when(&home.join("code/fix")).is_some(), "its own index");
        // The budget holds.
        let mut budget = 2;
        let mut few = Vec::new();
        walk(home, 2, &[], &mut budget, &mut few);
        assert_eq!(budget, 0);
    }

    #[test]
    fn the_terminals_come_first_then_the_newest_and_no_projects() {
        let at = |s: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        let r = |p: &str, when, terminals| Repo {
            path: PathBuf::from(p),
            when,
            terminals,
        };
        let repos = ordered(
            vec![
                r("/h/old", at(10), false),
                r("/h/new", at(30), false),
                r("/h/term", at(5), true),
                r("/h/new", at(30), false),
                r("/h/project", at(50), false),
            ],
            &[PathBuf::from("/h/project")],
        );
        let paths: Vec<&str> = repos
            .iter()
            .map(|r| r.path.to_str().unwrap_or(""))
            .collect();
        assert_eq!(paths, ["/h/term", "/h/new", "/h/old"]);
        assert_eq!(repos[0].name(), "term");
    }

    #[test]
    fn a_stat_line_says_its_parent_and_terminal() {
        let stat = "4012 (fish (x)) S 3990 4012 4012 34817 4012 4194304 0";
        assert_eq!(parent_and_tty(stat), Some((3990, 34817)));
        assert_eq!(parent_and_tty("1 (init) S 0 1 1 0"), Some((0, 0)));
    }

    #[test]
    fn the_folders_file_holds_the_last_30_days() {
        let home = tempfile::tempdir().expect("a home");
        let state = home.path().join("state");
        std::fs::create_dir_all(&state).expect("made");
        let fresh = home.path().join("fresh");
        let old = home.path().join("old");
        repo(&fresh);
        repo(&old);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        std::fs::write(
            state.join("folders"),
            format!(
                "{}\t{}\n{}/src\t{}\n",
                old.display(),
                now - 40 * 24 * 3600,
                fresh.display(),
                now - 3600
            ),
        )
        .expect("written");
        let places = Places {
            home: home.path().to_path_buf(),
            state: Some(state),
            proc: PathBuf::from("/nonexistent"),
        };
        let repos: Vec<PathBuf> = from_folders(&places).into_iter().map(|r| r.path).collect();
        assert_eq!(repos, [fresh]);
    }
}
