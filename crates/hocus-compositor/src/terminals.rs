//! Terminals (docs/design/projects.md §8.2, #789): which windows are
//! terminals, the folder their shell is in and the program it runs, read
//! from `/proc` on a worker thread (never on the loop), and how a terminal
//! is started again in a folder.
//!
//! - **A terminal** is a window whose desktop entry has `TerminalEmulator`
//!   (or `Terminal`) in its `Categories`, or whose app id is in [`TABLE`].
//! - **Its folder**: the Wayland client's process, its children that have
//!   a controlling terminal (the shells), and their `/proc/PID/cwd`. A
//!   process serving one window: its first shell's folder (a window with
//!   tabs or splits comes back as one window). A process serving several
//!   (`footclient`'s server, kitty's single instance,
//!   `gnome-terminal-server`, wezterm's mux) cannot tell which shell is
//!   whose: each of its windows gets the folder of the shell started last
//!   inside the project's folder, else the project's folder (outside a
//!   project, the shell started last).
//! - **The program it runs**: the shell's foreground process group
//!   (`/proc/PID/stat`'s `tpgid`), when it is not the shell's own: its
//!   command line, which the closing's confirmation says will stop. It is
//!   never started again.

use std::path::{Path, PathBuf};

/// How a terminal is given a folder, and a command (lot D's agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderArg {
    /// `--working-directory=DIR`, one argument.
    Joined(&'static str),
    /// `--directory DIR`, two arguments.
    Apart(&'static str),
    /// wezterm's `start --cwd DIR`.
    Wezterm,
}

/// The terminals §8.2 knows, by their app ids (lower case), and how each
/// takes a folder. Another terminal gets the working directory alone.
pub const TABLE: [(&str, FolderArg); 12] = [
    ("foot", FolderArg::Joined("--working-directory=")),
    ("footclient", FolderArg::Joined("--working-directory=")),
    ("kitty", FolderArg::Apart("--directory")),
    ("alacritty", FolderArg::Apart("--working-directory")),
    ("org.wezfurlong.wezterm", FolderArg::Wezterm),
    (
        "com.mitchellh.ghostty",
        FolderArg::Joined("--working-directory="),
    ),
    (
        "org.gnome.terminal",
        FolderArg::Joined("--working-directory="),
    ),
    (
        "org.gnome.console",
        FolderArg::Joined("--working-directory="),
    ),
    (
        "org.gnome.ptyxis",
        FolderArg::Joined("--working-directory="),
    ),
    ("org.kde.konsole", FolderArg::Apart("--workdir")),
    ("xfce4-terminal", FolderArg::Joined("--working-directory=")),
    ("xterm", FolderArg::Apart("")),
];

/// The table's row of an app id or a desktop entry's id.
fn row(id: &str) -> Option<FolderArg> {
    let id = id.to_lowercase();
    TABLE
        .iter()
        .find(|(app, _)| *app == id)
        .map(|(_, arg)| *arg)
}

/// Whether an app id is a terminal's (the table).
pub fn is_terminal_app(app: &str) -> bool {
    row(app).is_some()
}

/// Whether a desktop entry is a terminal's: its categories, else its id.
pub fn is_terminal_entry(entry: &hocus_xdg::desktop::Entry) -> bool {
    entry
        .categories
        .iter()
        .any(|c| c == "TerminalEmulator" || c == "Terminal")
        || is_terminal_app(&entry.id)
}

/// `argv`, a terminal's command, opening in `folder`: its own option where
/// the table has one (the working directory is set besides, by whoever
/// starts it).
pub fn in_folder(app: &str, argv: &[String], folder: &Path) -> Vec<String> {
    let mut out = argv.to_vec();
    let dir = folder.display().to_string();
    match row(app) {
        Some(FolderArg::Joined(option)) => out.push(format!("{option}{dir}")),
        Some(FolderArg::Apart("")) | None => {}
        Some(FolderArg::Apart(option)) => out.extend([option.to_owned(), dir]),
        Some(FolderArg::Wezterm) => {
            if !out.iter().any(|a| a == "start") {
                out.push("start".to_owned());
            }
            out.extend(["--cwd".to_owned(), dir]);
        }
    }
    out
}

/// What `argv`'s terminal `app` (after [`in_folder`]) adds to run
/// `command` in its window (§8.2's command column): none for a terminal the
/// table does not know.
pub fn with_command(app: &str, command: &[String]) -> Option<Vec<String>> {
    let app = app.to_lowercase();
    let before: &[&str] = match app.as_str() {
        "foot"
        | "footclient"
        | "org.wezfurlong.wezterm"
        | "org.gnome.terminal"
        | "org.gnome.console" => &["--"],
        "kitty" => &[],
        "alacritty" | "com.mitchellh.ghostty" | "org.kde.konsole" | "xterm" => &["-e"],
        "xfce4-terminal" => &["-x"],
        // Ptyxis takes the command as one word.
        "org.gnome.ptyxis" => {
            return Some(vec!["-x".to_owned(), shell_words(command)]);
        }
        _ => return None,
    };
    Some(
        before
            .iter()
            .map(|s| (*s).to_owned())
            .chain(command.iter().cloned())
            .collect(),
    )
}

/// `words` as one line a shell reads back the same: each quoted.
pub fn shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|w| format!("'{}'", w.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A process as `/proc/PID/stat` says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub pid: u32,
    pub ppid: u32,
    pub pgrp: i32,
    /// Its controlling terminal, 0 none.
    pub tty: i32,
    /// Its terminal's foreground process group, -1 none.
    pub tpgid: i32,
    /// When it started, in clock ticks after boot.
    pub start: u64,
}

/// `/proc/PID/stat`'s line read: its fields after the command's name in
/// parentheses, which may hold spaces and parentheses itself.
pub fn parse_stat(line: &str) -> Option<Stat> {
    let (head, rest) = line.rsplit_once(')')?;
    let pid = head.split_whitespace().next()?.parse().ok()?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // From the state, field 3: ppid 4, pgrp 5, session 6, tty_nr 7,
    // tpgid 8, …, starttime 22.
    let at = |n: usize| fields.get(n - 3).copied();
    Some(Stat {
        pid,
        ppid: at(4)?.parse().ok()?,
        pgrp: at(5)?.parse().ok()?,
        tty: at(7)?.parse().ok()?,
        tpgid: at(8)?.parse().ok()?,
        start: at(22)?.parse().ok()?,
    })
}

/// A shell of a terminal: its process, its folder, the program in its
/// foreground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    pub stat: Stat,
    pub folder: PathBuf,
    pub program: Option<String>,
}

/// What a terminal window is: its folder, and the program it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub folder: PathBuf,
    pub program: Option<String>,
}

/// The shell whose folder a window of a terminal takes, among `shells` (in
/// any order): the first started when its process serves that window
/// alone; else the one started last inside `within` (the project's
/// folder), else none (the project's folder is taken); without a project,
/// the one started last.
pub fn pick<'a>(shells: &'a [Shell], windows: usize, within: Option<&Path>) -> Option<&'a Shell> {
    let mut by_start: Vec<&Shell> = shells.iter().collect();
    by_start.sort_by_key(|s| s.stat.start);
    if windows <= 1 {
        return by_start.first().copied();
    }
    match within {
        Some(within) => by_start
            .iter()
            .rev()
            .find(|s| s.folder.starts_with(within))
            .copied(),
        None => by_start.last().copied(),
    }
}

/// The processes of `/proc` (`root`): those whose parent is `pid`.
fn children(root: &Path, pid: u32) -> Vec<Stat> {
    let Ok(dir) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    dir.filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|e| std::fs::read_to_string(e.path().join("stat")).ok())
        .filter_map(|line| parse_stat(&line))
        .filter(|s| s.ppid == pid)
        .collect()
}

/// A process's command line, its arguments a space apart, its program by
/// its name alone: what the confirmation names.
fn command_line(root: &Path, pid: i32) -> Option<String> {
    let raw = std::fs::read(root.join(pid.to_string()).join("cmdline")).ok()?;
    let mut words = raw
        .split(|b| *b == 0)
        .filter(|w| !w.is_empty())
        .map(|w| String::from_utf8_lossy(w).into_owned());
    let first = words.next()?;
    let program = first.rsplit('/').next().unwrap_or(&first).to_owned();
    let line: Vec<String> = std::iter::once(program).chain(words).collect();
    let line = line.join(" ");
    // A short line: the confirmation shows it in a sentence.
    Some(if line.chars().count() > 40 {
        let cut: String = line.chars().take(39).collect();
        format!("{cut}…")
    } else {
        line
    })
}

/// The shells of terminal process `pid`, read from `/proc` (`root`): its
/// children with a controlling terminal.
pub fn shells(root: &Path, pid: u32) -> Vec<Shell> {
    children(root, pid)
        .into_iter()
        .filter(|s| s.tty != 0)
        .filter_map(|stat| {
            let folder = std::fs::read_link(root.join(stat.pid.to_string()).join("cwd")).ok()?;
            let program = (stat.tpgid > 0 && stat.tpgid != stat.pgrp)
                .then(|| command_line(root, stat.tpgid))
                .flatten();
            Some(Shell {
                stat,
                folder,
                program,
            })
        })
        .collect()
}

/// What the window of terminal process `pid` is, it serving `windows`
/// windows, in a project of folder `within` (if it is in one): read off
/// the loop. None when its shells cannot be read; the project's folder
/// when none of them is in it.
pub fn read(pid: u32, windows: usize, within: Option<&Path>) -> Option<Found> {
    let root = Path::new("/proc");
    let shells = shells(root, pid);
    match pick(&shells, windows, within) {
        Some(shell) => Some(Found {
            folder: shell.folder.clone(),
            program: shell.program.clone(),
        }),
        None => within.filter(|_| !shells.is_empty()).map(|w| Found {
            folder: w.to_path_buf(),
            program: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(pid: u32, start: u64, folder: &str) -> Shell {
        Shell {
            stat: Stat {
                pid,
                ppid: 1,
                pgrp: pid as i32,
                tty: 34816,
                tpgid: pid as i32,
                start,
            },
            folder: folder.into(),
            program: None,
        }
    }

    #[test]
    fn a_stat_line_reads_whatever_its_name_holds() {
        let line = "4012 (my (odd) sh) S 4000 4012 4012 34817 4100 4194304 0 0 0 0 1 2 0 0 \
                    20 0 1 0 987654 1000 100";
        assert_eq!(
            parse_stat(line),
            Some(Stat {
                pid: 4012,
                ppid: 4000,
                pgrp: 4012,
                tty: 34817,
                tpgid: 4100,
                start: 987_654,
            })
        );
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn a_window_takes_its_shells_folder_a_server_the_last_in_the_project() {
        let shells = [
            shell(10, 300, "/home/a/github/site/src"),
            shell(11, 100, "/home/a/github/site"),
            shell(12, 500, "/tmp"),
        ];
        // One window: its first shell, tabs or not.
        assert_eq!(pick(&shells, 1, None).map(|s| s.stat.pid), Some(11));
        // A server's windows: the last started inside the project.
        let site = Path::new("/home/a/github/site");
        assert_eq!(pick(&shells, 3, Some(site)).map(|s| s.stat.pid), Some(10));
        assert_eq!(pick(&shells, 3, Some(Path::new("/srv"))), None);
        // Outside a project, the last started.
        assert_eq!(pick(&shells, 3, None).map(|s| s.stat.pid), Some(12));
    }

    #[test]
    fn a_terminal_opens_in_a_folder_by_its_own_option() {
        let argv = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let dir = Path::new("/home/a/site");
        assert_eq!(
            in_folder("foot", &argv(&["foot"]), dir),
            ["foot", "--working-directory=/home/a/site"]
        );
        assert_eq!(
            in_folder("kitty", &argv(&["kitty"]), dir),
            ["kitty", "--directory", "/home/a/site"]
        );
        assert_eq!(
            in_folder("org.wezfurlong.wezterm", &argv(&["wezterm", "start"]), dir),
            ["wezterm", "start", "--cwd", "/home/a/site"]
        );
        assert_eq!(in_folder("other", &argv(&["other"]), dir), ["other"]);
        assert!(is_terminal_app("Alacritty") && !is_terminal_app("firefox"));
    }

    #[test]
    fn a_terminal_runs_a_command_by_its_own_option() {
        let cmd = ["/bin/zsh", "-c", "claude --resume 3f6c; exec $SHELL"].map(str::to_owned);
        let with = |app: &str| with_command(app, &cmd);
        assert_eq!(
            with("foot"),
            Some(vec![
                "--".into(),
                "/bin/zsh".into(),
                "-c".into(),
                "claude --resume 3f6c; exec $SHELL".into()
            ])
        );
        assert_eq!(with("kitty").map(|a| a.len()), Some(3));
        assert_eq!(
            with("Alacritty").and_then(|a| a.first().cloned()),
            Some("-e".into())
        );
        assert_eq!(
            with("xfce4-terminal").and_then(|a| a.first().cloned()),
            Some("-x".into())
        );
        assert_eq!(
            with("org.gnome.Ptyxis"),
            Some(vec![
                "-x".into(),
                "'/bin/zsh' '-c' 'claude --resume 3f6c; exec $SHELL'".into()
            ])
        );
        assert_eq!(with("org.example.Term"), None);
        // wezterm: its folder's `start --cwd DIR`, then the command.
        let dir = Path::new("/srv/site");
        let mut argv = in_folder("org.wezfurlong.wezterm", &["wezterm".to_owned()], dir);
        argv.extend(with("org.wezfurlong.wezterm").unwrap_or_default());
        assert_eq!(argv[..5], ["wezterm", "start", "--cwd", "/srv/site", "--"]);
        assert_eq!(shell_words(&["it's".to_owned()]), "'it'\\''s'");
    }

    #[test]
    fn this_process_shells_are_read_from_proc() {
        // This test's process has no shell child: none.
        assert!(shells(Path::new("/proc"), std::process::id()).is_empty());
        assert_eq!(read(std::process::id(), 1, Some(Path::new("/srv"))), None);
    }
}
