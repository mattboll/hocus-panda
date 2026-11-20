//! The battery's charge limit (6.10, issue #896): `[power] charge-limit`
//! sets the battery's own threshold to 80 %, "Charger à 100 % une fois"
//! lifts it to 100 % until the next unplug, kept across a restart, and
//! off sets 100 %. Headless, in-process; the kernel's supplies a folder of
//! the test's own and the helper a script writing into it, never the
//! machine's `/sys` nor pkexec.

use crate::common;

use std::{os::unix::fs::PermissionsExt, path::PathBuf};

use common::WAIT;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, away::charge::Helper};

const END: &str = "charge_control_end_threshold";

/// A laptop's supplies (its battery's end threshold at 100, a mains
/// supply), and a helper writing what it is given into every battery's
/// end threshold, each call logged.
struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    fn new() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        let supply = |name: &str, files: &[(&str, &str)]| -> anyhow::Result<()> {
            let at = root.join("supplies").join(name);
            std::fs::create_dir_all(&at)?;
            for (file, text) in files {
                std::fs::write(at.join(file), format!("{text}\n"))?;
            }
            Ok(())
        };
        supply(
            "BAT0",
            &[("type", "Battery"), ("capacity", "64"), (END, "100")],
        )?;
        supply("AC", &[("type", "Mains"), ("online", "1")])?;
        let helper = root.join("charge-limit");
        std::fs::write(
            &helper,
            format!(
                "#!/bin/sh\nset -e\necho \"$*\" >> '{calls}'\nprintf '%s\\n' \"$1\" > '{end}'\n",
                calls = root.join("calls").display(),
                end = root.join("supplies/BAT0").join(END).display(),
            ),
        )?;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))?;
        std::fs::create_dir_all(root.join("state"))?;
        Ok(Self { dir })
    }

    fn config(&self, on: bool) -> Config {
        let root = self.dir.path();
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.power.charge_limit = on;
        config.state_dir = Some(root.join("state"));
        config.charge = Some(Helper {
            program: root.join("charge-limit"),
            pkexec: false,
            supplies: root.join("supplies"),
        });
        config
    }

    fn start(&self, on: bool) -> anyhow::Result<Headless> {
        Headless::start_with(
            HeadlessConfig {
                outputs: vec!["1280x800".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            self.config(on),
        )
    }

    fn end(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("supplies/BAT0").join(END))
            .unwrap_or_default()
            .trim()
            .to_owned()
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn lift_kept(&self) -> bool {
        self.state().join("charge-lifted").exists()
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    /// Waits until the battery's end threshold reads `want` and the shell
    /// is told `shown`.
    fn until(&self, hp: &mut Headless, want: &str, shown: Option<u8>) -> anyhow::Result<()> {
        let done =
            hp.dispatch_until(WAIT, |hp| self.end() == want && hp.charge_limit() == shown)?;
        anyhow::ensure!(
            done,
            "the threshold reads {:?} and the shell is told {:?}, not {want:?} and {shown:?}; calls {:?}",
            self.end(),
            hp.charge_limit(),
            self.calls()
        );
        Ok(())
    }
}

/// On: 80 % as the session starts, the gauge's mark told. "Charger à
/// 100 % une fois": 100 %, kept in the state's folder, and still after a
/// restart (written again by nobody: the battery holds it). Unplugged:
/// 80 % again, the lift forgotten. Off: 100 %.
#[test]
fn the_limit_writes_80_lifts_to_100_once_and_comes_back_when_unplugged() -> anyhow::Result<()> {
    let machine = Machine::new()?;
    let mut hp = machine.start(true)?;
    machine.until(&mut hp, "80", Some(80))?;
    // Lifted once.
    hp.charge_lift().map_err(anyhow::Error::msg)?;
    machine.until(&mut hp, "100", None)?;
    assert!(hp.charge_lifted());
    assert!(machine.lift_kept(), "the lift is not kept");
    // A restart keeps it: nothing written, the battery holds 100 already.
    drop(hp);
    let mut hp = machine.start(true)?;
    assert!(hp.charge_lifted());
    machine.until(&mut hp, "100", None)?;
    // Plugged in still: the next unplug ends it.
    hp.set_on_battery(true);
    machine.until(&mut hp, "80", Some(80))?;
    assert!(!hp.charge_lifted());
    assert!(!machine.lift_kept(), "the lift is still kept");
    // Plugged in and out again: nothing more to do.
    hp.set_on_battery(false);
    hp.set_on_battery(true);
    // Off: 100 %, the mark gone.
    hp.reconfigure(machine.config(false));
    machine.until(&mut hp, "100", None)?;
    assert_eq!(hp.charge_lift(), Err("the charge limit is off"));
    let calls = machine.calls();
    assert_eq!(calls, ["80", "100", "80", "100"], "{calls:?}");
    Ok(())
}
