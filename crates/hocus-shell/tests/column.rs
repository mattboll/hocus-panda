//! The column as drawn, and what its buttons do.

use std::time::Duration;

use hocus_shell::{
    Action, ActiveWindow, Column, ColumnState, KeyboardLayout, Lang, NotificationLine,
    ScreenDesktops, TrayApp, TrayMenuEntry, WallTime,
    activity::Badge,
    agents::{AgentAsk, AgentState, AgentsView, Board, Elsewhere},
    column::{AdbLine, Agenda, Meeting, PairStage, Pairing, PhoneLine},
    journal::{Did, JournalLine, JournalView, Pressed},
    tasks::{Due, DueChoice, ListMark, TaskRow, TasksView},
};
use hocus_ui::{Input, Point, Role, Theme};

mod common;

fn state() -> ColumnState {
    ColumnState {
        can_hibernate: false,
        status: Default::default(),
        tray: Vec::new(),
        sharing: false,
        tasks: None,
        updates: None,
        phone: None,
        meeting: None,
        calendar_set: false,
        agenda: Default::default(),
        agents: None,
        journal: None,
        time: WallTime {
            hour: 14,
            minute: 37,
            weekday: 2,
            day: 21,
            month: 10,
            ..WallTime::default()
        },
        active: Some(ActiveWindow {
            title: "foot — ~/github/hocus-panda: cargo nextest run --workspace".into(),
            maximized: false,
        }),
        screens: vec![
            ScreenDesktops {
                output: "DP-1".into(),
                count: 4,
                shown: 1,
                name: String::new(),
                ..ScreenDesktops::default()
            },
            ScreenDesktops {
                output: "eDP-1".into(),
                count: 1,
                shown: 1,
                name: String::new(),
                ..ScreenDesktops::default()
            },
        ],
        layouts: vec![KeyboardLayout {
            name: "French (AZERTY)".into(),
            short: "FR".into(),
        }]
        .into(),
        layout: 0,
        switch_layout_keys: Some("Super+Ctrl+Space".into()),
        user: "Camille".into(),
        notifications: vec![
            line(1, "Mail", "Revue de la MR #42"),
            line(2, "Chat", "Les maquettes du dock sont prêtes"),
        ],
        notification_count: 2,
    }
}

fn line(id: u32, app: &str, summary: &str) -> NotificationLine {
    NotificationLine {
        id,
        app: app.into(),
        summary: summary.into(),
    }
}

fn column(lang: Lang) -> Column {
    let mut column = Column::new(Theme::default(), lang, 760.0, 1.0);
    column.set_state(&state());
    column.draw(Duration::ZERO);
    column
}

/// The centre of the button named `name`, relative to the column.
fn button(column: &Column, name: &str) -> anyhow::Result<Point> {
    let access = column.access();
    let node = access
        .walk()
        .into_iter()
        .find(|n| n.role == Role::Button && n.label.as_deref() == Some(name))
        .ok_or_else(|| anyhow::anyhow!("no button {name:?} in\n{}", access.dump()))?;
    let r = node.rect;
    Ok(Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
}

fn click(column: &mut Column, at: Point) -> Vec<Action> {
    column.input(at, Input::PointerMove(at));
    column.input(at, Input::PointerButton { pressed: true });
    column.input(at, Input::PointerButton { pressed: false })
}

#[test]
fn the_column_matches_its_references() -> anyhow::Result<()> {
    let column = column(Lang::Fr);
    common::expect_reference(&column.view(), "column-fr.png")?;
    let column = self::column(Lang::En);
    common::expect_reference(&column.view(), "column-en.png")?;
    Ok(())
}

/// Two agents at work, one of them waiting for an answer, and eleven at
/// rest, as board S3 shows them.
fn agents(pandas: bool) -> AgentsView {
    let mut board = Board::default();
    let mut list = vec![
        ("w1:p1".into(), "refacto".into(), AgentState::Work, None),
        ("w1:p2".into(), "tests".into(), AgentState::Work, None),
    ];
    for i in 3..14 {
        list.push((
            format!("w1:p{i}"),
            format!("agent-{i}"),
            AgentState::Rest,
            None,
        ));
    }
    board.replace(0, list, Duration::ZERO);
    let now = Duration::from_secs(180);
    board.set(
        "w1:p1",
        AgentState::Wait,
        Some("Push the branch?".into()),
        now,
    );
    board
        .view(now + Duration::from_secs(60), pandas, false, &[])
        .expect("agents")
}

#[test]
fn the_agents_show_and_the_waiting_one_is_answered() -> anyhow::Result<()> {
    let mut column = Column::new(Theme::default(), Lang::En, 900.0, 1.0);
    let mut s = state();
    s.agents = Some(agents(true));
    column.set_state(&s);
    // The panda that waits moved from minute 3 on; long after, it holds still.
    column.draw(Duration::from_secs(600));
    common::expect_reference(&column.view(), "column-agents.png")?;
    for (name, ask) in [
        ("Yes to refacto", AgentAsk::Yes),
        ("No to refacto", AgentAsk::No),
        ("Show refacto", AgentAsk::Show),
    ] {
        let at = button(&column, name)?;
        assert_eq!(
            click(&mut column, at),
            vec![Action::Agent {
                key: "w1:p1".into(),
                ask
            }]
        );
    }
    let at = button(&column, "tests · Works · 4 min")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::Agent {
            key: "w1:p2".into(),
            ask: AgentAsk::Show
        }]
    );
    // Without pandas, a dot of each one's colour.
    s.agents = Some(agents(false));
    column.set_state(&s);
    column.draw(Duration::from_secs(601));
    let access = column.access().dump();
    assert!(!access.contains("Panda"), "{access}");
    assert!(access.contains("11 others at rest in herdr"), "{access}");
    Ok(())
}

fn journal(stopped: bool) -> JournalView {
    let line = |did, target: &str, refused: Option<&str>| JournalLine {
        agent: "helper".into(),
        did,
        target: target.into(),
        time: "14:37".into(),
        refused: refused.map(str::to_owned),
    };
    JournalView {
        stopped,
        seats: Vec::new(),
        lines: vec![
            line(
                Did::Click {
                    button: Pressed::Left,
                    count: 1,
                },
                "Passwords",
                Some("agent \"helper\" may not act on windows of \"other\""),
            ),
            line(
                Did::Type {
                    characters: 4,
                    text: None,
                },
                "Shopping list",
                None,
            ),
            line(Did::Keys(Some("Ctrl+S".into())), "Shopping list", None),
            line(Did::Scroll { down: true }, "Shopping list", None),
        ],
        total: 4,
    }
}

#[test]
fn the_journal_shows_the_latest_opens_on_all_and_stops_every_agent() -> anyhow::Result<()> {
    let mut column = Column::new(Theme::default(), Lang::En, 900.0, 1.0);
    let mut s = state();
    s.journal = Some(journal(false));
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    assert!(
        access.contains("helper · Click refused · Passwords"),
        "{access}"
    );
    // Three in the column, newest first; the fourth only in the popover.
    assert!(!access.contains("Scrolled down"), "{access}");
    let at = button(&column, "Stop every agent")?;
    assert_eq!(click(&mut column, at), vec![Action::StopAgents]);
    let at = button(&column, "Agents' journal")?;
    assert!(click(&mut column, at).is_empty());
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    assert!(
        access.contains("helper · Scrolled down · Shopping list"),
        "{access}"
    );
    assert!(access.contains("4 actions this session"), "{access}");
    // Why it was refused, as a sentence (the dump quotes it).
    assert!(
        access.contains("\"Agent \\\"helper\\\" may not act"),
        "{access}"
    );
    // Stopped: said, and the button allows them again.
    s.journal = Some(journal(true));
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    assert!(
        access.contains("Stopped: their requests are refused"),
        "{access}"
    );
    let at = button(&column, "Allow agents again")?;
    assert_eq!(click(&mut column, at), vec![Action::AllowAgents]);
    // In French too.
    let mut column = Column::new(Theme::default(), Lang::Fr, 900.0, 1.0);
    column.set_state(&s);
    column.draw(Duration::ZERO);
    button(&column, "Autoriser à nouveau les agents")?;
    Ok(())
}

#[test]
fn an_agent_connected_shows_its_window_and_the_user_takes_over_and_hands_back() -> anyhow::Result<()>
{
    let mut column = Column::new(Theme::default(), Lang::En, 900.0, 1.0);
    let mut s = state();
    let seat = |taken| hocus_shell::journal::SeatLine {
        agent: "helper".into(),
        window: Some("Draft".into()),
        taken,
    };
    s.journal = Some(JournalView {
        seats: vec![seat(false)],
        ..journal(false)
    });
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    assert!(access.contains("helper · in Draft"), "{access}");
    // The slot the compositor draws the window in, live.
    let slot = rect_of(&column, "Window of helper")?;
    let slots = column.seat_slots();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].0, "helper");
    assert_eq!(slots[0].1, slot);
    let at = button(&column, "Take over")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TakeOver("helper".into())]
    );
    s.journal = Some(JournalView {
        seats: vec![seat(true)],
        ..journal(false)
    });
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    assert!(access.contains("helper · you took over"), "{access}");
    let at = button(&column, "Hand back")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::HandBack("helper".into())]
    );
    // In French too.
    let mut column = Column::new(Theme::default(), Lang::Fr, 900.0, 1.0);
    column.set_state(&s);
    column.draw(Duration::ZERO);
    button(&column, "Rendre la main")?;
    rect_of(&column, "Fenêtre de helper")?;
    Ok(())
}

/// The rectangle of the element labelled `label`.
fn rect_of(column: &Column, label: &str) -> anyhow::Result<hocus_ui::Rect> {
    let access = column.access();
    access
        .walk()
        .into_iter()
        .find(|n| n.label.as_deref() == Some(label))
        .map(|n| n.rect)
        .ok_or_else(|| anyhow::anyhow!("no {label:?} in\n{}", access.dump()))
}

#[test]
fn on_a_laptop_screen_the_account_stays_in_the_column_and_the_lists_give_room_back()
-> anyhow::Result<()> {
    // 1280×800, less the margins.
    let height = 768.0;
    let mut column = Column::new(Theme::default(), Lang::En, height, 1.0);
    let mut s = state();
    s.journal = Some(journal(true));
    s.notifications = (1..=5)
        .map(|i| line(i, "Mail", &format!("Message {i}")))
        .collect();
    s.meeting = Some(Meeting {
        title: "Design review".into(),
        when: "In 12 min".into(),
        progress: 80,
        join: true,
    });
    s.calendar_set = true;
    s.phone = Some(PhoneLine {
        name: "Pixel 8".into(),
        battery: Some(82),
        charging: false,
        connect: true,
        adb: None,
    });
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let account = rect_of(&column, "Account: Camille")?;
    assert!(
        account.y >= 0.0 && account.bottom() <= height - 16.0 + 0.5,
        "the account at {account:?} leaves the column, {height} tall"
    );
    // The notifications gave room first, then the journal, down to its
    // latest entry, then the notifications' section altogether.
    let access = column.access().dump();
    assert!(!access.contains("Mail · Message 2"), "{access}");
    assert!(
        access.contains("helper · Click refused · Passwords"),
        "{access}"
    );
    assert!(!access.contains("helper · Typed 4"), "{access}");
    // Without the phone, a notification has room again.
    s.phone = None;
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let account = rect_of(&column, "Account: Camille")?;
    assert!(account.bottom() <= height - 16.0 + 0.5, "{account:?}");
    let access = column.access().dump();
    assert!(access.contains("Mail · Message 1"), "{access}");
    // With room again, everything shows again.
    let mut tall = Column::new(Theme::default(), Lang::En, 1400.0, 1.0);
    tall.set_state(&s);
    tall.draw(Duration::ZERO);
    let access = tall.access().dump();
    assert!(access.contains("Mail · Message 3"), "{access}");
    assert!(access.contains("helper · Pressed Ctrl+S"), "{access}");
    column.resize(1400.0, 1.0);
    column.draw(Duration::ZERO);
    assert_eq!(column.access().dump(), access);
    Ok(())
}

/// One agent waiting and one at work on the desktop shown, and three
/// project desktops with a herdr each (#192).
fn agents_by_desktop() -> AgentsView {
    let mut board = Board::default();
    let agent = |key: &str, name: &str, state| (key.to_owned(), name.to_owned(), state, None);
    board.replace(
        0,
        vec![
            agent("0/1", "refacto", AgentState::Work),
            agent("0/2", "tests", AgentState::Work),
        ],
        Duration::ZERO,
    );
    board.replace(
        1,
        vec![agent("1/1", "deploy", AgentState::Work)],
        Duration::ZERO,
    );
    board.replace(
        2,
        vec![
            agent("2/1", "blog", AgentState::Done),
            agent("2/2", "seo", AgentState::Rest),
        ],
        Duration::ZERO,
    );
    board.replace(
        3,
        vec![agent("3/1", "manual", AgentState::Work)],
        Duration::ZERO,
    );
    let now = Duration::from_secs(60);
    board.set(
        "0/1",
        AgentState::Wait,
        Some("Push the branch?".into()),
        now,
    );
    board.set("1/1", AgentState::Wait, None, now);
    let elsewhere = |source: usize, name: &str| Elsewhere {
        name: name.into(),
        output: "DP-1".into(),
        desktop: source + 1,
        sources: vec![source],
    };
    board
        .view(
            now,
            true,
            false,
            &[
                elsewhere(1, "Chapitô"),
                elsewhere(2, "Website"),
                elsewhere(3, "Docs"),
            ],
        )
        .expect("agents")
}

#[test]
fn other_desktops_agents_show_in_a_line_each_and_give_room_back_first() -> anyhow::Result<()> {
    let mut s = state();
    s.agents = Some(agents_by_desktop());
    s.screens[0].names = ["Me", "Chapitô", "Website", "Docs"]
        .map(String::from)
        .to_vec();
    s.screens[0].badges = vec![Badge::None, Badge::Waits, Badge::Done, Badge::None];
    // Room enough: every desktop in a line, and a click goes there.
    let mut tall = Column::new(Theme::default(), Lang::En, 1400.0, 1.0);
    tall.set_state(&s);
    tall.draw(Duration::from_secs(600));
    let access = tall.access().dump();
    for line in [
        "Chapitô · 1 waiting",
        "Website · 1 done",
        "Docs · 1 at work",
        "refacto · Waits for an answer: “Push the branch?”",
        "Desktop 1 of 4 on this screen · Chapitô: an agent waits for an answer · Website: an agent is done",
    ] {
        assert!(access.contains(line), "{line:?} in {access}");
    }
    assert!(access.contains("2 wait for an answer"), "{access}");
    let at = button(&tall, "Website · 1 done")?;
    assert_eq!(
        click(&mut tall, at),
        vec![Action::ShowDesktop {
            output: "DP-1".into(),
            desktop: 3
        }]
    );
    // Less room, with notifications: the lines share one before the
    // agents shown in full give any.
    let height = 900.0;
    let mut column = Column::new(Theme::default(), Lang::En, height, 1.0);
    s.notifications = (1..=5)
        .map(|i| line(i, "Mail", &format!("Message {i}")))
        .collect();
    column.set_state(&s);
    column.draw(Duration::from_secs(600));
    common::expect_reference(&column.view(), "column-agents-by-desktop.png")?;
    let account = rect_of(&column, "Account: Camille")?;
    assert!(
        account.bottom() <= height - 16.0 + 0.5,
        "the account at {account:?} leaves the column, {height} tall"
    );
    // The lines went before anything else, notifications but one, while
    // both agents of this desktop show in full; the summary and the dots
    // still tell.
    let access = column.access().dump();
    assert!(!access.contains("Docs · 1 at work"), "{access}");
    assert!(!access.contains("3 other desktops"), "{access}");
    assert!(!access.contains("Mail · Message 2"), "{access}");
    assert!(access.contains("Show refacto"), "{access}");
    assert!(access.contains("Yes to refacto"), "{access}");
    assert!(access.contains("tests · Works · 1 min"), "{access}");
    assert!(access.contains("2 wait for an answer"), "{access}");
    // A little more room: they share a line.
    column.resize(990.0, 1.0);
    column.draw(Duration::from_secs(600));
    let access = column.access().dump();
    assert!(access.contains("\"3 other desktops\""), "{access}");
    assert!(
        access.contains("\"1 waiting · 1 at work · 1 done\""),
        "{access}"
    );
    Ok(())
}

#[test]
fn on_a_laptop_screen_a_waiting_agent_and_the_meeting_fit_with_the_account() -> anyhow::Result<()> {
    // 1280×800, less the margins.
    let height = 768.0;
    let mut column = Column::new(Theme::default(), Lang::En, height, 1.0);
    let mut s = state();
    s.agents = Some(agents(true));
    s.meeting = Some(Meeting {
        title: "Design review".into(),
        when: "In 12 min".into(),
        progress: 80,
        join: true,
    });
    s.calendar_set = true;
    column.set_state(&s);
    column.draw(Duration::from_secs(600));
    common::expect_reference(&column.view(), "column-laptop-agents.png")?;
    let account = rect_of(&column, "Account: Camille")?;
    let size = column.size();
    assert!(
        account.x >= 0.0
            && account.y >= 0.0
            && account.right() <= size.w
            && account.bottom() <= size.h - 16.0 + 0.5,
        "the account at {account:?} leaves the column, {size:?}"
    );
    // The meeting on one line, the waiting agent in short: still joined,
    // still answered.
    let at = button(&column, "Join")?;
    assert_eq!(click(&mut column, at), vec![Action::JoinMeeting]);
    for (name, ask) in [
        ("Yes to refacto", AgentAsk::Yes),
        ("No to refacto", AgentAsk::No),
    ] {
        let at = button(&column, name)?;
        assert_eq!(
            click(&mut column, at),
            vec![Action::Agent {
                key: "w1:p1".into(),
                ask
            }]
        );
    }
    let at = button(&column, "refacto · Waits for an answer: “Push the branch?”")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::Agent {
            key: "w1:p1".into(),
            ask: AgentAsk::Show
        }]
    );
    // With room, the card and the row as the design draws them.
    column.resize(1000.0, 1.0);
    column.draw(Duration::from_secs(600));
    let access = column.access().dump();
    assert!(access.contains("Show refacto"), "{access}");
    assert!(access.contains("\"In 12 min\""), "{access}");
    Ok(())
}

/// Board Bureau's tasks, one late.
fn tasks() -> TasksView {
    let row = |id: &str, text: &str, due, late, done| TaskRow {
        id: id.into(),
        text: text.into(),
        due,
        late,
        done,
        list: None,
    };
    TasksView {
        rows: vec![
            row(
                "quote",
                "Envoyer le devis au client",
                Some(Due::Yesterday),
                true,
                false,
            ),
            row("review", "Relire la MR #42", Some(Due::Today), false, false),
            row("domain", "Réserver hocuspanda.org", None, false, false),
            row("brief", "Écrire le brief design", None, false, true),
        ],
        open: 3,
        late: 1,
        done_today: 1,
        pandas: true,
        offline: false,
    }
}

#[test]
fn the_tasks_show_late_ones_first_tick_and_open_their_menu() -> anyhow::Result<()> {
    let mut column = Column::new(Theme::default(), Lang::Fr, 900.0, 1.0);
    let mut s = state();
    s.tasks = Some(tasks().into());
    column.set_state(&s);
    column.draw(Duration::ZERO);
    common::expect_reference(&column.view(), "column-tasks-fr.png")?;
    let access = column.access().dump();
    for said in ["TÂCHES", "1 en retard", "hier", "auj."] {
        assert!(access.contains(said), "{said} in {access}");
    }
    let at = button(&column, "Relire la MR #42")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::ToggleTask("review".into())]
    );
    let at = button(&column, "Ajouter une tâche")?;
    assert_eq!(click(&mut column, at), vec![Action::AddTask]);
    // Its menu, on the secondary button.
    let at = button(&column, "Réserver hocuspanda.org")?;
    assert!(column.secondary_click(at).is_empty());
    column.draw(Duration::from_millis(16));
    let menu = column.access().dump();
    for entry in ["Modifier", "Pour aujourd’hui", "Pour demain", "Supprimer"] {
        assert!(menu.contains(entry), "{entry} in {menu}");
    }
    assert!(!menu.contains("Sans date"), "it has none: {menu}");
    let at = button(&column, "Pour demain")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TaskDue {
            id: "domain".into(),
            due: DueChoice::Tomorrow
        }]
    );
    // Nothing left: all done, the panda says so.
    s.tasks = Some(
        TasksView {
            rows: Vec::new(),
            open: 0,
            late: 0,
            done_today: 4,
            pandas: true,
            offline: false,
        }
        .into(),
    );
    column.set_state(&s);
    column.draw(Duration::from_millis(32));
    let access = column.access().dump();
    for said in [
        "Tout est fait pour aujourd’hui.",
        "4 sur 4",
        "Panda qui a fini",
    ] {
        assert!(access.contains(said), "{said} in {access}");
    }
    Ok(())
}

#[test]
fn a_task_of_a_caldav_list_says_its_list_and_offline_says_so() -> anyhow::Result<()> {
    let mut view = tasks();
    view.rows[1].list = Some(ListMark {
        name: "Courses de la semaine".into(),
        color: Some(hocus_ui::Color::rgb(0x2E, 0x7D, 0x4F)),
    });
    view.offline = true;
    let mut column = Column::new(Theme::default(), Lang::Fr, 900.0, 1.0);
    let mut s = state();
    s.tasks = Some(view.into());
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    for said in [
        "1 en retard · hors ligne",
        "Courses de…",
        "Relire la MR #42",
    ] {
        assert!(access.contains(said), "{said} in {access}");
    }
    // The row still ticks the task.
    let at = button(&column, "Relire la MR #42")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::ToggleTask("review".into())]
    );
    Ok(())
}

#[test]
fn short_of_room_all_done_is_said_beside_the_caption_with_a_plus() -> anyhow::Result<()> {
    let empty = TasksView {
        pandas: true,
        ..TasksView::default()
    };
    // Tall: the card with its panda, and the field.
    let mut column = Column::new(Theme::default(), Lang::En, 1000.0, 1.0);
    let mut s = state();
    s.tasks = Some(empty.clone().into());
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    assert!(access.contains("Panda done"), "{access}");
    // A laptop's: neither; the + adds one all the same.
    let mut column = Column::new(Theme::default(), Lang::En, 600.0, 1.0);
    column.set_state(&s);
    column.draw(Duration::ZERO);
    let access = column.access().dump();
    assert!(!access.contains("Panda done"), "{access}");
    assert!(access.contains("All done for today."), "{access}");
    let at = button(&column, "Add a task")?;
    assert_eq!(click(&mut column, at), vec![Action::AddTask]);
    rect_of(&column, "Account: Camille")?;
    Ok(())
}

#[test]
fn the_window_buttons_ask_for_their_actions() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    for (name, action) in [
        ("Minimize", Action::Minimize),
        ("Maximize", Action::ToggleMaximize),
        ("Close", Action::Close),
    ] {
        let at = button(&column, name)?;
        assert_eq!(click(&mut column, at), vec![action]);
    }
    Ok(())
}

#[test]
fn the_notifications_are_cleared_all_at_once_or_one_by_one() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    // A row's dismiss button shows while the pointer is over the row.
    let row = rect_of(&column, "Mail · Revue de la MR #42")?;
    let over = Point::new(row.x + 20.0, row.y + row.h / 2.0);
    column.input(over, Input::PointerMove(over));
    column.draw(Duration::from_secs(1));
    common::expect_reference(&column.view(), "column-notification-hovered.png")?;
    let at = button(&column, "Dismiss: Chat · Les maquettes du dock sont prêtes")?;
    assert_eq!(click(&mut column, at), vec![Action::DismissNotification(2)]);
    let at = button(&column, "Clear all")?;
    assert_eq!(click(&mut column, at), vec![Action::ClearNotifications]);
    // None left: neither Clear all nor rows, but that there is none.
    let mut s = state();
    s.notifications.clear();
    s.notification_count = 0;
    column.set_state(&s);
    column.draw(Duration::from_secs(2));
    assert!(button(&column, "Clear all").is_err());
    rect_of(&column, "Nothing new")?;
    // In French.
    let column = self::column(Lang::Fr);
    button(&column, "Tout effacer")?;
    button(&column, "Ignorer\u{a0}: Mail · Revue de la MR #42")?;
    Ok(())
}

#[test]
fn the_desktops_line_follows_the_screen_and_opens_its_popover() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    let mut s = state();
    s.screens[0].shown = 3;
    column.set_state(&s);
    column.draw(Duration::from_millis(16));
    let line = "Desktop 3 of 4 on this screen";
    let at = button(&column, line)?;
    assert_eq!(click(&mut column, at), vec![]);
    column.draw(Duration::from_millis(32));
    let popover = column
        .popover_view()
        .ok_or_else(|| anyhow::anyhow!("no popover"))?;
    common::expect_reference(&popover, "column-desktops-popover.png")?;
    let at = button(&column, "eDP-1 · Desktop 1")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::ShowDesktop {
            output: "eDP-1".into(),
            desktop: 1
        }]
    );
    assert!(column.popover_view().is_none(), "an action closes it");
    Ok(())
}

/// October 2026 begins on a Thursday, in week 40.
fn agenda() -> Agenda {
    Agenda {
        year: 2026,
        first_weekday: 3,
        days: 31,
        weeks: vec![40, 41, 42, 43, 44],
        busy: vec![7, 14, 21, 23, 28],
        today: vec![
            ("14:49".into(), "Weekly review".into()),
            ("16:00".into(), "Design review".into()),
        ],
    }
}

#[test]
fn the_meeting_card_opens_the_agenda_then_joins() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    let mut s = state();
    s.calendar_set = true;
    s.agenda = agenda();
    s.meeting = Some(Meeting {
        title: "Weekly review".into(),
        when: "In 12 min".into(),
        progress: 80,
        join: false,
    });
    column.set_state(&s);
    column.draw(Duration::from_millis(16));
    let at = button(&column, "Details")?;
    assert_eq!(click(&mut column, at), vec![]);
    column.draw(Duration::from_millis(32));
    let popover = column
        .popover_view()
        .ok_or_else(|| anyhow::anyhow!("no agenda"))?;
    common::expect_reference(&popover, "column-agenda.png")?;
    // Its button again: closed, not opened anew.
    let at = button(&column, "Details")?;
    assert_eq!(click(&mut column, at), vec![]);
    assert!(
        column.popover_view().is_none(),
        "the second click closes it"
    );
    // The clock opens it too.
    let at = button(&column, "Agenda")?;
    assert_eq!(click(&mut column, at), vec![]);
    assert!(
        column.popover_view().is_some(),
        "the clock opens the agenda"
    );
    assert_eq!(click(&mut column, at), vec![]);
    // Ten minutes before, Join.
    if let Some(m) = &mut s.meeting {
        m.when = "In 10 min".into();
        m.join = true;
    }
    column.set_state(&s);
    column.draw(Duration::from_millis(48));
    let at = button(&column, "Join")?;
    assert_eq!(click(&mut column, at), vec![Action::JoinMeeting]);
    Ok(())
}

#[test]
fn the_account_menu_logs_out() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    let at = button(&column, "Account: Camille")?;
    click(&mut column, at);
    column.draw(Duration::from_millis(16));
    let popover = column
        .popover_view()
        .ok_or_else(|| anyhow::anyhow!("no menu"))?;
    common::expect_reference(&popover, "column-account-menu.png")?;
    let dump = column.access().dump();
    assert!(dump.contains("button \"Lock\""), "{dump}");
    assert!(dump.contains("button \"Suspend\""), "{dump}");
    // Not offered where the machine does not hibernate.
    assert!(!dump.contains("Hibernate"), "{dump}");
    let at = button(&column, "Log out")?;
    assert_eq!(click(&mut column, at), vec![Action::LogOut]);
    Ok(())
}

/// Board D13's keyboard menu: AZERTY, US and BÉPO kept.
fn three_layouts() -> ColumnState {
    let layout = |name: &str, short: &str| KeyboardLayout {
        name: name.into(),
        short: short.into(),
    };
    ColumnState {
        layouts: vec![
            layout("Français (AZERTY)", "FR"),
            layout("Anglais (US)", "US"),
            layout("Français (BÉPO)", "BÉPO"),
        ]
        .into(),
        layout: 0,
        switch_layout_keys: Some("Super+Ctrl+Espace".into()),
        ..state()
    }
}

#[test]
fn the_keyboard_menu_shows_the_layouts_kept_and_picks_one() -> anyhow::Result<()> {
    let mut column = column(Lang::Fr);
    column.set_state(&three_layouts());
    column.draw(Duration::ZERO);
    let at = button(&column, "Disposition du clavier\u{a0}: Français (AZERTY)")?;
    // The chip opens the menu; it no longer switches by itself.
    assert!(click(&mut column, at).is_empty());
    column.draw(Duration::from_millis(16));
    let popover = column
        .popover_view()
        .ok_or_else(|| anyhow::anyhow!("no menu"))?;
    common::expect_reference(&popover, "column-keyboard-menu.png")?;
    let access = column.access();
    let chosen: Vec<_> = access
        .walk()
        .into_iter()
        .filter(|n| n.role == Role::Button && n.selected)
        .filter_map(|n| n.label.clone())
        .collect();
    assert!(
        chosen.contains(&"Français (AZERTY)".to_owned()),
        "{chosen:?}"
    );
    assert!(
        access.dump().contains("Super+Ctrl+Espace"),
        "{}",
        access.dump()
    );
    let at = button(&column, "Français (BÉPO)")?;
    assert_eq!(click(&mut column, at), vec![Action::SetLayout(2)]);
    assert!(column.popover_view().is_none(), "closed once chosen");
    // The chip says the one in use.
    column.set_state(&ColumnState {
        layout: 2,
        ..three_layouts()
    });
    column.draw(Duration::from_millis(32));
    assert!(button(&column, "Disposition du clavier\u{a0}: Français (BÉPO)").is_ok());
    Ok(())
}

#[test]
fn the_tray_shows_its_applications_and_their_menus() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    assert!(
        button(&column, "Tray: 1 application").is_err(),
        "no chip when empty"
    );
    let app = |key: &str, title: &str, attention| TrayApp {
        key: key.into(),
        title: title.into(),
        attention,
        icon: Some((1, 1, vec![0x80, 0, 0, 0x80].into())),
    };
    column.set_state(&ColumnState {
        tray: vec![app(":1.4/A", "Chat", true), app(":1.5/B", "Sync", false)],
        ..state()
    });
    column.draw(Duration::ZERO);
    let at = button(&column, "Tray: 2 applications, 1 waiting")?;
    click(&mut column, at);
    column.draw(Duration::ZERO);
    let at = button(&column, "Chat")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TrayActivate(":1.4/A".into())]
    );
    // A right click on a line, or its menu button, asks for the menu.
    let at = button(&column, "Tray: 2 applications, 1 waiting")?;
    click(&mut column, at);
    column.draw(Duration::ZERO);
    let at = button(&column, "Sync")?;
    assert_eq!(
        column.secondary_click(at),
        vec![Action::TrayMenu(":1.5/B".into())]
    );
    let at = button(&column, "Chat’s menu")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TrayMenu(":1.4/A".into())]
    );
    let entry = |id, label: &str| TrayMenuEntry {
        id,
        label: label.into(),
        enabled: true,
        separator: false,
        toggle: None,
        children: Vec::new(),
    };
    column.show_tray_menu(":1.4/A".into(), vec![entry(1, "Open"), entry(2, "Quit")]);
    column.draw(Duration::ZERO);
    let at = button(&column, "Quit")?;
    assert_eq!(
        click(&mut column, at),
        vec![Action::TrayChosen {
            key: ":1.4/A".into(),
            id: 2
        }]
    );
    // The chip goes with the last application.
    column.set_state(&state());
    column.draw(Duration::ZERO);
    assert!(button(&column, "Tray: 2 applications").is_err());
    assert!(button(&column, "Keyboard layout: French (AZERTY)").is_ok());
    Ok(())
}

#[test]
fn while_sharing_the_column_says_so_and_stops_it() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    assert!(
        button(&column, "Stop sharing").is_err(),
        "hidden when not sharing"
    );
    column.set_state(&ColumnState {
        sharing: true,
        ..state()
    });
    column.draw(Duration::ZERO);
    let at = button(&column, "Stop sharing")?;
    assert_eq!(click(&mut column, at), vec![Action::StopSharing]);
    Ok(())
}

#[test]
fn without_a_window_the_controls_are_disabled() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    column.set_state(&ColumnState {
        active: None,
        ..state()
    });
    column.draw(Duration::from_millis(16));
    let at = button(&column, "Close")?;
    assert_eq!(click(&mut column, at), vec![]);
    assert!(column.access().dump().contains("No window"));
    Ok(())
}

#[test]
fn the_phone_rings_and_its_popover_sends_and_browses() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    assert!(button(&column, "Ring").is_err(), "no phone, no line");
    column.set_state(&ColumnState {
        phone: Some(PhoneLine {
            name: "Pixel 8".into(),
            battery: Some(82),
            charging: false,
            connect: true,
            adb: None,
        }),
        ..state()
    });
    column.draw(Duration::ZERO);
    assert!(column.access().dump().contains("Phone · 82%"));
    let at = button(&column, "Ring")?;
    assert_eq!(click(&mut column, at), vec![Action::RingPhone]);
    let at = button(&column, "Phone: Pixel 8, 82%")?;
    assert_eq!(click(&mut column, at), vec![Action::PhoneOpened]);
    column.draw(Duration::ZERO);
    assert!(column.access().dump().contains("Battery 82%"));
    let at = button(&column, "Send the clipboard")?;
    assert_eq!(click(&mut column, at), vec![Action::SendClipboardToPhone]);
    let at = button(&column, "Phone: Pixel 8, 82%")?;
    click(&mut column, at);
    column.draw(Duration::ZERO);
    let at = button(&column, "Browse its files")?;
    assert_eq!(click(&mut column, at), vec![Action::BrowsePhone]);
    // Out of reach, its line and popover go.
    let at = button(&column, "Phone: Pixel 8, 82%")?;
    click(&mut column, at);
    column.set_state(&state());
    column.draw(Duration::ZERO);
    assert!(button(&column, "Browse its files").is_err());
    assert!(button(&column, "Ring").is_err());
    Ok(())
}

#[test]
fn adb_alone_pairs_by_a_qr_code_and_mirrors() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    let adb = AdbLine {
        device: None,
        mirror: true,
        pairing: None,
    };
    let phone = PhoneLine {
        connect: false,
        adb: Some(adb.clone()),
        ..PhoneLine::default()
    };
    column.set_state(&ColumnState {
        phone: Some(phone.clone()),
        ..state()
    });
    column.draw(Duration::ZERO);
    // No KDE Connect: no Ring, a line to pair from.
    assert!(button(&column, "Ring").is_err());
    let at = button(&column, "Phone")?;
    assert_eq!(click(&mut column, at), vec![Action::PhoneOpened]);
    column.draw(Duration::ZERO);
    assert!(column.access().dump().contains("No phone paired yet"));
    assert!(
        button(&column, "Mirror its screen").is_err(),
        "nothing to mirror"
    );
    assert!(column.phone_popover_open());
    let at = button(&column, "Pair over Wi-Fi")?;
    assert_eq!(click(&mut column, at), vec![Action::PairPhone]);
    assert!(column.phone_popover_open(), "pairing shows in it");
    // The popover stays, and shows the code: drawn, never named.
    let qr = "WIFI:T:ADB;S:hocuspanda-test;P:secret123;;".to_owned();
    let pairing = |stage| PhoneLine {
        adb: Some(AdbLine {
            pairing: Some(Pairing {
                qr: qr.clone(),
                stage,
            }),
            ..adb.clone()
        }),
        ..phone.clone()
    };
    column.set_state(&ColumnState {
        phone: Some(pairing(PairStage::Waiting)),
        ..state()
    });
    column.draw(Duration::ZERO);
    let dump = column.access().dump();
    assert!(dump.contains("QR code to scan with the phone"), "{dump}");
    assert!(dump.contains("Waiting for the phone"), "{dump}");
    assert!(!dump.contains("secret123"), "{dump}");
    assert!(!format!("{:?}", pairing(PairStage::Waiting)).contains("secret123"));
    column.set_state(&ColumnState {
        phone: Some(pairing(PairStage::Failed("Could not pair".into()))),
        ..state()
    });
    column.draw(Duration::ZERO);
    let at = button(&column, "Try again")?;
    assert_eq!(click(&mut column, at), vec![Action::PairPhone]);
    let at = button(&column, "Cancel")?;
    assert_eq!(click(&mut column, at), vec![Action::CancelPairing]);
    // Paired: the device, and its screen to mirror.
    column.set_state(&ColumnState {
        phone: Some(PhoneLine {
            name: "Pixel 8".into(),
            adb: Some(AdbLine {
                device: Some(("Pixel 8".into(), true)),
                ..adb.clone()
            }),
            ..phone.clone()
        }),
        ..state()
    });
    column.draw(Duration::ZERO);
    assert!(
        column
            .access()
            .dump()
            .contains("Paired over Wi-Fi: Pixel 8")
    );
    let at = button(&column, "Mirror its screen")?;
    assert_eq!(click(&mut column, at), vec![Action::MirrorPhone]);
    Ok(())
}

#[test]
fn the_phone_popover_says_when_it_closes() -> anyhow::Result<()> {
    let mut column = column(Lang::En);
    column.set_state(&ColumnState {
        phone: Some(PhoneLine {
            adb: Some(AdbLine::default()),
            ..PhoneLine::default()
        }),
        ..state()
    });
    column.draw(Duration::ZERO);
    assert!(!column.phone_popover_open());
    let at = button(&column, "Phone")?;
    click(&mut column, at);
    assert!(column.phone_popover_open());
    column.clicked_elsewhere();
    assert!(!column.phone_popover_open());
    Ok(())
}
