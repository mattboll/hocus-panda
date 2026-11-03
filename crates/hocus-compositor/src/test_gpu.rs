//! Which GPU the tests may draw on: none, unless `HOCUS_TEST_GPU` names
//! its render node (issue #534).
//!
//! On a developer's machine the first render node is as often as not the
//! GPU the live session draws with: test compositors and their shells
//! drawing there in parallel once froze a machine whole. So the harness
//! draws on the CPU (pixman, or GLES on Mesa's software device) unless the
//! variable names a node, and fails a test that held one open anyway.

use std::path::{Path, PathBuf};

use crate::{HeadlessConfig, RendererChoice};

/// The variable naming the render node tests may draw on, as
/// `/dev/dri/renderD128`.
pub const ENV: &str = "HOCUS_TEST_GPU";

/// The render node `HOCUS_TEST_GPU` names, when it is there; `None`, and
/// the tests draw on the CPU.
pub fn node() -> Option<PathBuf> {
    named(std::env::var_os(ENV).as_deref())
}

/// The node a value of `HOCUS_TEST_GPU` names, when it is there.
fn named(value: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let path = PathBuf::from(value.filter(|v| !v.is_empty())?);
    path.exists().then_some(path)
}

/// A headless configuration drawing with GLES: on the GPU that
/// `HOCUS_TEST_GPU` names; else on Mesa's software device (llvmpipe),
/// never a GPU; else, without one, with pixman, which the tests of what
/// only GLES draws see in [`Headless::renderer`](crate::Headless::renderer).
pub fn gles() -> HeadlessConfig {
    let (renderer, render_node) = match node() {
        Some(node) => (RendererChoice::Gles, Some(node)),
        None if crate::backend::headless::software_gles() => (RendererChoice::GlesSoftware, None),
        None => (RendererChoice::Pixman, None),
    };
    HeadlessConfig {
        renderer,
        render_node,
        ..HeadlessConfig::default()
    }
}

/// What a test compositor draws with: a GPU only on the node
/// `HOCUS_TEST_GPU` names. `Auto` takes it, or pixman without it; `Gles`
/// without it, or on another node, is refused.
pub(crate) fn resolve(mut config: HeadlessConfig) -> anyhow::Result<HeadlessConfig> {
    let allowed = node();
    match (config.renderer, allowed) {
        (RendererChoice::Pixman | RendererChoice::GlesSoftware, _) => {}
        (RendererChoice::Auto, None) => config.renderer = RendererChoice::Pixman,
        (RendererChoice::Gles, None) => anyhow::bail!(
            "a test compositor would draw on a GPU: name its render node with \
             {ENV}=/dev/dri/renderDNNN, or draw with RendererChoice::GlesSoftware"
        ),
        (RendererChoice::Auto | RendererChoice::Gles, Some(node)) => {
            match &config.render_node {
                Some(asked) if !same_node(asked, &node) => anyhow::bail!(
                    "a test compositor would draw on {}, not on the {ENV} {}",
                    asked.display(),
                    node.display()
                ),
                _ => config.render_node = Some(node),
            }
            config.renderer = RendererChoice::Gles;
        }
    }
    Ok(config)
}

fn same_node(a: &Path, b: &Path) -> bool {
    a == b || std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok()
}

/// The render nodes open in this process and in the processes it started,
/// down to theirs (the shell, the programs it runs), as `pid: path`.
/// What draws on a GPU keeps its node open while it does: Mesa's EGL
/// displays, wgpu's devices. Empty where there is no `/proc`.
pub fn render_nodes_open() -> Vec<String> {
    let mut open = Vec::new();
    for pid in std::iter::once(std::process::id()).chain(descendants(std::process::id())) {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path())
                && is_render_node(&target)
            {
                open.push(format!("{pid}: {}", target.display()));
            }
        }
    }
    open
}

/// Whether `path` is a render node, as `/proc/PID/fd` shows it.
fn is_render_node(path: &Path) -> bool {
    path.parent() == Some(Path::new("/dev/dri"))
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("renderD"))
}

/// The processes `root` started, and theirs, from each one's parent in
/// `/proc/PID/stat`.
fn descendants(root: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let parents: Vec<(u32, u32)> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            Some((pid, parent_in_stat(&stat)?))
        })
        .collect();
    let mut found = vec![root];
    let mut next = 0;
    while next < found.len() {
        let parent = found[next];
        found.extend(
            parents
                .iter()
                .filter(|(pid, ppid)| *ppid == parent && !found.contains(pid))
                .map(|(pid, _)| *pid)
                .collect::<Vec<_>>(),
        );
        next += 1;
    }
    found.remove(0);
    found
}

/// The parent's pid in a `/proc/PID/stat` line: `pid (comm) state ppid …`,
/// the command's name possibly holding spaces and parentheses.
fn parent_in_stat(stat: &str) -> Option<u32> {
    let (_, after) = stat.rsplit_once(')')?;
    after.split_whitespace().nth(1)?.parse().ok()
}

/// Fails the test when a render node is open without `HOCUS_TEST_GPU`:
/// something in it drew on a GPU it was not given.
pub(crate) fn check_none_open() {
    if node().is_some() || std::thread::panicking() {
        return;
    }
    let open = render_nodes_open();
    assert!(
        open.is_empty(),
        "render nodes open without {ENV} (tests must not draw on the session's GPU): {open:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_node_that_is_there_is_named() -> anyhow::Result<()> {
        assert_eq!(named(None), None);
        assert_eq!(named(Some("".as_ref())), None);
        assert_eq!(named(Some("/nonexistent/renderD128".as_ref())), None);
        let dir = tempfile::tempdir()?;
        let node = dir.path().join("renderD200");
        std::fs::write(&node, b"")?;
        assert_eq!(named(Some(node.as_os_str())), Some(node));
        Ok(())
    }

    #[test]
    fn render_nodes_are_told_from_other_files() {
        assert!(is_render_node(Path::new("/dev/dri/renderD128")));
        assert!(!is_render_node(Path::new("/dev/dri/card1")));
        assert!(!is_render_node(Path::new("/tmp/renderD128")));
        assert!(!is_render_node(Path::new("socket:[1234]")));
    }

    #[test]
    fn a_parent_is_read_past_a_name_with_spaces() {
        assert_eq!(parent_in_stat("42 (a (b) c) S 7 42 42 0"), Some(7));
        assert_eq!(parent_in_stat("1 (init) S 0 1 1"), Some(0));
        assert_eq!(parent_in_stat("garbage"), None);
    }

    #[test]
    fn without_a_node_named_no_test_draws_on_a_gpu() -> anyhow::Result<()> {
        if node().is_some() {
            return Ok(());
        }
        let auto = resolve(HeadlessConfig::default())?;
        assert_eq!(auto.renderer, RendererChoice::Pixman);
        let on_a_gpu = HeadlessConfig {
            renderer: RendererChoice::Gles,
            render_node: Some("/dev/dri/renderD128".into()),
            ..HeadlessConfig::default()
        };
        assert!(resolve(on_a_gpu).is_err());
        assert_ne!(gles().renderer, RendererChoice::Gles);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_processes_started_and_their_files_are_found() -> anyhow::Result<()> {
        // `cat` waits on its input, and ends once it is closed.
        let mut child = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        let found = descendants(std::process::id()).contains(&child.id());
        drop(child.stdin.take());
        let _ = child.wait();
        assert!(found);
        // The scan reads this process's files: none is a render node.
        assert!(render_nodes_open().is_empty() || node().is_some());
        Ok(())
    }
}
