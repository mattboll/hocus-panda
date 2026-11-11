//! How the bar calls the user for the agents follows `[agents]` as the
//! file changes (6.16, issue #450): what lights the halo (`halo`), when a
//! waiting agent is reminded of (`remind`), for what a panda peeks out
//! (`peek`), and pandas or dots in the agents' rows (`pandas`). The
//! Settings' "Agents" page writes these keys; here they are written by
//! hand, as the page's worker would: beside the file, then renamed over.
//!
//! The real shell on the CPU, headless, in French, in a home of the
//! test's own, no session bus; its agents said with the `hocus` command.
//! Pixman draws no halo: what it would show is the model's
//! (`halo_concern`); its fades are measured by the halo's unit tests.
//! Each test waits for what it checks, never for a time.

use crate::common;

use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use common::{WAIT, wait};
use hocus_agents::tools::Places;
use hocus_compositor::{
    AgentsSettings, Config, Headless, HeadlessConfig, RendererChoice,
    halo::{Concern, Meeting},
};
use hocus_link::PeekKind;

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    hocus: PathBuf,
    file: PathBuf,
}

/// The compositor with the real shell on the CPU on a 1920 × 1080 screen,
/// its file `agents` (an `[agents]` table, or nothing); none when the
/// programs are not built.
fn start(agents: &str) -> anyhow::Result<Option<Bed>> {
    let shell = common::shell_program();
    let hocus = common::hocus_program();
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let home = root.join("home");
    for sub in ["home/bin", "config", "state/hocuspanda", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    // Day.1's first-run card set aside ("Plus tard"): the agents' group
    // starts under the clock.
    std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
    let file = root.join("config/config.toml");
    std::fs::write(&file, agents)?;
    // This test's process in the test's own `/proc`: no tool's client is
    // looked for in the machine's.
    let mine = home.join("proc").join(std::process::id().to_string());
    std::fs::create_dir_all(&mine)?;
    std::os::unix::fs::symlink(std::env::current_dir()?, mine.join("cwd"))?;
    let log = root.join("shell.log");
    let script = root.join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{config}' XDG_CACHE_HOME='{cache}' \
             XDG_STATE_HOME='{state}' XDG_DATA_HOME='{home}/.local/share' \
             HOCUS_CLOCK_AT='10:00:05' LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 \
             ICED_BACKEND=tiny-skia PATH='{home}/bin:/usr/bin:/bin' \
             RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            config = root.join("config").display(),
            cache = root.join("cache").display(),
            state = root.join("state").display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let config = Config {
        file: Some(file.clone()),
        agent_places: Some(Places {
            home: home.clone(),
            config_home: home.join(".config"),
            codex_home: home.join(".codex"),
            path: vec![home.join("bin")],
            herdr_socket: home.join(".config/herdr/herdr.sock"),
            processes: home.join("proc"),
            hocus: hocus.clone(),
            state_dir: Some(home.join("state")),
        }),
        // As the file says it at first: the compositor reads it at once.
        agents: toml::from_str::<Config>(agents)?.agents,
        plain_background: true,
        french: true,
        ..Config::default()
    };
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    hp.pointer_move(400.0, 600.0);
    Ok(Some(Bed {
        _dir: dir,
        hp,
        log,
        hocus,
        file,
    }))
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Bed {
    /// The file becomes `agents`, as the Settings write it, and the
    /// compositor has taken it.
    fn write(&mut self, agents: &str) -> anyhow::Result<()> {
        let new = self.file.with_extension("new");
        std::fs::write(&new, agents)?;
        std::fs::rename(&new, &self.file)?;
        let wanted = toml::from_str::<Config>(agents)?.agents;
        self.taken(wanted)
    }

    /// Waits until the compositor follows `wanted`.
    fn taken(&mut self, wanted: AgentsSettings) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.state().config.agents == wanted)?;
        anyhow::ensure!(
            done,
            "not taken: {:?}, not {wanted:?}",
            self.hp.state().config.agents
        );
        Ok(())
    }

    fn mark(&self) -> usize {
        plain(&self.log).len()
    }

    /// Waits until the shell logged, past `from`, a line holding all of
    /// `holds`; that line.
    fn logged_after(&mut self, from: usize, holds: &[&str]) -> anyhow::Result<String> {
        let log = self.log.clone();
        let find = |text: &str| {
            text.get(from..).and_then(|t| {
                t.lines()
                    .rev()
                    .find(|l| holds.iter().all(|h| l.contains(h)))
                    .map(str::to_owned)
            })
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| find(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "the shell never logged {holds:?}:\n{}", plain(&log));
        Ok(find(&plain(&log)).unwrap_or_default())
    }

    /// `hocus agent NAME STATE`, through the control socket, once the
    /// compositor's model has it.
    fn agent_is(&mut self, name: &str, state: &str) -> anyhow::Result<()> {
        let socket = self
            .hp
            .control_socket()
            .ok_or_else(|| anyhow::anyhow!("no control socket"))?
            .to_path_buf();
        let mut child = Command::new(&self.hocus)
            .arg("--socket")
            .arg(socket)
            .args(["agent", name, state])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.flush()?;
        }
        // The command waits for the compositor's answer: run it meanwhile.
        let done = self
            .hp
            .dispatch_until(WAIT, |_| matches!(child.try_wait(), Ok(Some(_))))?;
        anyhow::ensure!(done, "hocus agent {name} {state} never ended");
        anyhow::ensure!(child.wait()?.success(), "hocus agent {name} {state}");
        let known = self.hp.dispatch_until(WAIT, |hp| {
            hp.agents_peek()
                .iter()
                .any(|a| a.name == name && a.state == state)
        })?;
        anyhow::ensure!(known, "{name} not {state}");
        Ok(())
    }

    /// Agent `name`'s id.
    fn id_of(&self, name: &str) -> anyhow::Result<u64> {
        self.hp
            .agents_peek()
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.id)
            .ok_or_else(|| anyhow::anyhow!("no agent {name}"))
    }

    /// Waits until the halo would show `concern` (drawn on GLES).
    fn halo_is(&mut self, concern: Option<Concern>) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.halo_concern() == concern)?;
        anyhow::ensure!(
            done,
            "the halo shows {:?}, not {concern:?}",
            self.hp.halo_concern()
        );
        Ok(())
    }

    /// Waits until `told` pandas have peeked out so far.
    fn peeks_are(&mut self, told: u64) -> anyhow::Result<()> {
        let done = self.hp.dispatch_until(WAIT, |hp| hp.peeks_told() == told)?;
        anyhow::ensure!(done, "{} peeks, not {told}", self.hp.peeks_told());
        Ok(())
    }

    /// Where the rows of agents `names` are, top to bottom: their
    /// middles, global, once the shell logged its group's items with them
    /// in that order, in the bar's frame (from its outer right edge, its
    /// top).
    fn rows(&mut self, names: &[&str]) -> anyhow::Result<Vec<(f64, f64)>> {
        let wanted: Vec<String> = names
            .iter()
            .map(|n| Ok(format!("Agent({})@", self.id_of(n)?)))
            .collect::<anyhow::Result<_>>()?;
        let log = self.log.clone();
        let find = |text: &str| {
            let line = text
                .lines()
                .rev()
                .find(|l| l.contains("the agents group's items"))?;
            let places: Option<Vec<(f64, f64)>> = wanted
                .iter()
                .map(|w| {
                    let at = line.split(w.as_str()).nth(1)?.split_whitespace().next()?;
                    let (x, y) = at.split_once(',')?;
                    Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
                })
                .collect();
            places.filter(|p| p.windows(2).all(|w| w[0].1 < w[1].1))
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| find(&plain(&log)).is_some())?;
        anyhow::ensure!(done, "no rows {names:?}:\n{}", plain(&log));
        let right = f64::from(self.hp.outputs()[0].1.width) - 8.0;
        Ok(find(&plain(&log))
            .unwrap_or_default()
            .into_iter()
            .map(|(x, y)| (right + x, 8.0 + y))
            .collect())
    }

    /// The screen's pixel at `(x, y)`, as red, green, blue.
    fn pixel(&mut self, (x, y): (f64, f64)) -> anyhow::Result<[i32; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y as i32 * width + x as i32)? * 4;
        let p = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("({x}, {y}) is off the screen"))?;
        Ok([i32::from(p[2]), i32::from(p[1]), i32::from(p[0])])
    }

    /// Waits until the pixel at `at` is within 12 of `colour` on each
    /// channel (`near`), or is not (`!near`); the pixel then.
    fn pixel_becomes(
        &mut self,
        at: (f64, f64),
        colour: u32,
        near: bool,
    ) -> anyhow::Result<[i32; 3]> {
        let want = [colour >> 16, (colour >> 8) & 0xFF, colour & 0xFF].map(|c| c as i32);
        let close = |p: [i32; 3]| (0..3).all(|i| (p[i] - want[i]).abs() <= 12);
        let mut last = [0; 3];
        let deadline = std::time::Instant::now() + WAIT;
        while std::time::Instant::now() < deadline {
            last = self.pixel(at)?;
            if close(last) == near {
                return Ok(last);
            }
            self.hp.dispatch(Duration::from_millis(50))?;
        }
        anyhow::bail!("{at:?} is {last:?}, near {colour:06x}: {}", !near)
    }
}

/// Agent `name` (the only one, or the first) as if it had waited `waited`
/// by now.
fn waited(hp: &mut Headless, name: &str, waited: Duration) {
    let since = hp
        .agents_peek()
        .iter()
        .find(|a| a.name == name)
        .map_or(Duration::ZERO, |a| Duration::from_micros(a.since));
    let now = hp.state().now().saturating_sub(since);
    hp.backdate_agents(waited.saturating_sub(now));
}

/// `halo = "waiting"`: a finished agent and a stuck one light nothing, a
/// waiting one does; `"off"`: no agent does, a meeting still does; back
/// to `"all"`, the agents' states show again, in their order.
#[test]
fn halo_waiting_only_shows_no_glow_for_a_finished_agent() -> anyhow::Result<()> {
    let Some(mut bed) = start("[agents]\nhalo = \"waiting\"\n")? else {
        return Ok(());
    };
    bed.agent_is("api", "done")?;
    bed.agent_is("site", "failed")?;
    bed.halo_is(None)?;
    bed.agent_is("infra", "waiting")?;
    bed.halo_is(Some(Concern::Waiting))?;
    // Off: nothing of the agents; the meetings keep theirs.
    bed.write("[agents]\nhalo = \"off\"\n")?;
    bed.halo_is(None)?;
    bed.hp.meeting_near(Some(Meeting::InTen));
    bed.halo_is(Some(Concern::MeetingSoon))?;
    bed.hp.meeting_near(None);
    bed.halo_is(None)?;
    // All again: waiting first; answered, the stuck one, then the
    // finished one.
    bed.write("[agents]\nhalo = \"all\"\n")?;
    bed.halo_is(Some(Concern::Waiting))?;
    bed.agent_is("infra", "working")?;
    bed.halo_is(Some(Concern::Stuck))?;
    bed.agent_is("site", "working")?;
    bed.halo_is(Some(Concern::ToSee))?;
    // The table gone: the defaults, all of it.
    bed.write("")?;
    bed.halo_is(Some(Concern::ToSee))?;
    Ok(())
}

/// `peek = "off"`: no panda peeks out for a waiting agent; `"waiting"`:
/// none for a finished one, one for a waiting one; turned off while it is
/// out, it stays out (its 8 s), then none comes out again.
#[test]
fn peek_off_never_shows_the_panda() -> anyhow::Result<()> {
    let Some(mut bed) = start("[agents]\npeek = \"off\"\n")? else {
        return Ok(());
    };
    bed.agent_is("build", "working")?;
    let from = bed.mark();
    bed.agent_is("build", "waiting")?;
    // The shell shows it waiting in its row: the compositor has long
    // decided whether its panda peeks out.
    bed.logged_after(from, &["shows=AGENTS 1 · build t'attend"])?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.hp.peeks_told(), 0);
    assert_eq!(bed.hp.peek_out(), None);
    // The halo still calls.
    assert_eq!(bed.hp.halo_concern(), Some(Concern::Waiting));
    bed.write("[agents]\npeek = \"waiting\"\n")?;
    let from = bed.mark();
    bed.agent_is("api", "done")?;
    bed.logged_after(from, &["api à voir"])?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.hp.peeks_told(), 0);
    bed.agent_is("site", "waiting")?;
    bed.peeks_are(1)?;
    let site = bed.id_of("site")?;
    assert_eq!(
        bed.hp.peek_out().map(|(id, kind, _)| (id, kind)),
        Some((site, PeekKind::Waiting))
    );
    // Off while it is out: it stays out, not going back.
    bed.write("[agents]\npeek = \"off\"\n")?;
    assert_eq!(bed.hp.peek_out(), Some((site, PeekKind::Waiting, false)));
    let from = bed.mark();
    bed.agent_is("infra", "waiting")?;
    bed.logged_after(from, &["infra t'attend"])?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.hp.peeks_told(), 1);
    assert_eq!(bed.hp.peek_out().map(|(id, _, _)| id), Some(site));
    Ok(())
}

/// `remind = "2m"`: still waiting at its second minute, the agent's
/// panda comes out once more (the halo's reminder); `"never"`: an agent
/// waiting an hour is not reminded of; nor with `halo = "off"`, the
/// reminder being the halo's.
#[test]
fn the_reminder_comes_after_the_time_chosen_or_never() -> anyhow::Result<()> {
    let Some(mut bed) = start("[agents]\nremind = \"2m\"\n")? else {
        return Ok(());
    };
    bed.agent_is("build", "waiting")?;
    bed.peeks_are(1)?;
    // At 1 min 59 s of waiting, then over its second minute: the motions'
    // clock held and moved on. The clock moves, not the agent's start:
    // that is on the machine's clock, which counts from boot, and a test
    // may run less than 2 min after it (CI's machines do).
    bed.hp.hold_clock();
    let since = Duration::from_micros(bed.hp.agents_peek()[0].since);
    let to = (since + Duration::from_secs(119)).saturating_sub(bed.hp.now());
    let stepped = (|| {
        bed.hp.step(to)?;
        anyhow::ensure!(bed.hp.peeks_told() == 1, "reminded before 2 min");
        let start = bed.hp.now();
        while bed.hp.peeks_told() < 2 && bed.hp.now() - start < Duration::from_millis(2500) {
            bed.hp.step(common::STEP)?;
        }
        anyhow::Ok(())
    })();
    // From here the compositor's clock runs 2 min ahead of the machine's:
    // the agents that come start past it, and can be backdated that far.
    bed.hp.release_clock();
    stepped?;
    anyhow::ensure!(bed.hp.peeks_told() == 2, "no reminder at 2 min");
    // Never: a new one waiting an hour, nothing.
    bed.write("[agents]\nremind = \"never\"\n")?;
    bed.agent_is("api", "waiting")?;
    bed.peeks_are(3)?;
    bed.hp.backdate_agents(Duration::from_secs(3600));
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(bed.hp.peeks_told(), 3);
    // Two minutes again, without a halo for the agents: none either.
    bed.write("[agents]\nhalo = \"off\"\nremind = \"2m\"\n")?;
    bed.agent_is("site", "waiting")?;
    bed.peeks_are(4)?;
    waited(&mut bed.hp, "site", Duration::from_secs(121));
    wait(&mut bed.hp, Duration::from_millis(1500))?;
    assert_eq!(bed.hp.peeks_told(), 4);
    // The halo back: those still waiting past their second minute, not
    // reminded of yet, are at once (one panda, the latest's).
    bed.write("[agents]\nremind = \"2m\"\n")?;
    bed.peeks_are(5)?;
    Ok(())
}

/// `pandas = false`: each row shows a 10 px dot of its state's colour in
/// its panda's place (amber for a waiting agent, green for a working
/// one); turned on, the pandas are back.
#[test]
fn pandas_off_draws_dots_in_the_rows() -> anyhow::Result<()> {
    let Some(mut bed) = start("[agents]\npandas = false\n")? else {
        return Ok(());
    };
    bed.logged_after(0, &["rows=dots", "the agents' rows show them"])?;
    bed.agent_is("build", "working")?;
    bed.agent_is("infra", "waiting")?;
    // The panda's place: 6 px into the row, 42 px wide; a waiting agent's
    // card's 2 px in, 60 px wide (#649); the row's middle is 92 px in from
    // its left.
    let rows = bed.rows(&["infra", "build"])?;
    let (infra, build) = (
        (rows[0].0 - 92.0 + 2.0 + 30.0, rows[0].1),
        (rows[1].0 - 92.0 + 6.0 + 21.0, rows[1].1),
    );
    bed.pixel_becomes(infra, 0xF5B44A, true)?;
    bed.pixel_becomes(build, 0x6CC287, true)?;
    let from = bed.mark();
    bed.write("[agents]\npandas = true\n")?;
    bed.logged_after(from, &["rows=pandas"])?;
    bed.pixel_becomes(infra, 0xF5B44A, false)?;
    bed.pixel_becomes(build, 0x6CC287, false)?;
    Ok(())
}

/// The bar's right side, saved as `DIR/agents-live-WHAT.png`, and its
/// agents' group three times as large as `DIR/agents-live-WHAT-rows.png`.
fn shot(bed: &mut Bed, folder: &Path, what: &str) -> anyhow::Result<()> {
    common::at_rest(&mut bed.hp)?;
    let (width, height, bytes) = bed.hp.screenshot(0)?;
    let pixel = |x: i32, y: i32| {
        let at = usize::try_from(y * width + x).unwrap_or(0) * 4;
        bytes
            .get(at..at + 4)
            .map_or(image::Rgba([0, 0, 0, 255]), |p| {
                image::Rgba([p[2], p[1], p[0], 255])
            })
    };
    let w = 640.min(width);
    let image = image::RgbaImage::from_fn(w as u32, height as u32, |x, y| {
        pixel(width - w + x as i32, y as i32)
    });
    image.save(folder.join(format!("agents-live-{what}.png")))?;
    let (_, group) = bed
        .hp
        .drawn_layers(hocus_link::AGENTS_NAMESPACE)
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no agents' group"))?;
    let rows =
        image::RgbaImage::from_fn(group.width as u32 * 3, group.height as u32 * 3, |x, y| {
            pixel(group.x + x as i32 / 3, group.y + y as i32 / 3)
        });
    rows.save(folder.join(format!("agents-live-{what}-rows.png")))?;
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: five agents in
/// each state with their pandas, then with dots, and a waiting agent's
/// panda peeking out beside the rows' dots.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&folder)?;
    let Some(mut bed) = start("[agents]\npeek = \"off\"\n")? else {
        return Ok(());
    };
    for (name, state) in [
        ("build", "working"),
        ("notes", "idle"),
        ("site", "failed"),
        ("api", "done"),
        ("infra", "waiting"),
    ] {
        bed.agent_is(name, state)?;
    }
    bed.rows(&["infra", "site", "api", "build", "notes"])?;
    shot(&mut bed, &folder, "pandas")?;
    let rows = bed.rows(&["infra"])?;
    bed.write("[agents]\npeek = \"off\"\npandas = false\n")?;
    bed.pixel_becomes((rows[0].0 - 71.0, rows[0].1), 0xF5B44A, true)?;
    shot(&mut bed, &folder, "dots")?;
    bed.write("[agents]\npeek = \"waiting\"\npandas = false\n")?;
    bed.agent_is("infra", "working")?;
    bed.agent_is("infra", "waiting")?;
    bed.peeks_are(1)?;
    let out = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.peek().is_some_and(|l| l.alpha >= 1.0))?;
    anyhow::ensure!(out, "no peek");
    shot(&mut bed, &folder, "dots-peek")?;
    Ok(())
}
