//! Secrétaire (#759, the approved mockup ux2 §6): the real
//! `hocus-settings` window on her own page, a client of a headless
//! compositor whose notifications (the shell's history), Google accounts
//! (a fake Online Accounts on a private bus) and secretary's socket (a
//! fake) are the test's; her consent book in the test's data folder, her
//! configuration file the test's own, no GPU.
//!
//! One test per point: her switch writes her mode; in assistant mode only,
//! what she reads by account and the applications that notified; a switch
//! turned on writes nothing before its sheet's « Autoriser », then a
//! grant in the notebook chosen; turned off, a revocation, which
//! « Annuler » reads again; Travail | Privé writes the notebook of what
//! she reads of the account (or what its sheets propose); an Agenda's
//! calendars chosen; her name and her key; when the triage runs writes
//! `[secretary.triage] battery`; the triage's state and model from her
//! status. What the window shows is read from its log, never on a time.

use crate::agents_bed;
use crate::common;

use std::{os::unix::fs::PermissionsExt, path::PathBuf};

use agents_bed::{Bed, Machine, consents, save};
use common::{KEY_LEFTCTRL, WAIT, chord};
use hocus_secretary_link::{
    Notebook,
    consent::{ConsentBook, SourceKey, SourceKind},
};
use serde_json::Value;

const WORK: &str = "alex@atelier-lune.fr";
const PERSO: &str = "alex.martin@gmail.com";
const KEY_Z: u32 = 44;

fn notifier<'a>(shown: &'a Value, key: &str) -> &'a Value {
    shown["secretary"]["notifiers"]
        .as_array()
        .and_then(|n| n.iter().find(|n| n["key"] == key))
        .unwrap_or(&Value::Null)
}

fn google<'a>(shown: &'a Value, key: &str) -> &'a Value {
    shown["secretary"]["google"]
        .as_array()
        .and_then(|g| g.iter().find(|g| g["key"] == key))
        .unwrap_or(&Value::Null)
}

fn account<'a>(shown: &'a Value, id: &str) -> &'a Value {
    shown["secretary"]["accounts"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["account"] == id))
        .unwrap_or(&Value::Null)
}

fn book(bed: &Bed) -> anyhow::Result<ConsentBook> {
    Ok(serde_json::from_str(&consents(bed))?)
}

/// Her switch (secretary.md §21): `[secretary] mode` written "assistant",
/// then "classic" again, the compositor following; in classic mode, only
/// her switch shows.
#[test]
fn her_switch_writes_her_mode_and_her_rows_show_in_assistant_mode_only() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        notified: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open_secretary()?;
    let shown = bed.shows(|s| {
        s["secretary"]["notifiers"]
            .as_array()
            .is_some_and(|n| n.len() == 2)
    })?;
    assert_eq!(shown["secretary"]["assistant"], false);
    let places = bed.places_with("control:secretary.mode")?;
    assert!(places.get("switch:app:slack").is_none(), "classic: no row");
    assert!(places.get("secretary:name-field").is_none());
    bed.click("control:secretary.mode")?;
    bed.written(|t| t.contains("[secretary]") && t.contains("mode = \"assistant\""))?;
    let followed = bed.hp.dispatch_until(WAIT, |hp| {
        hp.state().config.secretary.mode == hocus_compositor::SecretaryMode::Assistant
    })?;
    assert!(followed, "the compositor follows");
    let shown = bed.shows(|s| s["secretary"]["assistant"] == true)?;
    let keys: Vec<&str> = shown["secretary"]["notifiers"]
        .as_array()
        .map(|n| n.iter().filter_map(|n| n["key"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        keys,
        ["slack", "signal"],
        "Slack first; an agent's left out"
    );
    bed.place_of("switch:app:slack")?;
    bed.place_of("control:secretary.triage.battery")?;
    assert!(consents(&bed).is_empty(), "nothing consented by itself");
    bed.click("control:secretary.mode")?;
    bed.written(|t| t.contains("mode = \"classic\""))?;
    Ok(())
}

/// An application's switch: on, its sheet, nothing written before its
/// « Autoriser »; « C'est privé » on the sheet; Travail | Privé on its
/// row; off, a revocation, and « Annuler » reads it again.
#[test]
fn an_application_s_switch_grants_from_its_sheet_and_revokes() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        notified: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open_secretary()?;
    bed.shows(|s| notifier(s, "slack")["granted"].is_null() && notifier(s, "signal").is_object())?;
    bed.click("switch:app:signal")?;
    bed.shows(|s| s["secretary"]["notify"] == "Signal")?;
    bed.click("sheet:not-now")?;
    bed.shows(|s| s["secretary"]["notify"].is_null())?;
    assert!(
        consents(&bed).is_empty(),
        "nothing before the sheet's Autoriser"
    );

    bed.click("switch:app:slack")?;
    let shown = bed.shows(|s| s["secretary"]["notify"] == "Slack")?;
    assert_eq!(
        shown["secretary"]["notify_notebook"], "pro",
        "Slack's is pro"
    );
    assert!(consents(&bed).is_empty());
    bed.click("sheet:allow")?;
    bed.shows(|s| notifier(s, "slack")["granted"] == "pro")?;
    let grant = book(&bed)?
        .notifications("Slack", Some("slack"))
        .expect("granted");
    assert_eq!(grant.notebook(), Notebook::Pro);

    bed.click("switch:app:signal")?;
    bed.shows(|s| s["secretary"]["notify"] == "Signal")?;
    bed.click("sheet:private")?;
    bed.shows(|s| s["secretary"]["notify_notebook"] == "perso")?;
    bed.click("sheet:allow")?;
    bed.shows(|s| notifier(s, "signal")["granted"] == "perso")?;
    // Travail | Privé on its row: work, still read; and back.
    bed.click("notebook:app:signal:pro")?;
    bed.shows(|s| notifier(s, "signal")["granted"] == "pro")?;
    bed.click("notebook:app:signal:perso")?;
    bed.shows(|s| notifier(s, "signal")["granted"] == "perso")?;

    // Off: a revocation; « Annuler » reads it again, without its sheet.
    bed.click("switch:app:slack")?;
    let shown = bed.shows(|s| notifier(s, "slack")["granted"].is_null())?;
    assert!(
        shown["secretary"]["notify"].is_null(),
        "no sheet to switch off"
    );
    assert!(
        book(&bed)?.notifications("Slack", None).is_none(),
        "revoked"
    );
    assert_eq!(shown["toast_action"], "undo");
    bed.click("toast-action")?;
    bed.shows(|s| notifier(s, "slack")["granted"] == "pro")?;
    let book = book(&bed)?;
    assert_eq!(
        book.notifications("Slack", None).map(|g| g.notebook()),
        Some(Notebook::Pro)
    );
    assert_eq!(
        book.notifications("Signal", None).map(|g| g.notebook()),
        Some(Notebook::Perso)
    );
    assert_eq!(book.records().len(), 6, "each change a later record");
    Ok(())
}

/// Each Google account a line with its kinds under it, its notebook
/// suggested by its domain; a kind's switch opens its sheet, in the
/// account's notebook; Travail | Privé on an account she reads nothing of
/// is what its sheets propose, on one she reads, the notebook of each
/// kind read; off, a revocation, `Ctrl+Z` reads it again.
#[test]
fn each_google_account_is_a_line_with_its_kinds_switched() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        google: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open_secretary()?;
    // The fake's ids are the accounts' e-mails.
    let work_calendar = format!("{WORK}:calendar");
    let work_mail = format!("{WORK}:mail");
    let perso_calendar = format!("{PERSO}:calendar");
    let shown = bed.shows(|s| {
        s["secretary"]["accounts"]
            .as_array()
            .is_some_and(|a| a.len() == 2)
    })?;
    assert_eq!(
        account(&shown, WORK)["kinds"],
        serde_json::json!([
            format!("{WORK}:calendar"),
            format!("{WORK}:mail"),
            format!("{WORK}:birthdays"),
        ])
    );
    assert_eq!(account(&shown, WORK)["notebook"], "pro");
    assert_eq!(account(&shown, PERSO)["notebook"], "perso", "gmail.com");

    bed.click(&format!("switch:{perso_calendar}"))?;
    let shown = bed.shows(|s| s["secretary"]["google_sheet"] == perso_calendar.as_str())?;
    assert_eq!(shown["secretary"]["google_notebook"], "perso");
    bed.click("sheet:not-now")?;
    bed.shows(|s| s["secretary"]["google_sheet"].is_null())?;
    // Nothing read of it: Travail is what its sheets propose now.
    bed.click(&format!("notebook:{PERSO}:pro"))?;
    bed.shows(|s| account(s, PERSO)["notebook"] == "pro")?;
    bed.click(&format!("switch:{perso_calendar}"))?;
    bed.shows(|s| {
        s["secretary"]["google_sheet"] == perso_calendar.as_str()
            && s["secretary"]["google_notebook"] == "pro"
    })?;
    bed.click("sheet:not-now")?;
    bed.shows(|s| s["secretary"]["google_sheet"].is_null())?;
    assert!(
        consents(&bed).is_empty(),
        "nothing before the sheet's Autoriser"
    );

    for kind in [&work_calendar, &work_mail] {
        bed.click(&format!("switch:{kind}"))?;
        bed.shows(|s| s["secretary"]["google_sheet"] == kind.as_str())?;
        bed.click("sheet:allow")?;
        bed.shows(|s| google(s, kind)["granted"] == "pro")?;
    }
    let grant = book(&bed)?
        .grant(&SourceKey::new(WORK, SourceKind::Calendar))
        .expect("granted");
    assert_eq!(grant.notebook(), Notebook::Pro);
    assert_eq!(grant.identity(), WORK);
    // Privé on the account's line: both kinds read, private.
    bed.click(&format!("notebook:{WORK}:perso"))?;
    bed.shows(|s| {
        google(s, &work_calendar)["granted"] == "perso"
            && google(s, &work_mail)["granted"] == "perso"
    })?;
    let shown = bed.shows(|s| account(s, WORK)["notebook"] == "perso")?;
    assert_eq!(shown["toast_action"], "undo");
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_Z]);
    bed.shows(|s| {
        google(s, &work_calendar)["granted"] == "pro" && google(s, &work_mail)["granted"] == "pro"
    })?;

    bed.click(&format!("switch:{work_calendar}"))?;
    bed.shows(|s| google(s, &work_calendar)["granted"].is_null())?;
    assert_eq!(
        book(&bed)?.revoked(),
        [SourceKey::new(WORK, SourceKind::Calendar)]
    );
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_Z]);
    bed.shows(|s| google(s, &work_calendar)["granted"] == "pro")?;
    assert!(book(&bed)?.revoked().is_empty(), "read again");
    Ok(())
}

/// #752 on her page: the Agenda's sheet lists the calendars she will read
/// (as she listed them, in her folder); one left out there is written with
/// the consent; on its row, « Elle en lit 3 sur 4 · Famille laissé de
/// côté », and « Choisir » lists them again, each to be left out or taken
/// back. « + Ajouter un compte Google » when GNOME's Online Accounts panel
/// is installed.
#[test]
fn the_agenda_says_the_calendars_she_reads_and_choisir_lists_them() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        google: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    let list = serde_json::json!({ WORK: [
        {"id": WORK, "name": "Travail"},
        {"id": "design@group.calendar.google.com", "name": "Équipe design", "shared_by": "lea@atelier-lune.fr"},
        {"id": "claire@atelier-lune.fr", "name": "Claire Dubois", "shared_by": "claire@atelier-lune.fr", "busy_only": true},
        {"id": "famille@group.calendar.google.com", "name": "Famille", "shared": true, "hidden": true},
    ]});
    let her = bed.root().join("data/hocuspanda/secretary");
    std::fs::create_dir_all(&her)?;
    std::fs::write(her.join("calendars.json"), list.to_string())?;
    // GNOME's panel, a fake on the Settings' PATH that says how it ran.
    let ran = bed.root().join("panel-ran");
    let panel = bed.root().join("bin/gnome-control-center");
    std::fs::write(
        &panel,
        format!("#!/bin/sh\necho \"$@\" > '{}'\n", ran.display()),
    )?;
    std::fs::set_permissions(&panel, std::fs::Permissions::from_mode(0o755))?;
    bed.open_secretary()?;
    let work_calendar = format!("{WORK}:calendar");
    bed.shows(|s| s["secretary"]["add_account"] == true)?;
    bed.click("secretary:add-google")?;
    let opened = bed.hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&ran).is_ok_and(|t| t.trim() == "online-accounts")
    })?;
    anyhow::ensure!(opened, "the panel never ran");
    bed.shows(|s| {
        google(s, &work_calendar)["calendars"]
            .as_array()
            .is_some_and(|c| c.len() == 4)
    })?;
    bed.click(&format!("switch:{work_calendar}"))?;
    bed.shows(|s| s["secretary"]["google_sheet"] == work_calendar.as_str())?;
    bed.click("sheet:calendar:3")?;
    bed.shows(|s| {
        s["secretary"]["google_left_out"]
            == serde_json::json!(["famille@group.calendar.google.com"])
    })?;
    bed.click("sheet:allow")?;
    let shown = bed.shows(|s| google(s, &work_calendar)["granted"] == "pro")?;
    assert_eq!(
        google(&shown, &work_calendar)["calendars_said"],
        "Elle en lit 3 sur 4 · Famille laissé de côté"
    );
    assert_eq!(
        book(&bed)?
            .grant(&SourceKey::new(WORK, SourceKind::Calendar))
            .expect("granted")
            .left_out(),
        ["famille@group.calendar.google.com"]
    );
    bed.click(&format!("choose:{work_calendar}"))?;
    bed.shows(|s| s["secretary"]["choosing"] == work_calendar.as_str())?;
    // Équipe design left out too, Famille taken back.
    bed.click(&format!("calendar:{work_calendar}:1"))?;
    bed.shows(|s| {
        google(s, &work_calendar)["left_out"]
            .as_array()
            .is_some_and(|l| l.len() == 2)
    })?;
    bed.click(&format!("calendar:{work_calendar}:3"))?;
    let shown = bed.shows(|s| {
        google(s, &work_calendar)["left_out"]
            == serde_json::json!(["design@group.calendar.google.com"])
    })?;
    assert_eq!(
        google(&shown, &work_calendar)["calendars_said"],
        "Elle en lit 3 sur 4 · Équipe design laissé de côté"
    );
    bed.click("sheet:close")?;
    bed.shows(|s| s["secretary"]["choosing"].is_null())?;
    Ok(())
}

/// Her name (#752): a field, « Ta secrétaire » greyed while empty; typed
/// and `Enter`, `[secretary] name` written.
#[test]
fn her_name_is_typed_in_its_field_and_written() -> anyhow::Result<()> {
    const KEY_M: u32 = 50;
    const KEY_A: u32 = 30;
    const KEY_R: u32 = 19;
    const KEY_G: u32 = 34;
    const KEY_O: u32 = 24;
    const KEY_T: u32 = 20;
    const KEY_ENTER: u32 = 28;
    const KEY_LEFTSHIFT: u32 = 42;
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open_secretary()?;
    let shown = bed.shows(|s| s["secretary"]["name"].is_string())?;
    assert_eq!(shown["secretary"]["name"], "");
    bed.click("secretary:name-field")?;
    bed.hp.key(KEY_LEFTSHIFT, true);
    bed.key(KEY_M)?;
    bed.hp.key(KEY_LEFTSHIFT, false);
    for code in [KEY_A, KEY_R, KEY_G, KEY_O, KEY_T] {
        bed.key(code)?;
    }
    bed.shows(|s| s["secretary"]["naming"] == "Margot")?;
    bed.key(KEY_ENTER)?;
    bed.written(|t| t.contains("name = \"Margot\""))?;
    bed.shows(|s| s["secretary"]["name"] == "Margot" && s["secretary"]["naming"].is_null())?;
    Ok(())
}

/// Her key (#752): « Clé Anthropic », a masked field while she misses it;
/// « Enregistrer » gives it to her over her socket, which says it refused
/// or kept; kept, « ✓ Enregistrée » with « Remplacer » and « Effacer »,
/// never the key; « Effacer » erases it. The key in no log.
#[test]
fn her_key_is_given_through_her_and_never_shown() -> anyhow::Result<()> {
    const KEY_S: u32 = 31;
    const KEY_K: u32 = 37;
    const KEY_MINUS: u32 = 12;
    const KEY_A: u32 = 30;
    const KEY_N: u32 = 49;
    const KEY_T: u32 = 20;
    const KEY_G: u32 = 34;
    const KEY_O: u32 = 24;
    const KEY_D: u32 = 32;
    const KEY_X: u32 = 45;
    const KEY_ENTER: u32 = 28;
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        her: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open_secretary()?;
    let shown = bed.shows(|s| s["secretary"]["key"]["field"] == true)?;
    assert_eq!(shown["secretary"]["key"]["kept"], false);
    let prefix = [KEY_S, KEY_K, KEY_MINUS, KEY_A, KEY_N, KEY_T, KEY_MINUS];
    bed.click("secretary:key-field")?;
    for code in prefix.iter().chain(&[KEY_X, KEY_X]) {
        bed.key(*code)?;
    }
    bed.shows(|s| s["secretary"]["key"]["typed"] == true)?;
    bed.key(KEY_ENTER)?;
    let shown = bed.shows(|s| s["secretary"]["key"]["said"].is_string())?;
    assert_eq!(
        shown["secretary"]["key"]["said"],
        // The words her drawer says too (#758): what to do.
        "Anthropic refuse cette clé. Vérifie qu’elle est complète, ou crée-en une autre dans la console."
    );
    assert_eq!(
        shown["secretary"]["key"]["typed"], false,
        "the field emptied"
    );
    bed.click("secretary:key-field")?;
    for code in prefix.iter().chain(&[KEY_G, KEY_O, KEY_O, KEY_D]) {
        bed.key(*code)?;
    }
    bed.shows(|s| s["secretary"]["key"]["typed"] == true)?;
    bed.click("secretary:key-save")?;
    bed.shows(|s| {
        s["secretary"]["key"]["kept"] == true && s["secretary"]["key"]["field"] == false
    })?;
    bed.click("secretary:key-erase")?;
    bed.shows(|s| s["secretary"]["key"]["kept"] == false)?;
    let given = bed
        .given
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        given,
        [
            Some("sk-ant-xx".to_owned()),
            Some("sk-ant-good".to_owned()),
            None
        ]
    );
    let log = common::logged(&bed.log);
    assert!(
        !log.contains("sk-ant-xx") && !log.contains("sk-ant-good"),
        "the key in no log"
    );
    Ok(())
}

/// LE TRI SUR CET ORDINATEUR: its state and its model as her status says
/// them, in plain words; « Quand trier » writes `[secretary.triage]
/// battery`, which `Ctrl+Z` takes back.
#[test]
fn the_triage_says_its_state_and_quand_trier_writes_the_battery_key() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        her: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    bed.open_secretary()?;
    let shown = bed.shows(|s| s["secretary"]["triage"].is_string())?;
    assert_eq!(
        shown["secretary"]["triage"],
        "En pause : la batterie est à 32 %. 21 en attente."
    );
    assert_eq!(
        shown["secretary"]["model"],
        "Ollama, sur cet ordinateur · qwen3:8b · prêt"
    );
    bed.choose("secretary.triage.battery", 3)?;
    let text = bed.written(|t| t.contains("battery = 80"))?;
    assert!(text.contains("[secretary.triage]"), "{text}");
    // The file is written before the page offers its undo: Ctrl+Z only
    // once the toast says it can.
    bed.shows(|s| s["toast_action"] == "undo")?;
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_Z]);
    bed.written(|t| !t.contains("battery = 80"))?;
    Ok(())
}

/// The page's screenshots, into `HOCUS_SHOTS`.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_of_her_page() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let Some(mut bed) = Bed::start(Machine {
        assistant: true,
        notified: true,
        google: true,
        her: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    let list = serde_json::json!({ WORK: [
        {"id": WORK, "name": "Travail"},
        {"id": "design@group.calendar.google.com", "name": "Équipe design", "shared_by": "lea@atelier-lune.fr"},
        {"id": "claire@atelier-lune.fr", "name": "Claire Dubois", "shared_by": "claire@atelier-lune.fr", "busy_only": true},
        {"id": "famille@group.calendar.google.com", "name": "Famille", "shared": true, "hidden": true},
    ]});
    let her = bed.root().join("data/hocuspanda/secretary");
    std::fs::create_dir_all(&her)?;
    std::fs::write(her.join("calendars.json"), list.to_string())?;
    let panel = bed.root().join("bin/gnome-control-center");
    std::fs::write(&panel, "#!/bin/sh\n")?;
    std::fs::set_permissions(&panel, std::fs::Permissions::from_mode(0o755))?;
    // Her name, her key, and what she reads, as the mockup shows them.
    let file = bed.root().join("config/hocuspanda/config.toml");
    let text = std::fs::read_to_string(&file)?;
    std::fs::write(
        &file,
        text.replace(
            "mode = \"assistant\"",
            "mode = \"assistant\"\nname = \"Margot\"",
        ),
    )?;
    let mut consents = ConsentBook::default();
    let at = jiff::Timestamp::now();
    consents.allow_leaving_out(
        SourceKey::new(WORK, SourceKind::Calendar),
        WORK,
        Notebook::Pro,
        vec!["famille@group.calendar.google.com".to_owned()],
        at,
    );
    consents.allow(
        SourceKey::new(WORK, SourceKind::Mail),
        WORK,
        Notebook::Pro,
        at,
    );
    consents.allow(
        SourceKey::notifications("Slack", None),
        "Slack",
        Notebook::Pro,
        at,
    );
    consents.save(&her.join("consents.json"))?;
    bed.open_secretary()?;
    bed.shows(|s| {
        s["secretary"]["key"]["field"] == true
            && s["secretary"]["accounts"]
                .as_array()
                .is_some_and(|a| a.len() == 2)
    })?;
    save(&mut bed, &out, "secretary-page-key-missing.png")?;
    bed.click("secretary:key-field")?;
    for code in [31, 37, 12, 30, 49, 20, 12, 34, 24, 24, 32] {
        bed.key(code)?;
    }
    bed.click("secretary:key-save")?;
    bed.shows(|s| s["secretary"]["key"]["kept"] == true)?;
    wait_toast_gone(&mut bed)?;
    save(&mut bed, &out, "secretary-page-top.png")?;
    bed.reveal("row:secretary.notifier.signal")?;
    save(&mut bed, &out, "secretary-page-reads.png")?;
    bed.reveal("row:secretary.model")?;
    save(&mut bed, &out, "secretary-page-triage.png")?;
    bed.click("control:secretary.triage.battery")?;
    bed.shows(|s| s["menu"] == "secretary.triage.battery")?;
    save(&mut bed, &out, "secretary-page-quand-trier.png")?;
    bed.key(1)?; // Esc
    bed.shows(|s| s["menu"].is_null())?;
    bed.click(&format!("choose:{WORK}:calendar"))?;
    bed.shows(|s| s["secretary"]["choosing"].is_string())?;
    save(&mut bed, &out, "secretary-page-choisir.png")?;
    bed.click("sheet:close")?;
    bed.shows(|s| s["secretary"]["choosing"].is_null())?;
    bed.click(&format!("switch:{PERSO}:mail"))?;
    bed.shows(|s| s["secretary"]["google_sheet"].is_string())?;
    save(&mut bed, &out, "secretary-page-sheet.png")?;
    bed.click("sheet:not-now")?;
    bed.shows(|s| s["secretary"]["google_sheet"].is_null())?;
    bed.click("control:secretary.mode")?;
    bed.shows(|s| s["secretary"]["assistant"] == false)?;
    save(&mut bed, &out, "secretary-page-classic.png")?;
    let Some(mut agents) = Bed::start(Machine {
        claude: true,
        assistant: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    agents.open()?;
    agents.reveal("link:secretary")?;
    save(&mut agents, &out, "agents-page-line-to-secretary.png")?;
    Ok(())
}

/// The toast gone, for a screenshot without it.
fn wait_toast_gone(bed: &mut Bed) -> anyhow::Result<()> {
    bed.shows(|s| s["toast"].is_null())?;
    Ok(())
}
