//! Acceptance scenarios of applications' accessibility trees for agents
//! (#175): a test application exposes its tree on a private accessibility
//! bus (never the session's), its window drawing the elements where the
//! tree says, and an MCP agent reads it and clicks its elements by label.

use std::{
    process::{Child, Command},
    thread::JoinHandle,
    time::Duration,
};

use hocus_api::{Permission, Reply, Request, mcp::Bridge};
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, RendererChoice};
use hocus_test::{
    atspi::{FakeApp, FakeElement, FakeRegistry},
    bus::PrivateBus,
    client::{ClientEvent, ClientOptions, SolidWindow, TestClient},
};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(20);

const AGENTS: &str = r#"
[agents]
show_before_click_ms = 0

[[agents.mcp]]
name = "helper"
access = "act"
apps = ["test.mail", "test.list", "plain"]

[[agents.mcp]]
name = "reader"
apps = ["test.mail"]

[[agents.mcp]]
name = "outsider"
access = "act"
apps = ["other"]
"#;

/// A compositor as the session starts it, reading trees on a private
/// accessibility bus, with the agents configured.
struct Desk {
    compositor: Headless,
    _bus: PrivateBus,
    _registry: FakeRegistry,
    address: String,
}

fn desk() -> anyhow::Result<Desk> {
    let mut compositor = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            renderer: RendererChoice::Pixman,
        },
        Permission::Observe,
    )?;
    if let Some(err) = compositor.write_config(AGENTS)? {
        anyhow::bail!("the configuration was refused: {err}");
    }
    let bus = PrivateBus::start()?;
    let registry = FakeRegistry::start(&bus.address)?;
    compositor.set_a11y_bus(&bus.address);
    let address = bus.address.clone();
    Ok(Desk {
        compositor,
        _bus: bus,
        _registry: registry,
        address,
    })
}

/// A window of `app_id` titled `title`, its application exposing `tree`
/// (its window drawing the elements), and its id.
fn open(
    desk: &mut Desk,
    app_id: &str,
    title: &str,
    tree: FakeElement,
) -> anyhow::Result<(TestClient, FakeApp, u64)> {
    let elements = tree.drawn();
    let app = FakeApp::start(&desk.address, app_id, vec![tree])?;
    let before = desk.compositor.window_count();
    let client = SolidWindow {
        app_id: app_id.into(),
        width: 400,
        height: 300,
        argb: 0xff_1d_20_25,
    }
    .spawn_with(
        desk.compositor.socket_path(),
        ClientOptions {
            title: Some(title.into()),
            elements,
            ..ClientOptions::default()
        },
    )?;
    desk.compositor
        .dispatch_until(TIMEOUT, |c| c.window_count() > before)?;
    let id = window_of(&mut desk.compositor, app_id)?;
    Ok((client, app, id))
}

fn window_of(compositor: &mut Headless, app_id: &str) -> anyhow::Result<u64> {
    let Reply::Windows(windows) = compositor.request(Request::Windows)? else {
        anyhow::bail!("expected windows");
    };
    windows
        .iter()
        .find(|w| w.app_id.as_deref() == Some(app_id))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window of {app_id}"))
}

/// Runs a whole session, line by line as a client writes it on the
/// server's stdin, and returns the answers it read on its stdout.
fn session(
    compositor: &mut Headless,
    agent: &str,
    calls: &[(&str, Value)],
) -> anyhow::Result<Vec<Value>> {
    let socket = compositor.api_socket().to_path_buf();
    let agent = agent.to_owned();
    let mut messages = vec![
        json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
            },
        }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ];
    for (n, (tool, arguments)) in calls.iter().enumerate() {
        messages.push(json!({
            "jsonrpc": "2.0", "id": n + 1, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments },
        }));
    }
    let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
    let thread: JoinHandle<std::io::Result<Vec<u8>>> = std::thread::spawn(move || {
        let mut output = Vec::new();
        Bridge::new(socket, Some(agent)).serve(input.as_bytes(), &mut output)?;
        Ok(output)
    });
    let deadline = std::time::Instant::now() + TIMEOUT;
    while !thread.is_finished() {
        anyhow::ensure!(std::time::Instant::now() < deadline, "not done in time");
        match compositor.agent_deadline().0 {
            Some(due) if due > compositor.now() => {
                let by = due - compositor.now();
                compositor.advance(by)?;
            }
            _ => compositor.dispatch(Duration::from_millis(5))?,
        }
    }
    let output = thread
        .join()
        .map_err(|_| anyhow::anyhow!("the MCP server panicked"))??;
    let answers: Vec<Value> = String::from_utf8(output)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    // Answers to the calls, in order.
    (1..=calls.len())
        .map(|n| {
            answers
                .iter()
                .find(|a| a["id"] == n)
                .map(|a| a["result"].clone())
                .ok_or_else(|| anyhow::anyhow!("no answer {n} among {answers:?}"))
        })
        .collect()
}

fn said(result: &Value) -> &str {
    result["content"][0]["text"].as_str().unwrap_or_default()
}

fn element<'a>(elements: &'a [Value], role: &str, label: &str) -> anyhow::Result<&'a Value> {
    elements
        .iter()
        .find(|e| e["role"] == role && e["label"] == label)
        .ok_or_else(|| anyhow::anyhow!("no {role} {label:?} in {elements:?}"))
}

/// What the agent's seat pressed on the client's elements.
fn hits(client: &TestClient) -> Vec<String> {
    client
        .events()
        .iter()
        .filter_map(|e| match e {
            ClientEvent::OnSeat(seat, e) if seat == "agent-helper" => match &**e {
                ClientEvent::Hit(label) => Some(label.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn given_a_test_client_exposing_an_atspi_tree_when_an_mcp_client_asks_for_its_windows_tree_then_it_gets_its_roles_labels_and_places()
-> anyhow::Result<()> {
    let mut desk = desk()?;
    let (_client, app, id) = open(
        &mut desk,
        "test.mail",
        "Compose",
        FakeElement::compose("Compose"),
    )?;

    let answers = session(
        &mut desk.compositor,
        "helper",
        &[("accessibility_tree", json!({ "window": id }))],
    )?;

    let result = &answers[0];
    assert_eq!(result["isError"], false, "{result}");
    let tree = &result["structuredContent"];
    assert_eq!(tree["source"], "application");
    assert_eq!(tree["application"], "test.mail");
    assert_eq!(tree["toolkit"], "hocus-test");
    assert_eq!(tree["window"]["id"], id);
    assert!(tree.get("partial").is_none(), "{tree}");
    let elements = tree["elements"].as_array().cloned().unwrap_or_default();
    let frame = element(&elements, "frame", "Compose")?;
    assert_eq!(frame["depth"], 0);
    let send = element(&elements, "push button", "Send")?;
    assert_eq!(
        send["rect"],
        json!({ "x": 284, "y": 244, "width": 100, "height": 40 })
    );
    assert_eq!(send["depth"], 2);
    assert_eq!(send["disabled"], false);
    assert_eq!(
        element(&elements, "push button", "Delete")?["disabled"],
        true
    );
    let subject = element(&elements, "entry", "Subject")?;
    assert_eq!(subject["value"], "Lunch on Friday");
    // The password field: its role and label, never what it holds, which
    // is not even read.
    let password = element(&elements, "password text", "Password")?;
    assert!(password.get("value").is_none(), "{password}");
    assert!(!result.to_string().contains("hunter2"));
    assert_eq!(app.read(), ["Subject"]);
    // A page not shown is left out, with what it holds.
    assert!(
        !elements
            .iter()
            .any(|e| e["label"] == "Hidden" || e["label"] == "Drafts")
    );
    assert!(
        said(result).contains("\"label\": \"Send\""),
        "{}",
        said(result)
    );
    Ok(())
}

#[test]
fn given_that_tree_when_the_agent_clicks_an_element_by_label_then_the_client_receives_the_click_on_it()
-> anyhow::Result<()> {
    let mut desk = desk()?;
    let (client, _app, id) = open(
        &mut desk,
        "test.mail",
        "Compose",
        FakeElement::compose("Compose"),
    )?;

    let answers = session(
        &mut desk.compositor,
        "helper",
        &[
            (
                "click",
                json!({ "window": id, "label": "Send", "intent": "Sends the message" }),
            ),
            // A part of a label, any case, when only one has it.
            ("click", json!({ "window": id, "label": "LATER" })),
            // Two share a part: refused, both listed.
            ("click", json!({ "window": id, "label": "send" })),
            // A label and an entry share "Subject": the role says which.
            ("click", json!({ "window": id, "label": "Subject" })),
            (
                "click",
                json!({ "window": id, "label": "Subject", "role": "entry" }),
            ),
            ("click", json!({ "window": id, "label": "Delete" })),
            ("click", json!({ "window": id, "label": "Archive" })),
            ("journal", json!({ "limit": 7 })),
        ],
    )?;

    assert_eq!(answers[0]["isError"], false, "{}", answers[0]);
    assert_eq!(answers[1]["isError"], false, "{}", answers[1]);
    assert_eq!(
        said(&answers[2]),
        "bad request: 2 elements match \"send\": push button \"Send later\" (at 16,244 140×40); \
         push button \"Send\" (at 284,244 100×40); say which with role, or the whole label"
    );
    assert!(
        said(&answers[3]).starts_with("bad request: 2 elements match \"Subject\": label"),
        "{}",
        said(&answers[3])
    );
    assert_eq!(answers[4]["isError"], false, "{}", answers[4]);
    assert_eq!(said(&answers[5]), "failed: \"Delete\" is disabled");
    assert!(
        said(&answers[6]).starts_with("not found: the window's tree has no element labelled"),
        "{}",
        said(&answers[6])
    );
    desk.compositor
        .dispatch_until(TIMEOUT, |_| hits(&client).len() >= 3)?;
    assert_eq!(hits(&client), ["Send", "Send later", "Subject"]);
    // The user's pointer saw nothing.
    assert!(!client.events().contains(&ClientEvent::Hit("Send".into())));
    let journal = said(&answers[7]);
    assert!(
        journal.contains(&format!(
            "helper: click on \"Send\" of window {id} test.mail \"Compose\""
        )),
        "{journal}"
    );
    assert!(
        journal.contains("click on \"Delete\" of window")
            && journal.contains("refused: \"Delete\" is disabled"),
        "{journal}"
    );
    Ok(())
}

#[test]
fn the_ring_surrounds_the_element_before_the_click_lands() -> anyhow::Result<()> {
    let mut desk = desk()?;
    let (_client, _app, id) = open(
        &mut desk,
        "test.mail",
        "Compose",
        FakeElement::compose("Compose"),
    )?;
    if let Some(err) = desk
        .compositor
        .write_config(&AGENTS.replace("show_before_click_ms = 0", "show_before_click_ms = 600"))?
    {
        anyhow::bail!("{err}");
    }
    let socket = desk.compositor.api_socket().to_path_buf();
    let thread = std::thread::spawn(move || {
        let mut bridge = Bridge::new(socket, Some("helper".into()));
        bridge.handle(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "click", "arguments": { "window": id, "label": "Send" } },
        }))
    });
    // Once the element is found, the click waits under its ring.
    desk.compositor
        .dispatch_until(TIMEOUT, |c| c.agent_pending().0 == 1)?;
    let Reply::Ui(nodes) = desk.compositor.request(Request::Ui)? else {
        anyhow::bail!("expected the interface");
    };
    assert!(
        nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("helper · Clicks “Send”")),
        "{nodes:?}"
    );
    while !thread.is_finished() {
        desk.compositor.advance(Duration::from_millis(100))?;
    }
    let answer = thread
        .join()
        .map_err(|_| anyhow::anyhow!("the MCP server panicked"))?;
    assert_eq!(
        answer.map(|a| a["result"]["isError"].clone()),
        Some(json!(false))
    );
    Ok(())
}

#[test]
fn a_window_without_an_accessibility_tree_says_so() -> anyhow::Result<()> {
    let mut desk = desk()?;
    // Its own process, which puts nothing on the accessibility bus.
    let socket = desk.compositor.socket_path().to_path_buf();
    let (Some(runtime), Some(display)) = (socket.parent(), socket.file_name()) else {
        anyhow::bail!("no socket");
    };
    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_hocus-test-client"))
        .args(["--app-id", "plain", "--title", "Plain"])
        .env("XDG_RUNTIME_DIR", runtime)
        .env("WAYLAND_DISPLAY", display)
        .spawn()?;
    let shown = desk
        .compositor
        .dispatch_until(TIMEOUT, |c| c.window_count() == 1);
    let answers = shown.and_then(|()| {
        let id = window_of(&mut desk.compositor, "plain")?;
        session(
            &mut desk.compositor,
            "helper",
            &[
                ("accessibility_tree", json!({ "window": id })),
                ("click", json!({ "window": id, "label": "OK" })),
            ],
        )
    });
    // By its own pid, whatever happened.
    let _ = child.kill();
    let _ = child.wait();
    let answers = answers?;

    let tree = &answers[0]["structuredContent"];
    assert_eq!(answers[0]["isError"], false, "{}", answers[0]);
    assert_eq!(tree["source"], "window");
    assert_eq!(tree["elements"], json!([]));
    let note = tree["note"].as_str().unwrap_or_default();
    assert!(
        note.starts_with(
            "This window has no accessibility tree: its application is not on the accessibility bus"
        ),
        "{note}"
    );
    assert!(
        said(&answers[1]).contains("has no accessibility tree"),
        "{}",
        said(&answers[1])
    );
    Ok(())
}

#[test]
fn without_an_accessibility_bus_a_tree_is_not_read_and_the_agent_is_told() -> anyhow::Result<()> {
    let mut compositor = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1280, 800)],
            renderer: RendererChoice::Pixman,
        },
        Permission::Observe,
    )?;
    if let Some(err) = compositor.write_config(AGENTS)? {
        anyhow::bail!("{err}");
    }
    let _client = SolidWindow {
        app_id: "test.mail".into(),
        width: 400,
        height: 300,
        argb: 0xff_1d_20_25,
    }
    .spawn(compositor.socket_path())?;
    compositor.dispatch_until(TIMEOUT, |c| c.window_count() == 1)?;
    let id = window_of(&mut compositor, "test.mail")?;
    let answers = session(
        &mut compositor,
        "helper",
        &[("accessibility_tree", json!({ "window": id }))],
    )?;
    let note = answers[0]["structuredContent"]["note"]
        .as_str()
        .unwrap_or_default();
    assert!(
        note.contains("the accessibility bus is not reachable"),
        "{note}"
    );
    Ok(())
}

#[test]
fn a_huge_tree_is_cut_and_marked_partial() -> anyhow::Result<()> {
    let mut desk = desk()?;
    let (_client, _app, id) = open(
        &mut desk,
        "test.list",
        "Inbox",
        FakeElement::long_list("Inbox", 1500),
    )?;

    let answers = session(
        &mut desk.compositor,
        "helper",
        &[("accessibility_tree", json!({ "window": id }))],
    )?;

    let tree = &answers[0]["structuredContent"];
    let elements = tree["elements"].as_array().cloned().unwrap_or_default();
    assert!(
        elements.len() <= hocus_platform::atspi::MAX_NODES,
        "{}",
        elements.len()
    );
    // Breadth first: the window and its list come before any cut.
    assert_eq!(elements[0]["label"], "Inbox");
    assert_eq!(elements[1]["label"], "Messages");
    let partial = tree["partial"].as_str().unwrap_or_default();
    assert!(
        partial.starts_with("only the first 800 elements")
            || partial.starts_with("reading took over"),
        "{partial}"
    );
    Ok(())
}

#[test]
fn an_agent_reads_the_trees_of_its_apps_windows_alone() -> anyhow::Result<()> {
    let mut desk = desk()?;
    let (client, _app, id) = open(
        &mut desk,
        "test.mail",
        "Compose",
        FakeElement::compose("Compose"),
    )?;

    let outsider = session(
        &mut desk.compositor,
        "outsider",
        &[
            ("accessibility_tree", json!({ "window": id })),
            ("click", json!({ "window": id, "label": "Send" })),
        ],
    )?;
    assert_eq!(
        said(&outsider[0]),
        "refused: agent \"outsider\" may not read windows of \"test.mail\": it reads the trees \
         of its apps alone, and its apps in [[agents.mcp]] do not match it"
    );
    assert!(
        said(&outsider[1]).starts_with("refused: agent \"outsider\" may not act on windows"),
        "{}",
        said(&outsider[1])
    );
    // A reader of its apps reads, and does not click.
    let reader = session(
        &mut desk.compositor,
        "reader",
        &[
            ("accessibility_tree", json!({ "window": id })),
            ("click", json!({ "window": id, "label": "Send" })),
        ],
    )?;
    assert_eq!(reader[0]["isError"], false, "{}", reader[0]);
    assert!(
        said(&reader[1]).starts_with("refused: agent \"reader\" may only read"),
        "{}",
        said(&reader[1])
    );
    desk.compositor.dispatch(Duration::from_millis(50))?;
    assert!(
        client
            .events()
            .iter()
            .all(|e| !matches!(e, ClientEvent::OnSeat(_, e) if matches!(**e, ClientEvent::Hit(_))))
    );
    Ok(())
}

/// The session's `org.a11y.Status`, as at-spi's bus launcher has it.
struct Status {
    enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[zbus::interface(name = "org.a11y.Status")]
impl Status {
    #[zbus(property)]
    fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[zbus(property)]
    fn set_is_enabled(&mut self, on: bool) {
        self.enabled.store(on, std::sync::atomic::Ordering::SeqCst);
    }
}

fn wait_enabled(enabled: &std::sync::atomic::AtomicBool, wanted: bool) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + TIMEOUT;
    while enabled.load(std::sync::atomic::Ordering::SeqCst) != wanted {
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "IsEnabled never became {wanted}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// While agents are there the session's accessibility is on, so that the
/// toolkits that wait for it expose their trees; it is set back once they
/// left, and only if it was off before.
#[test]
fn accessibility_is_on_while_agents_are_there() -> anyhow::Result<()> {
    use hocus_platform::atspi::{Bus, Toggle};
    use std::sync::{Arc, atomic::AtomicBool};

    let session = PrivateBus::start()?;
    let enabled = Arc::new(AtomicBool::new(false));
    let _status = zbus::block_on(
        zbus::connection::Builder::address(session.address.as_str())
            .serve_at(
                "/org/a11y/bus",
                Status {
                    enabled: enabled.clone(),
                },
            )
            .name("org.a11y.Bus")
            .build(),
    )?;
    let toggle = Toggle::start(&Bus::Session(session.address.clone()))?
        .ok_or_else(|| anyhow::anyhow!("no toggle on a session bus"))?;
    toggle.agents(true);
    wait_enabled(&enabled, true)?;
    toggle.agents(false);
    wait_enabled(&enabled, false)?;

    // On before the agents came: it stays on after they went.
    enabled.store(true, std::sync::atomic::Ordering::SeqCst);
    toggle.agents(true);
    toggle.agents(false);
    std::thread::sleep(Duration::from_millis(300));
    assert!(enabled.load(std::sync::atomic::Ordering::SeqCst));

    // No session bus: nothing to follow.
    assert!(Toggle::start(&Bus::Unknown)?.is_none());
    Ok(())
}
