//! Runs every scenario in the repository's `tests/scenarios/` as its own
//! test, each on its own headless compositor.
//!
//! `HOCUS_BLESS=1` records the screenshots as new references. Failed
//! comparisons leave their images in `target/test-artifacts/`.

use std::path::{Path, PathBuf};

use hocus_test::{
    bus::installed,
    scenario::{RunOptions, Scenario, ScenarioRenderer},
};
use libtest_mimic::{Arguments, Failed, Trial};

fn main() -> std::process::ExitCode {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(2).unwrap_or(manifest);
    let options = RunOptions {
        artifacts: root.join("target/test-artifacts"),
        bless: std::env::var_os("HOCUS_BLESS").is_some_and(|v| v == "1"),
        test_client: Some(PathBuf::from(env!("CARGO_BIN_EXE_hocus-test-client"))),
    };
    let trials = match scenario_files(&root.join("tests/scenarios")) {
        Ok(files) => files
            .into_iter()
            .map(|file| {
                let name = file
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let options = options.clone();
                // Without XWayland, D-Bus or a tool it runs (FreeBSD CI),
                // a scenario is skipped.
                let skip = Scenario::load(&file).is_ok_and(|s| {
                    (s.xwayland && !installed("Xwayland"))
                        || (s.dbus && !installed("dbus-daemon"))
                        || (s.renderer == ScenarioRenderer::Gles && !render_node())
                        || s.needs.iter().any(|n| !installed(n))
                });
                Trial::test(name, move || run(&file, &options)).with_ignored_flag(skip)
            })
            .collect(),
        Err(err) => vec![Trial::test("scenarios", move || {
            Err(Failed::from(format!("{err:#}")))
        })],
    };
    libtest_mimic::run(&Arguments::from_args(), trials).exit_code()
}

/// A GPU to draw with off screen (CI machines have none).
fn render_node() -> bool {
    std::fs::read_dir("/dev/dri").is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("renderD"))
    })
}

fn scenario_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    Ok(files)
}

fn run(file: &Path, options: &RunOptions) -> Result<(), Failed> {
    Scenario::load(file)
        .and_then(|scenario| scenario.run(options))
        .map_err(|err| Failed::from(format!("{err:#}")))
}
