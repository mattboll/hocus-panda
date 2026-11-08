//! `hocus msg secretary status` and `graph`: asked on her socket, beside
//! the control socket, as JSON. Her program is the one built beside
//! `hocus`; without it, the test skips.

use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn hocus(dir: &Path, args: &[&str]) -> anyhow::Result<(bool, String, String)> {
    let out = Command::new(env!("CARGO_BIN_EXE_hocus"))
        .arg("--socket")
        .arg(dir.join("hocus-0.control"))
        .args(["msg", "secretary"])
        .args(args)
        .env_clear()
        .output()?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

#[test]
fn her_status_and_her_graph_as_of_a_date() -> anyhow::Result<()> {
    let program = Path::new(env!("CARGO_BIN_EXE_hocus")).with_file_name("hocus-secretary");
    if !program.exists() {
        return Ok(());
    }
    let dir = tempfile::tempdir()?;
    let (ok, _, said) = hocus(dir.path(), &["status"])?;
    assert!(!ok && said.contains("assistant"), "{said}");
    let mut her = Command::new(&program)
        .arg("--socket")
        .arg(dir.path().join("hocus-0.secretary"))
        .arg("--data")
        .arg(dir.path().join("data"))
        .args([
            "--virtual-clock",
            "2026-11-09T08:00:00Z",
            "--until-stdin-closes",
        ])
        .env_clear()
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        let (ok, out, _) = hocus(dir.path(), &["status"])?;
        if ok {
            break out;
        }
        anyhow::ensure!(
            start.elapsed() < Duration::from_secs(20),
            "she never answered"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let status: serde_json::Value = serde_json::from_str(&status)?;
    assert_eq!(status["virtual_clock"], true);
    assert_eq!(status["key"], "file");
    let (ok, _, said) = hocus(dir.path(), &["graph", "Chapitô", "--as-of", "2026-04-15"])?;
    assert!(
        !ok && said.contains("Chapitô"),
        "she knows none yet: {said}"
    );
    drop(her.stdin.take());
    let ended = her.wait()?;
    assert!(ended.success(), "her pipe closed, she ended: {ended}");
    assert!(!dir.path().join("hocus-0.secretary").exists());
    Ok(())
}
