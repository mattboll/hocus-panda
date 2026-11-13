//! The agents' good news (#809, #810, #811; docs/design/effects.md, "The
//! moments"), on the held clock: an agent that finishes with all its
//! tests passing (as Claude Code's hooks say it, through `hocus
//! agent-hook`) sets fireworks off behind the windows, its row saying so,
//! and they end with nothing drawn after; `hocus celebrate` sets confetti
//! off, which settle on a window's top edge and melt, never over what it
//! shows, its agent's row saying why. On GLES (Mesa's software device, or
//! the GPU `HOCUS_TEST_GPU` names); on the CPU, the bar alone says it.
//! The commands run beside the test's program (`cargo programs`), each a
//! child of the test's process, whose window is the agent's.

use crate::common;

use std::{
    io::Write as _,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

use common::{WAIT, open_with};
use hocus_compositor::{Config, Headless, HeadlessConfig, OutputSpec, model::Rect};
use hocus_link::AgentNews;
use serde_json::json;

const STEP: Duration = Duration::from_micros(16_667);
/// The agent's window: room above it for the moments.
const AT: Rect = Rect {
    x: 300,
    y: 380,
    width: 640,
    height: 320,
};

/// Steps the held clock by `by`, a frame at a time.
fn run(hp: &mut Headless, by: Duration) -> anyhow::Result<()> {
    let mut left = by;
    while left > Duration::ZERO {
        let step = left.min(STEP);
        hp.step(step)?;
        left -= step;
    }
    Ok(())
}

/// `hocus ARGS` on `hp`'s control socket, `input` on its standard input.
fn hocus(hp: &Headless, args: &[&str], input: Option<&str>) -> anyhow::Result<()> {
    let socket = hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?;
    let mut child = Command::new(common::hocus_program())
        .arg("--socket")
        .arg(socket)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "hocus {args:?}: {status}");
    Ok(())
}

/// Claude Code's hook for `event` of session `s1`, its Bash command
/// having printed `stdout`.
fn hook(hp: &Headless, event: &str, stdout: &str) -> anyhow::Result<()> {
    let payload = json!({ "session_id": "s1", "cwd": "/home/alex/site",
        "hook_event_name": event, "tool_name": "Bash",
        "tool_input": { "command": "cargo nextest run --workspace" },
        "tool_response": { "stdout": stdout, "stderr": "", "interrupted": false } });
    hocus(
        hp,
        &["agent-hook", "claude-code"],
        Some(&payload.to_string()),
    )
}

/// Whether the agents come to say what `said` wants, within [`WAIT`].
fn agents_say(
    hp: &mut Headless,
    said: impl Fn(&[hocus_link::AgentShown]) -> bool,
) -> anyhow::Result<bool> {
    let mut waited = Duration::ZERO;
    while waited < WAIT {
        if said(&hp.agents()) {
            return Ok(true);
        }
        hp.dispatch_until(Duration::from_millis(20), |_| false)?;
        waited += Duration::from_millis(20);
    }
    Ok(false)
}

/// The news agent `name`'s row says, its reports all come.
fn news(hp: &mut Headless, name: &str) -> anyhow::Result<Option<AgentNews>> {
    // The commands returned: what they wrote is there, read within a turn.
    hp.dispatch_until(Duration::from_millis(200), |_| false)?;
    let agent = hp
        .agents()
        .into_iter()
        .find(|a| a.name == name)
        .ok_or_else(|| anyhow::anyhow!("no agent {name}: {:?}", hp.agents()))?;
    Ok(agent.news)
}

/// What output 0 shows: its width, height and pixels.
type Shot = (i32, i32, Vec<u8>);

/// A compositor (GLES unless `cpu`), the test's window at [`AT`], `config`;
/// what it shows then; `away`, on the second desktop then, where no window
/// has the keyboard (an agent finishing is not seen at once). The clock
/// held.
fn bed(
    config: Config,
    cpu: bool,
    away: bool,
) -> anyhow::Result<(Headless, common::ClientHandle, Shot)> {
    let headless = if cpu {
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..HeadlessConfig::default()
        }
    } else {
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            ..common::gles()
        }
    };
    let mut hp = Headless::start_with(headless, config)?;
    common::at_rest(&mut hp)?;
    let (_, client) = open_with(
        &mut hp,
        "test.terminal",
        (AT.width as u32, AT.height as u32),
        false,
    )?;
    let id = hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.terminal"))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window"))?;
    hp.place_window(id, AT);
    common::at_rest(&mut hp)?;
    let before = hp.screenshot(0)?;
    if away {
        desktop(&mut hp, common::KEY_2)?;
    }
    hp.hold_clock();
    run(&mut hp, STEP * 2)?;
    Ok((hp, client, before))
}

/// Goes to the desktop of `key` (`Super+N`), and waits there.
fn desktop(hp: &mut Headless, key: u32) -> anyhow::Result<()> {
    let index = if key == common::KEY_2 { 1 } else { 0 };
    common::chord(hp, &[common::KEY_LEFTMETA, key]);
    let there = hp.dispatch_until(WAIT, |hp| hp.desktop() == (index, index as f64))?;
    anyhow::ensure!(there, "{:?}", hp.desktop());
    Ok(())
}

fn moments(fireworks: bool, confetti: bool) -> Config {
    let mut config = Config::default();
    config.effects.fireworks = fireworks;
    config.effects.confetti = confetti;
    config
}

/// Over `rect`, the pixels the same in `a` and `b`, of how many.
fn same(a: &(i32, i32, Vec<u8>), b: &(i32, i32, Vec<u8>), rect: Rect) -> (usize, usize) {
    let (mut same, mut all) = (0, 0);
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let i = ((y * a.0 + x) * 4) as usize;
            all += 1;
            same += usize::from(a.2.get(i..i + 3) == b.2.get(i..i + 3));
        }
    }
    (same, all)
}

/// What the window shows: below its top 8 pixels, its round corners
/// apart.
const SHOWS: Rect = Rect {
    x: AT.x + 12,
    y: AT.y + 8,
    width: AT.width - 24,
    height: AT.height - 20,
};
/// The wallpaper above the window.
const SKY: Rect = Rect {
    x: 0,
    y: 0,
    width: 1280,
    height: AT.y - 30,
};

/// Its tests pass as its turn ends: fireworks once, the row saying so,
/// the window's pixels untouched; over within ten seconds, nothing drawn
/// after. A turn whose last run failed sets none off.
#[test]
fn fireworks_go_off_as_an_agents_tests_pass_then_nothing_is_drawn() -> anyhow::Result<()> {
    let (mut hp, _client, before) = bed(moments(true, false), false, true)?;
    let green = "     Summary [   2.000s] 12 tests run: 12 passed, 0 skipped\n";
    let red = "     Summary [   2.000s] 12 tests run: 11 passed, 1 failed\n";
    // Green, then red: the last run counts.
    hook(&hp, "PostToolUse", green)?;
    hook(&hp, "PostToolUse", red)?;
    hook(&hp, "Stop", "")?;
    assert_eq!(news(&mut hp, "site")?, None);
    assert!(hp.fireworks().is_none(), "none for a red run");
    // The next turn: green as it ends.
    hook(&hp, "PostToolUse", green)?;
    hook(&hp, "Stop", "")?;
    let went = hp.dispatch_until(WAIT, |hp| hp.fireworks().is_some())?;
    anyhow::ensure!(went, "no fireworks");
    assert_eq!(
        news(&mut hp, "site")?,
        Some(AgentNews::TestsPassed { tests: Some(12) })
    );
    // Back to its window, which the fireworks play behind.
    common::chord(&mut hp, &[common::KEY_LEFTMETA, common::KEY_1]);
    run(&mut hp, Duration::from_millis(2600))?;
    assert_eq!(hp.desktop(), (0, 0.0));
    let (output, sparks) = hp
        .fireworks()
        .ok_or_else(|| anyhow::anyhow!("over too soon"))?;
    assert_eq!(output, "HEADLESS-1");
    assert!(sparks.len() > 100, "{} sparks", sparks.len());
    let shot = hp.screenshot(0)?;
    let (kept, all) = same(&before, &shot, SKY);
    assert!(all - kept > 2000, "the sky lit: {} pixels", all - kept);
    let (kept, all) = same(&before, &shot, SHOWS);
    assert_eq!(kept, all, "the window's pixels untouched");
    // Over, then nothing.
    let mut waited = Duration::ZERO;
    // A frame each tenth of a second: they step by their own clock.
    while hp.fireworks().is_some() && waited < Duration::from_secs(10) {
        hp.step(Duration::from_millis(100))?;
        waited += Duration::from_millis(100);
    }
    assert!(hp.fireworks().is_none(), "still playing after 10 s");
    run(&mut hp, Duration::from_millis(100))?;
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(500))?;
    assert_eq!(hp.frames_on(0), frames, "nothing drawn once over");
    let after = hp.screenshot(0)?;
    let (kept, all) = same(&before, &after, SKY);
    assert_eq!(kept, all, "nothing left of them");
    Ok(())
}

/// `hocus celebrate`: confetti fly, settle on the window's top edge (a
/// few pixels above it) and melt; never over what it shows; the agent's
/// row says why; all gone within twenty seconds, nothing drawn after.
#[test]
fn confetti_settle_on_a_windows_edge_and_melt_never_over_what_it_shows() -> anyhow::Result<()> {
    let (mut hp, _client, before) = bed(moments(false, true), false, false)?;
    hocus(&hp, &["agent", "site", "working"], None)?;
    hocus(&hp, &["celebrate", "#791 merged\non main"], None)?;
    let went = hp.dispatch_until(WAIT, |hp| hp.confetti().is_some())?;
    anyhow::ensure!(went, "no confetti");
    assert_eq!(
        news(&mut hp, "site")?,
        Some(AgentNews::Good {
            why: "#791 merged on main".into()
        })
    );
    let window = hp.windows()[0].id;
    let mut lay = false;
    let mut over_edge = false;
    for _ in 0..12 {
        run(&mut hp, Duration::from_millis(250))?;
        let (_, lying) = hp
            .confetti()
            .ok_or_else(|| anyhow::anyhow!("gone too soon"))?;
        let on_it: Vec<_> = lying
            .iter()
            .filter(|(w, _, _)| *w == Some(window))
            .collect();
        assert!(
            on_it
                .iter()
                .all(|(_, x, y)| (0.0..AT.width as f32).contains(x) && (-5.0..=-2.0).contains(y)),
            "{on_it:?}"
        );
        if on_it.len() > 5 {
            lay = true;
            let shot = hp.screenshot(0)?;
            // Its top pixels: only what lies on it, in front of it, draws
            // there (what flies is behind it).
            let edge = Rect {
                x: AT.x + 12,
                y: AT.y,
                width: AT.width - 24,
                height: 3,
            };
            let (kept, all) = same(&before, &shot, edge);
            over_edge |= all - kept > 30;
            let (kept, all) = same(&before, &shot, SHOWS);
            assert_eq!(kept, all, "the window's pixels untouched");
        }
    }
    assert!(lay, "none settled on the window");
    assert!(over_edge, "none drawn on its edge");
    let mut waited = Duration::ZERO;
    while hp.confetti().is_some() && waited < Duration::from_secs(20) {
        hp.step(Duration::from_millis(100))?;
        waited += Duration::from_millis(100);
    }
    assert!(hp.confetti().is_none(), "still there after 20 s");
    run(&mut hp, Duration::from_millis(100))?;
    let frames = hp.frames_on(0);
    run(&mut hp, Duration::from_millis(500))?;
    assert_eq!(hp.frames_on(0), frames, "nothing drawn once over");
    Ok(())
}

/// On the CPU (as with reduced motion, in the power saver or while a
/// screen is shared), neither plays: the bar alone says the news. Done
/// without its tests passing, it says none.
#[test]
fn on_the_cpu_the_bar_alone_says_the_news() -> anyhow::Result<()> {
    let (mut hp, _client, _) = bed(moments(true, true), true, true)?;
    hocus(&hp, &["agent", "build", "working"], None)?;
    hocus(&hp, &["agent", "build", "done"], None)?;
    assert_eq!(news(&mut hp, "build")?, None);
    hocus(&hp, &["agent", "build", "working"], None)?;
    hocus(
        &hp,
        &["agent", "build", "done", "--tests-passed", "40"],
        None,
    )?;
    let said = agents_say(&mut hp, |agents| {
        agents
            .iter()
            .any(|a| a.news == Some(AgentNews::TestsPassed { tests: Some(40) }))
    })?;
    anyhow::ensure!(said, "no tests passing said: {:?}", hp.agents());
    hocus(&hp, &["celebrate", "deployed", "--name", "infra"], None)?;
    let said = agents_say(&mut hp, |agents| {
        agents.iter().any(|a| {
            a.news
                == Some(AgentNews::Good {
                    why: "deployed".into(),
                })
        })
    })?;
    anyhow::ensure!(said, "no good news said: {:?}", hp.agents());
    run(&mut hp, Duration::from_millis(500))?;
    assert!(hp.fireworks().is_none() && hp.confetti().is_none());
    Ok(())
}

// --- The screenshots ------------------------------------------------------------

/// A screenshot of output 0 saved as `name`.png in `out`.
fn save(hp: &mut Headless, out: &Path, name: &str) -> anyhow::Result<()> {
    let (w, h, px) = hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .ok_or_else(|| anyhow::anyhow!("no image"))?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The moments for the maintainer, written to `$HOCUS_SHOTS` when it
/// names a folder, on GLES, with the shell's bar (in French) and two
/// windows: the fireworks mid-burst, the bar's row saying the tests
/// passed; the confetti flying, then settled on the windows' edges and
/// the bar's, the row saying why. Run by hand (`--run-ignored only`).
#[test]
#[ignore = "screenshots, by hand"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let (mut hp, mut config, _windows, _home) = common::effects_desk()?;
    config.effects.fireworks = true;
    config.effects.confetti = true;
    // The bar in full, its agents' rows shown.
    config.panel.form = hocus_compositor::FormChoice::Full;
    hp.reconfigure(config);
    run(&mut hp, Duration::from_secs(1))?;
    // The agent: a process of no window, nor above it (left by the shell
    // that started it), whose finish nobody sees at once; ours, stopped
    // by its own id.
    let started = Command::new("sh")
        .args(["-c", "sleep 300 >/dev/null 2>&1 & echo $!"])
        .output()?;
    let pid = String::from_utf8(started.stdout)?.trim().to_owned();
    anyhow::ensure!(pid.parse::<u32>().is_ok(), "no agent's process: {pid:?}");
    let shot = (|| -> anyhow::Result<()> {
        hocus(
            &hp,
            &["agent", "hocus-panda", "working", "--pid", &pid],
            None,
        )?;
        hocus(
            &hp,
            &[
                "agent",
                "hocus-panda",
                "done",
                "--pid",
                &pid,
                "--tests-passed",
                "2040",
            ],
            None,
        )?;
        let went = hp.dispatch_until(WAIT, |hp| hp.fireworks().is_some())?;
        anyhow::ensure!(went, "no fireworks");
        run(&mut hp, Duration::from_millis(3000))?;
        save(&mut hp, &out, "fireworks-mid-burst")?;
        run(&mut hp, Duration::from_millis(900))?;
        save(&mut hp, &out, "fireworks-later")?;
        run(&mut hp, Duration::from_secs(8))?;
        hocus(
            &hp,
            &["agent", "hocus-panda", "working", "--pid", &pid],
            None,
        )?;
        hocus(
            &hp,
            &["celebrate", "#791 mergée sur main", "--pid", &pid],
            None,
        )?;
        let went = hp.dispatch_until(WAIT, |hp| hp.confetti().is_some())?;
        anyhow::ensure!(went, "no confetti");
        run(&mut hp, Duration::from_millis(450))?;
        save(&mut hp, &out, "confetti-flying")?;
        run(&mut hp, Duration::from_millis(2350))?;
        save(&mut hp, &out, "confetti-settled")?;
        Ok(())
    })();
    let _ = Command::new("kill").arg(&pid).status();
    shot
}
