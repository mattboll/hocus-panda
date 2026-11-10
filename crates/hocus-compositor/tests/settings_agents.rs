//! Agents (6.16, issue #450, variant A): the real `hocus-settings` window,
//! a client of a headless compositor (one 2560 × 1600 screen) whose
//! agents' tools are looked for in the test's own home, as the Settings
//! read them there too; its configuration file, state and runtime
//! directories the test's own, no bus, no GPU; herdr a fake on a socket of
//! the test's.
//!
//! One test per point of the spec: the three tools and their states;
//! "Brancher" writing nothing before the consent, then plugging in;
//! "Annuler" after "Débrancher" plugging it back; a declined tool plugged
//! in from the page and its refusal cleared; "Suivre herdr" off declining
//! it, its agents gone; the halo for a waiting agent only, the preview
//! saying so; the peek never; the rows' pandas as dots; no tool at all.
//! What the window shows is read from its log, never on a time.

use crate::agents_bed;
use crate::common;

use std::{path::PathBuf, time::Duration};

use agents_bed::{Bed, Machine, ORIGINAL, save};
use common::{WAIT, wait};
use hocus_compositor::{HaloRule, PeekRule, halo::Concern};
use serde_json::Value;

/// The tool `word`'s row as the window shows it.
fn tool<'a>(shown: &'a Value, word: &str) -> &'a Value {
    shown["agents"]["tools"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["tool"] == word))
        .unwrap_or(&Value::Null)
}

#[test]
fn the_page_lists_the_three_tools_with_their_states() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        codex: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| tool(s, "codex")["row"] == "found")?;
    let rows: Vec<(&str, &str, &str)> = shown["agents"]["tools"]
        .as_array()
        .map(|t| {
            t.iter()
                .map(|t| {
                    (
                        t["tool"].as_str().unwrap_or(""),
                        t["state"].as_str().unwrap_or(""),
                        t["action"].as_str().unwrap_or(""),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        rows,
        [
            ("claude-code", "Trouvé, pas branché", "plug"),
            ("codex", "Trouvé, pas branché", "plug"),
            ("herdr", "Pas sur cet ordinateur", "none"),
        ]
    );
    assert_eq!(shown["agents"]["count"], "0 suivi · aucun agent en cours");
    assert_eq!(shown["agents"]["none"], false);
    // The halo's and the pandas' rows, the model's.
    for id in [
        "row:agents.halo",
        "row:agents.remind",
        "row:agents.preview",
        "row:agents.peek",
        "row:agents.pandas",
        "row:agents.rest",
        "row:agents.another",
        "agents:copy",
    ] {
        bed.place_of(id)?;
    }
    // An agent running is counted.
    bed.agent_is("build", "working")?;
    bed.shows(|s| s["agents"]["count"] == "0 suivi · 1 agent en cours")?;
    Ok(())
}

#[test]
fn brancher_writes_nothing_before_the_consent_then_plugs_in() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| tool(s, "claude-code")["action"] == "plug")?;
    // The sheet, then Esc: nothing written.
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code" && s["agents"]["consent_on"] == "plug")?;
    bed.place_of("sheet:plug")?;
    assert_eq!(std::fs::read_to_string(bed.claude_file())?, ORIGINAL);
    bed.key(common::KEY_ESC)?;
    bed.shows(|s| s["agents"]["consent"].is_null())?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(std::fs::read_to_string(bed.claude_file())?, ORIGINAL);
    // Again, and its "Brancher": the hooks written, a copy kept.
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code")?;
    bed.click("sheet:plug")?;
    bed.shows(|s| {
        tool(s, "claude-code")["row"] == "plugged"
            && s["toast"] == "Claude Code branché"
            && s["agents"]["consent"].is_null()
    })?;
    let now = std::fs::read_to_string(bed.claude_file())?;
    assert!(now.contains("agent-hook claude-code"), "{now}");
    assert_eq!(
        std::fs::read_to_string(
            bed.root()
                .join("home/.claude/settings.json.before-hocuspanda")
        )?,
        ORIGINAL
    );
    bed.shows(|s| s["agents"]["count"] == "1 suivi · aucun agent en cours")?;
    Ok(())
}

#[test]
fn annuler_after_debrancher_plugs_it_back() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code")?;
    bed.key(28)?; // Enter: "Brancher" holds the keyboard.
    bed.shows(|s| tool(s, "claude-code")["action"] == "unplug")?;
    // "Débrancher": no consent, the file as it was.
    bed.click("tool:claude-code")?;
    bed.shows(|s| {
        tool(s, "claude-code")["action"] == "plug"
            && s["toast"] == "Claude Code débranché"
            && s["agents"]["consent"].is_null()
    })?;
    assert_eq!(std::fs::read_to_string(bed.claude_file())?, ORIGINAL);
    // "Annuler": plugged back in, without asking.
    bed.click("toast-action")?;
    bed.shows(|s| {
        tool(s, "claude-code")["action"] == "unplug" && s["agents"]["consent"].is_null()
    })?;
    assert!(std::fs::read_to_string(bed.claude_file())?.contains("agent-hook claude-code"));
    Ok(())
}

#[test]
fn a_declined_tool_is_plugged_in_from_the_page_and_its_refusal_cleared() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        codex: true,
        codex_declined: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| {
        tool(s, "codex")["row"] == "declined" && tool(s, "codex")["declined_on"].is_i64()
    })?;
    assert_eq!(tool(&shown, "codex")["state"], "Refusé");
    assert_eq!(tool(&shown, "codex")["action"], "plug");
    bed.click("tool:codex")?;
    bed.shows(|s| s["agents"]["consent"] == "codex")?;
    bed.click("sheet:plug")?;
    // Plugged, its hooks to approve in Codex; no longer declined.
    let shown = bed.shows(|s| tool(s, "codex")["row"] == "to-approve")?;
    assert_eq!(tool(&shown, "codex")["state"], "Branché · à approuver");
    let config = std::fs::read_to_string(bed.root().join("home/.codex/config.toml"))?;
    assert!(config.contains("agent-hook"), "{config}");
    let memory = bed.memory();
    assert!(memory.contains("declined = []"), "{memory}");
    assert!(!memory.contains("codex = "), "{memory}");
    Ok(())
}

#[test]
fn following_herdr_off_declines_it_and_its_agents_go() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        herdr: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| {
        tool(s, "herdr")["row"] == "running" && tool(s, "herdr")["action"] == "follow-on"
    })?;
    let followed = bed.agents_until(|a| !a.is_empty())?;
    anyhow::ensure!(followed, "herdr's agent never came");
    bed.shows(|s| s["agents"]["count"] == "1 suivi · 1 agent en cours")?;
    bed.click("tool:herdr")?;
    bed.shows(|s| {
        tool(s, "herdr")["row"] == "running-ignored"
            && tool(s, "herdr")["action"] == "follow-off"
            && s["toast"] == "herdr n’est plus suivi"
    })?;
    let gone = bed.agents_until(<[_]>::is_empty)?;
    anyhow::ensure!(gone, "herdr's agents stay: {:?}", bed.hp.agents());
    assert!(bed.memory().contains("\"herdr\""), "{}", bed.memory());
    // On again: followed, its agent back.
    bed.click("tool:herdr")?;
    bed.shows(|s| tool(s, "herdr")["action"] == "follow-on")?;
    let back = bed.agents_until(|a| !a.is_empty())?;
    anyhow::ensure!(back, "herdr's agent never came back");
    Ok(())
}

#[test]
fn halo_waiting_only_shows_no_glow_for_a_finished_agent() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    bed.open()?;
    bed.choose("agents.halo", 1)?;
    bed.written(|t| t.contains("[agents]\nhalo = \"waiting\""))?;
    bed.shows(|s| {
        s["toast"]
            .as_str()
            .is_some_and(|t| t.starts_with("Le halo"))
    })?;
    bed.compositor_follows(|a| a.halo == HaloRule::Waiting)?;
    bed.agent_is("api", "done")?;
    assert_eq!(bed.hp.halo_concern(), None);
    bed.agent_is("infra", "waiting")?;
    let lit = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.halo_concern() == Some(Concern::Waiting))?;
    anyhow::ensure!(lit, "no halo for a waiting agent");
    // The preview: no halo for a finished agent, one for a waiting one.
    bed.click("preview:done")?;
    bed.shows(|s| s["agents"]["played"] == "done" && s["agents"]["preview_halo"] == false)?;
    bed.click("preview:waiting")?;
    bed.shows(|s| s["agents"]["played"] == "waiting" && s["agents"]["preview_halo"] == true)?;
    // Never: the reminder's row is inert.
    bed.choose("agents.halo", 2)?;
    bed.shows(|s| s["agents"]["remind_inert"] == true)?;
    bed.compositor_follows(|a| a.halo == HaloRule::Off)?;
    Ok(())
}

#[test]
fn peek_off_never_shows_the_panda() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    bed.open()?;
    bed.choose("agents.peek", 2)?;
    bed.written(|t| t.contains("peek = \"off\""))?;
    bed.compositor_follows(|a| a.peek == PeekRule::Off)?;
    // The reminder's menu, written as the compositor reads it.
    bed.choose("agents.remind", 0)?;
    bed.written(|t| t.contains("remind = \"2m\""))?;
    bed.compositor_follows(|a| a.remind == hocus_compositor::Remind::Two)?;
    Ok(())
}

#[test]
fn pandas_off_draws_dots_in_the_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    let shown = bed.open()?;
    assert_eq!(shown["agents"]["pandas"], true);
    bed.click("control:agents.pandas")?;
    bed.written(|t| t.contains("pandas = false"))?;
    bed.shows(|s| s["agents"]["pandas"] == false)?;
    bed.compositor_follows(|a| !a.pandas)?;
    Ok(())
}

#[test]
fn no_tool_shows_the_dashed_line_and_the_command() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine::default())? else {
        return Ok(());
    };
    bed.open()?;
    let shown = bed.shows(|s| s["agents"]["none"] == true)?;
    assert_eq!(shown["agents"]["count"], "0 suivi · aucun agent en cours");
    bed.place_of("agents:none")?;
    bed.click("agents:copy")?;
    bed.shows(|s| s["agents"]["copied"] == true)?;
    Ok(())
}

/// At rest on the page, the window draws nothing: its pandas hopped as it
/// opened, the preview played its wave, then nothing moves.
#[test]
fn at_rest_the_page_commits_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        motion: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.click("preview:waiting")?;
    bed.shows(|s| s["agents"]["played"] == "waiting")?;
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..200 {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 10 {
            break;
        }
    }
    anyhow::ensure!(still >= 10, "the window never came to rest");
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.window_commits(), last, "the window drew at rest");
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start(Machine {
        claude: true,
        codex: true,
        codex_declined: true,
        herdr: true,
        motion: true,
    })?
    else {
        return Ok(());
    };
    bed.open()?;
    bed.shows(|s| {
        tool(s, "codex")["declined_on"].is_i64() && tool(s, "herdr")["row"] == "running"
    })?;
    save(&mut bed, &out, "agents-page.png")?;
    bed.click("tool:claude-code")?;
    bed.shows(|s| s["agents"]["consent"] == "claude-code")?;
    save(&mut bed, &out, "agents-sheet.png")?;
    bed.click("sheet:plug")?;
    bed.shows(|s| tool(s, "claude-code")["row"] == "plugged")?;
    save(&mut bed, &out, "agents-plugged.png")?;
    bed.click("preview:waiting")?;
    bed.shows(|s| s["agents"]["played"] == "waiting")?;
    save(&mut bed, &out, "agents-halo.png")?;
    bed.choose("agents.halo", 1)?;
    bed.written(|t| t.contains("halo = \"waiting\""))?;
    bed.click("preview:done")?;
    bed.shows(|s| s["agents"]["preview_halo"] == false)?;
    save(&mut bed, &out, "agents-halo-waiting-only.png")?;
    bed.click("control:agents.pandas")?;
    bed.shows(|s| s["agents"]["pandas"] == false)?;
    bed.reveal("row:agents.rest")?;
    save(&mut bed, &out, "agents-pandas-dots.png")?;
    bed.click("control:agents.pandas")?;
    bed.shows(|s| s["agents"]["pandas"] == true)?;
    save(&mut bed, &out, "agents-pandas.png")?;
    let Some(mut none) = Bed::start(Machine {
        motion: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    none.open()?;
    none.shows(|s| s["agents"]["none"] == true)?;
    save(&mut none, &out, "agents-none.png")?;
    Ok(())
}
