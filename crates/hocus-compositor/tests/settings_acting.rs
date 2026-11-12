//! Agir sur le bureau (#440, docs/design/mcp.md, scene 5): the real
//! `hocus-settings` window, a client of a headless compositor, on the
//! Agents' bed (its own home, configuration and state, no bus, no GPU);
//! `claude` a fake on its PATH that writes down what it was asked.
//!
//! The page lists an agent of the file and writes its rights back, its
//! comments kept, the compositor following them; « Brancher » runs
//! `claude mcp add` only after the sheet's own « Brancher », and adds the
//! agent read-only; « Débrancher » takes it out of both.

use crate::agents_bed;
use crate::common;

use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use agents_bed::{Bed, Machine, save};
use common::wait;
use serde_json::Value;

/// A file of the user's, before any agent.
const BY_HAND: &str = "# By hand.\n[keyboard]\nlayouts = [\"us\"]\n";

/// The first agent's card as the window shows it.
fn card(shown: &Value) -> &Value {
    &shown["acting"]["agents"][0]
}

/// The bed with `file` as its configuration, a fake `claude` on its PATH
/// when `claude`; `None` when the programs are not built.
fn bed(file: &str, claude: bool) -> anyhow::Result<Option<Bed>> {
    let Some(bed) = Bed::start(Machine::default())? else {
        return Ok(None);
    };
    let root = bed.root().to_path_buf();
    std::fs::write(root.join("config/hocuspanda/config.toml"), file)?;
    if claude {
        let fake = root.join("bin/claude");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
                root.join("claude.args").display()
            ),
        )?;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))?;
    }
    // Applications by name, for the chips.
    let apps = root.join("data/applications");
    std::fs::create_dir_all(&apps)?;
    for (id, name) in [
        ("kitty", "kitty"),
        ("org.mozilla.firefox", "Firefox"),
        ("hocus-settings", "Réglages"),
        ("com.bitwarden.desktop", "Bitwarden"),
        ("org.keepassxc.KeePassXC", "KeePassXC"),
    ] {
        std::fs::write(
            apps.join(format!("{id}.desktop")),
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={id}\n"),
        )?;
    }
    Ok(Some(bed))
}

/// Opened on the page, the file read.
fn open(bed: &mut Bed) -> anyhow::Result<Value> {
    bed.launch("act-on-desktop")?;
    bed.shows(|s| s["file"] == "saved" && s["page"] == "act-on-desktop")
}

fn args(bed: &Bed) -> String {
    std::fs::read_to_string(bed.root().join("claude.args")).unwrap_or_default()
}

#[test]
fn the_page_lists_an_agent_from_the_file_and_writes_its_rights_back() -> anyhow::Result<()> {
    let file =
        "# By hand.\n[[agents.mcp]]\n# Mine.\nname = \"Claude\"\naccess = \"read\" # for now\n";
    let Some(mut bed) = bed(file, false)? else {
        return Ok(());
    };
    let shown = open(&mut bed)?;
    assert_eq!(card(&shown)["name"], "Claude");
    assert_eq!(card(&shown)["access"], "look");
    assert_eq!(card(&shown)["line"], "Nommé dans ta configuration");
    assert_eq!(
        card(&shown)["rights"],
        serde_json::json!(["windows", "desktops", "input"])
    );
    // Looking only, its rights do nothing.
    bed.click("act:Claude:settings")?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(
        !std::fs::read_to_string(bed.root().join("config/hocuspanda/config.toml"))?
            .contains("settings")
    );
    // « Agir », then « Modifier les réglages » and the windows off.
    bed.click("act:Claude:act")?;
    bed.shows(|s| card(s)["access"] == "act" && s["toast"] == "Claude peut agir")?;
    bed.click("act:Claude:settings")?;
    bed.shows(|s| card(s)["rights"].as_array().is_some_and(|r| r.len() == 4))?;
    bed.click("act:Claude:windows")?;
    let written = bed.written(|t| t.contains("windows = false"))?;
    assert_eq!(
        written,
        "# By hand.\n[[agents.mcp]]\n# Mine.\nname = \"Claude\"\naccess = \"act\" # for now\n\
         settings = true\nwindows = false\n"
    );
    bed.compositor_follows(|agents| {
        agents.mcp.first().is_some_and(|a| {
            a.access == hocus_compositor::McpAccess::Act && a.settings && !a.windows
        })
    })?;
    // An application to click and type in, from those installed.
    bed.click("act:Claude:add")?;
    let shown = bed.shows(|s| s["acting"]["menu"] == "act:Claude:add")?;
    // The first: the machine's own applications come among the test's.
    let first = shown["acting"]["choices"][0]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no application in {shown}"))?
        .to_owned();
    bed.click("option:0")?;
    bed.written(|t| t.contains("apps = [\""))?;
    bed.shows(|s| card(s)["apps"] == serde_json::json!([first]))?;
    Ok(())
}

#[test]
fn brancher_runs_claude_only_after_its_click() -> anyhow::Result<()> {
    let Some(mut bed) = bed(BY_HAND, true)? else {
        return Ok(());
    };
    let shown = open(&mut bed)?;
    assert_eq!(shown["acting"]["agents"], serde_json::json!([]));
    bed.click("act:plug")?;
    let shown = bed.shows(|s| s["acting"]["sheet"]["client"] == "claude-code")?;
    assert_eq!(shown["acting"]["sheet"]["claude"], true);
    assert_eq!(
        shown["acting"]["sheet"]["line"],
        "claude mcp add --scope user hocuspanda -- hocus mcp --agent Claude"
    );
    // The sheet, then Esc: nothing run, nothing written.
    bed.key(common::KEY_ESC)?;
    bed.shows(|s| s["acting"]["sheet"].is_null())?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(args(&bed), "");
    assert_eq!(
        std::fs::read_to_string(bed.root().join("config/hocuspanda/config.toml"))?,
        BY_HAND
    );
    // Again, and its « Brancher ».
    bed.click("act:plug")?;
    bed.shows(|s| s["acting"]["sheet"]["client"] == "claude-code")?;
    bed.click("sheet:plug")?;
    let shown = bed.shows(|s| {
        card(s)["name"] == "Claude"
            && s["toast"] == "Claude Code branché"
            && s["acting"]["sheet"].is_null()
    })?;
    assert_eq!(
        args(&bed),
        "mcp add --scope user hocuspanda -- hocus mcp --agent Claude\n"
    );
    assert_eq!(card(&shown)["access"], "look");
    assert_eq!(card(&shown)["title"], "Claude Code");
    let written = bed.written(|t| t.contains("[[agents.mcp]]"))?;
    assert!(
        written.starts_with(BY_HAND)
            && written.contains("name = \"Claude\"\naccess = \"read\"\nclient = \"claude-code\"\n"),
        "{written}"
    );
    bed.compositor_follows(|agents| agents.mcp.iter().any(|a| a.name == "Claude"))?;
    Ok(())
}

#[test]
fn debrancher_takes_it_out_of_both() -> anyhow::Result<()> {
    let file = "[agents]\nhalo = \"all\"\n\n[[agents.mcp]]\nname = \"Claude\"\naccess = \"act\"\n\
                client = \"claude-code\"\nsince = \"2026-11-12\"\n";
    let Some(mut bed) = bed(file, true)? else {
        return Ok(());
    };
    let shown = open(&mut bed)?;
    assert_eq!(
        card(&shown)["line"],
        "Branché le 12 novembre · dans tous tes projets"
    );
    bed.click("act:Claude:unplug")?;
    bed.shows(|s| {
        s["acting"]["agents"] == serde_json::json!([]) && s["toast"] == "Claude Code débranché"
    })?;
    assert_eq!(
        bed.written(|t| !t.contains("agents.mcp"))?,
        "[agents]\nhalo = \"all\"\n"
    );
    let asked = bed.root().join("claude.args");
    let ran = bed.hp.dispatch_until(common::WAIT, |_| {
        std::fs::read_to_string(&asked).is_ok_and(|t| t.ends_with('\n'))
    })?;
    anyhow::ensure!(ran, "claude never asked");
    assert_eq!(args(&bed), "mcp remove --scope user hocuspanda\n");
    Ok(())
}

/// Scene 5 as the window draws it, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn shots() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let file = "[agents]\nconfirm-apps = [\"hocus-settings\", \"com.bitwarden.desktop\", \
                \"org.keepassxc.KeePassXC\"]\n\n[[agents.mcp]]\nname = \"Claude\"\naccess = \"act\"\n\
                settings = true\ninput = false\napps = [\"kitty\", \"org.mozilla.firefox\"]\n\
                client = \"claude-code\"\nsince = \"2026-11-12\"\n";
    let Some(mut bed) = bed(file, true)? else {
        return Ok(());
    };
    open(&mut bed)?;
    save(&mut bed, &out, "act-on-desktop.png")?;
    bed.click("act:plug")?;
    bed.shows(|s| s["acting"]["sheet"]["client"] == "other")?;
    save(&mut bed, &out, "act-on-desktop-sheet.png")?;
    drop(bed);
    // No agent yet, Claude Code's sheet without `claude` on the PATH.
    let Some(mut bed) = self::bed(BY_HAND, false)? else {
        return Ok(());
    };
    open(&mut bed)?;
    save(&mut bed, &out, "act-on-desktop-empty.png")?;
    bed.click("act:plug")?;
    bed.shows(|s| s["acting"]["sheet"]["client"] == "claude-code")?;
    save(&mut bed, &out, "act-on-desktop-claude.png")?;
    Ok(())
}
