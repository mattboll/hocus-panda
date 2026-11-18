//! What comes and goes all day leaves nothing behind in the compositor
//! (#827): an agent whose process ended before its report was read is
//! forgotten, as one whose end was heard; a surface gone before its next
//! frame leaves no frame callback behind in its client, which the shell's
//! bubbles and pandas did for the session. The shell keeps to two of
//! glibc's malloc arenas, not one a thread, each keeping what was freed in
//! it (#855), and its surfaces opened and closed all day leave no Wayland
//! object behind (#857).

use crate::common;

use std::{io::Write as _, os::unix::net::UnixStream};

use anyhow::Context as _;
use common::{Command, Event, WAIT};
use hocus_agents::{Report, Source, State};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};

/// `report` on `hp`'s control socket, a connection of its own (read in
/// the order they come).
fn tell(hp: &Headless, report: &Report) -> anyhow::Result<()> {
    let socket = hp.control_socket().context("no control socket")?;
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(format!("{report}\n").as_bytes())?;
    Ok(())
}

fn done(key: &str, pid: u32) -> Report {
    Report {
        source: Source::ClaudeCode,
        key: key.to_owned(),
        state: Some(State::Done),
        pid: Some(pid),
        name: key.to_owned(),
        note: None,
        tests: None,
        conversation: hocus_agents::Conversation::default(),
    }
}

/// One-shot runs (`codex exec`, a script's agent) whose last hook's
/// process is gone before the desktop reads it: none stays in the list.
#[test]
fn agents_ended_before_their_report_was_read_are_forgotten() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    for n in 0..20 {
        let mut run = std::process::Command::new("true")
            .spawn()
            .unwrap_or_else(|_| common::missing("true"));
        let pid = run.id();
        run.wait()?;
        tell(&hp, &done(&format!("oneshot{n}"), pid))?;
    }
    // The test's own process, still running, told last: once its row
    // shows, the others were read.
    tell(&hp, &done("alive", std::process::id()))?;
    let names =
        |hp: &mut Headless| -> Vec<String> { hp.agents().into_iter().map(|a| a.name).collect() };
    let start = std::time::Instant::now();
    while names(&mut hp) != ["alive"] {
        anyhow::ensure!(start.elapsed() < WAIT, "{:?}", names(&mut hp));
        hp.dispatch(std::time::Duration::from_millis(5))?;
    }
    Ok(())
}

/// Fifty surfaces made and destroyed before a frame, each asking for its
/// frame callback: none of their callbacks stays.
#[test]
fn a_surface_gone_before_its_frame_leaves_no_callback() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let (_, client) = common::open_with(&mut hp, "test.menus", (400, 300), false)?;
    common::wait(&mut hp, std::time::Duration::from_millis(200))?;
    let before = hp.objects("wl_callback");
    client.commands.send(Command::Fleeting(50))?;
    // Its export, asked after them, answered: the compositor read them.
    client.commands.send(Command::Export)?;
    let exported = hp.dispatch_until(WAIT, |_| {
        client
            .events
            .try_iter()
            .any(|event| matches!(event, Event::Exported(_)))
    })?;
    anyhow::ensure!(exported, "the client's export never came");
    let after = hp.objects("wl_callback");
    anyhow::ensure!(after <= before, "{before} frame callbacks, then {after}");
    Ok(())
}

/// The heaps glibc's malloc mapped for its arenas besides the main one in
/// process `pid`: each reserves 64 MiB, aligned, its unused end
/// inaccessible.
fn malloc_heaps(pid: i32) -> anyhow::Result<usize> {
    const HEAP: u64 = 64 << 20;
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps"))?;
    // Start, end, permissions, and whether a file or a name backs it.
    let regions: Vec<(u64, u64, &str, bool)> = maps
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (start, end) = fields.next()?.split_once('-')?;
            let perms = fields.next()?;
            let named = fields.nth(3).is_some();
            Some((
                u64::from_str_radix(start, 16).ok()?,
                u64::from_str_radix(end, 16).ok()?,
                perms,
                named,
            ))
        })
        .collect();
    let mut heaps = 0;
    for (i, &(start, end, perms, named)) in regions.iter().enumerate() {
        if named || perms != "rw-p" || start % HEAP != 0 {
            continue;
        }
        let mut reach = end;
        for &(next, next_end, next_perms, next_named) in &regions[i + 1..] {
            if next != reach || next_named || next_perms != "---p" {
                break;
            }
            reach = next_end;
        }
        if reach - start == HEAP {
            heaps += 1;
        }
    }
    Ok(heaps)
}

/// The shell's threads allocate (iced's runtime, its workers), its
/// launcher opened and closed meanwhile: they share two arenas, the main
/// one and one heap, however many they are.
#[test]
fn the_shell_keeps_to_two_malloc_arenas() -> anyhow::Result<()> {
    let program = common::shell_program();
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let at = |p: &str| root.join(p).display().to_string();
    let script = common::shell_script(
        root,
        &program,
        &[
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_CACHE_HOME", at("cache")),
            ("ICED_BACKEND", "tiny-skia".to_owned()),
        ],
    )?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        Config::default(),
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    // `Super+Space` opens it, and again closes it.
    let toggle = |hp: &mut Headless| {
        hp.tell_shell(hocus_link::ToShell::Launcher {
            output: "HEADLESS-1".to_owned(),
            scale: 120,
        });
    };
    for _ in 0..5 {
        toggle(&mut hp);
        let open = hp.dispatch_until(WAIT, |hp| !hp.launcher_surfaces().is_empty())?;
        anyhow::ensure!(open, "the launcher never opened");
        toggle(&mut hp);
        let shut = hp.dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())?;
        anyhow::ensure!(shut, "the launcher never closed");
    }
    common::wait(&mut hp, std::time::Duration::from_secs(1))?;
    let pid = hp.shell_pid().context("no shell")?;
    let heaps = malloc_heaps(pid)?;
    anyhow::ensure!(heaps <= 1, "{heaps} malloc heaps besides the main arena");
    Ok(())
}

/// What each of the shell's layer surfaces asks of the compositor besides
/// its own: the layer shell bound for it (iced_layershell bound one a
/// surface), its viewport and its fractional scale (never destroyed with
/// it). #857.
const PER_SURFACE: [&str; 3] = [
    "zwlr_layer_shell_v1",
    "wp_viewport",
    "wp_fractional_scale_v1",
];

/// The launcher and the clipboard's popover opened and closed again and
/// again: the shell holds as many layer shells, viewports and fractional
/// scales as before.
#[test]
fn surfaces_opened_and_closed_leave_no_object_behind() -> anyhow::Result<()> {
    const KEY_ESC: u32 = 1;
    let program = common::shell_program();
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let at = |p: &str| root.join(p).display().to_string();
    let script = common::shell_script(
        root,
        &program,
        &[
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_CACHE_HOME", at("cache")),
            ("ICED_BACKEND", "tiny-skia".to_owned()),
        ],
    )?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        Config::default(),
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    // The bar's surfaces all made.
    common::wait(&mut hp, std::time::Duration::from_secs(1))?;
    let count = |hp: &Headless| PER_SURFACE.map(|interface| hp.objects(interface));
    let before = count(&hp);
    let launcher = |hp: &mut Headless| {
        hp.tell_shell(hocus_link::ToShell::Launcher {
            output: "HEADLESS-1".to_owned(),
            scale: 120,
        });
    };
    for _ in 0..10 {
        launcher(&mut hp);
        let open = hp.dispatch_until(WAIT, |hp| !hp.launcher_surfaces().is_empty())?;
        anyhow::ensure!(open, "the launcher never opened");
        launcher(&mut hp);
        let shut = hp.dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())?;
        anyhow::ensure!(shut, "the launcher never closed");
        hp.tell_shell(hocus_link::ToShell::Clipboard {
            output: "HEADLESS-1".to_owned(),
        });
        // Drawn, so that iced has its window for the key (#821).
        let open = hp.dispatch_until(WAIT, |hp| {
            common::drawn_popovers(hp).len() == 1
                && hp.keyboard_layer().as_deref() == Some(hocus_link::POPOVER_NAMESPACE)
        })?;
        anyhow::ensure!(open, "the clipboard's history never opened");
        hp.key(KEY_ESC, true);
        hp.key(KEY_ESC, false);
        let shut = hp.dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(shut, "the clipboard's history never closed");
    }
    common::wait(&mut hp, std::time::Duration::from_secs(1))?;
    let after = count(&hp);
    eprintln!("{PER_SURFACE:?}: {before:?} before, {after:?} after");
    anyhow::ensure!(
        after == before,
        "{PER_SURFACE:?}: {before:?} before 20 surfaces, {after:?} after"
    );
    Ok(())
}
