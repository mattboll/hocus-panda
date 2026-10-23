//! Acceptance scenarios of the agent API over MCP (#42). The server's own
//! loop (what `hocus mcp` runs on stdin and stdout) reads a session's lines
//! from a thread, against a headless compositor started as the session is:
//! without the test permission, so that only an agent's own rights let it
//! act.

use std::{thread::JoinHandle, time::Duration};

use hocus_api::{Permission, Reply, Request, mcp::Bridge};
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, RendererChoice};
use hocus_test::client::{ClientEvent, ClientOptions, SolidWindow, TestClient};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(10);

const AGENTS: &str = r#"
[[agents.mcp]]
name = "reader"

[[agents.mcp]]
name = "helper"
access = "act"
apps = ["test.notes"]
"#;

/// A compositor as the session starts it, with the agents configured and
/// one window titled `Notes`.
fn desktop() -> anyhow::Result<(Headless, TestClient, u64)> {
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
    let client = SolidWindow {
        app_id: "test.notes".into(),
        width: 400,
        height: 300,
        argb: 0xff_33_99_ff,
    }
    .spawn_with(
        compositor.socket_path(),
        ClientOptions {
            title: Some("Notes".into()),
            ..ClientOptions::default()
        },
    )?;
    compositor.dispatch_until(TIMEOUT, |c| c.window_count() == 1)?;
    let Reply::Windows(windows) = compositor.request(Request::Windows)? else {
        anyhow::bail!("expected windows");
    };
    let id = windows.first().map(|w| w.id).unwrap_or_default();
    Ok((compositor, client, id))
}

/// Runs a whole session, line by line as a client writes it on the
/// server's stdin, and returns the answers it read on its stdout.
fn session(
    compositor: &mut Headless,
    agent: Option<&str>,
    messages: &[Value],
) -> anyhow::Result<Vec<Value>> {
    let socket = compositor.api_socket().to_path_buf();
    let agent = agent.map(str::to_owned);
    let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
    let thread: JoinHandle<std::io::Result<Vec<u8>>> = std::thread::spawn(move || {
        let mut output = Vec::new();
        Bridge::new(socket, agent).serve(input.as_bytes(), &mut output)?;
        Ok(output)
    });
    compositor.dispatch_until(TIMEOUT, |_| thread.is_finished())?;
    let output = thread
        .join()
        .map_err(|_| anyhow::anyhow!("the MCP server panicked"))??;
    String::from_utf8(output)?
        .lines()
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

fn initialize() -> [Value; 2] {
    [
        json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
            },
        }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ]
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    })
}

fn answer(answers: &[Value], id: u64) -> anyhow::Result<&Value> {
    answers
        .iter()
        .find(|a| a["id"] == id)
        .ok_or_else(|| anyhow::anyhow!("no answer {id} among {answers:?}"))
}

fn clicks(client: &TestClient) -> usize {
    client
        .events()
        .iter()
        .filter(|e| matches!(e, ClientEvent::Button { pressed: true, .. }))
        .count()
}

#[test]
fn given_an_mcp_client_when_it_lists_windows_then_it_receives_ids_titles_outputs_and_geometry()
-> anyhow::Result<()> {
    let (mut compositor, _client, id) = desktop()?;

    let [init, initialized] = initialize();
    let answers = session(
        &mut compositor,
        Some("reader"),
        &[init, initialized, call(1, "list_windows", json!({}))],
    )?;

    assert_eq!(answers.len(), 2, "the notification gets no answer");
    assert_eq!(
        answer(&answers, 0)?["result"]["protocolVersion"],
        "2025-06-18"
    );
    let result = &answer(&answers, 1)?["result"];
    assert_eq!(result["isError"], false);
    let windows = &result["structuredContent"]["windows"];
    assert_eq!(
        windows,
        &json!([{
            "id": id,
            "title": "Notes",
            "app_id": "test.notes",
            "output": "HEADLESS-1",
            "geometry": { "x": 440, "y": 250, "width": 400, "height": 300 },
            "focused": true,
            "visible": true,
            "state": "normal",
            "desktop": windows[0]["desktop"],
        }])
    );
    // The same as text, for clients that read only that.
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("\"title\": \"Notes\""), "{text}");
    Ok(())
}

#[test]
fn given_an_agent_with_read_only_permission_when_it_tries_to_click_then_it_is_refused()
-> anyhow::Result<()> {
    let (mut compositor, client, id) = desktop()?;

    let [init, initialized] = initialize();
    let answers = session(
        &mut compositor,
        Some("reader"),
        &[
            init,
            initialized,
            call(1, "click", json!({ "window": id, "x": 20, "y": 20 })),
            call(2, "type", json!({ "window": id, "text": "rm" })),
        ],
    )?;

    for n in [1, 2] {
        let result = &answer(&answers, n)?["result"];
        assert_eq!(result["isError"], true, "{result}");
        let message = result["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            message.starts_with("refused: agent \"reader\" may only read"),
            "{message}"
        );
    }
    compositor.dispatch(Duration::from_millis(50))?;
    assert_eq!(clicks(&client), 0, "the window saw no click");
    Ok(())
}

#[test]
fn an_agent_that_may_act_clicks_its_apps_window_without_the_test_permission() -> anyhow::Result<()>
{
    let (mut compositor, client, id) = desktop()?;

    let [init, initialized] = initialize();
    let answers = session(
        &mut compositor,
        Some("helper"),
        &[
            init,
            initialized,
            call(1, "click", json!({ "window": id, "x": 20, "y": 20 })),
        ],
    )?;

    assert_eq!(answer(&answers, 1)?["result"]["isError"], false);
    compositor.dispatch_until(TIMEOUT, |_| clicks(&client) == 1)?;
    Ok(())
}

#[test]
fn an_agent_connection_makes_only_the_requests_agents_may() -> anyhow::Result<()> {
    let (mut compositor, _client, _) = desktop()?;
    let socket = compositor.api_socket().to_path_buf();

    let thread = std::thread::spawn(move || {
        let mut client = hocus_api::Client::connect(&socket)?;
        client.set_timeout(Some(TIMEOUT))?;
        client.request(Request::Agent {
            name: "helper".into(),
        })?;
        Ok::<_, hocus_api::ClientError>((
            client.request(Request::Config),
            client.request(Request::PointerMove { x: 1.0, y: 1.0 }),
            client.request(Request::Agent {
                name: "reader".into(),
            }),
        ))
    });
    compositor.dispatch_until(TIMEOUT, |_| thread.is_finished())?;
    let (config, raw, renamed) = thread
        .join()
        .map_err(|_| anyhow::anyhow!("the client panicked"))??;

    for refused in [config, raw] {
        let Err(hocus_api::ClientError::Api(err)) = refused else {
            anyhow::bail!("expected a refusal, got {refused:?}");
        };
        assert_eq!(err.code, hocus_api::ErrorCode::PermissionDenied);
        assert!(err.message.contains("agent \"helper\" may not"), "{err}");
    }
    assert!(
        renamed.is_err(),
        "once named, a connection stays that agent"
    );
    Ok(())
}

/// What a tool said, as text.
fn said(answer: &Value) -> &str {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
}

#[test]
fn given_the_stop_key_pressed_when_a_connected_agent_tries_to_act_then_it_is_refused_until_allowed_again()
-> anyhow::Result<()> {
    let (mut compositor, client, id) = desktop()?;
    let socket = compositor.api_socket().to_path_buf();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let (done, waited) = std::sync::mpsc::channel::<()>();

    // One session throughout, as an agent stays connected.
    let session = std::thread::spawn(move || {
        let mut bridge = Bridge::new(socket, Some("helper".into()));
        for message in initialize() {
            bridge.handle(message);
        }
        let click = || call(0, "click", json!({ "window": id, "x": 20, "y": 20 }));
        let mut answers =
            vec![bridge.handle(call(1, "type", json!({ "window": id, "text": "ab" })))];
        // Stopped.
        let _ = done.send(());
        let _ = wait.recv();
        answers.push(bridge.handle(click()));
        answers.push(bridge.handle(call(2, "journal", json!({}))));
        // Allowed again.
        let _ = done.send(());
        let _ = wait.recv();
        answers.push(bridge.handle(click()));
        answers.push(bridge.handle(call(3, "journal", json!({}))));
        answers
    });
    let step = |compositor: &mut Headless, stop: bool| -> anyhow::Result<()> {
        compositor.dispatch_until(TIMEOUT, |_| waited.try_recv().is_ok())?;
        compositor.on_loop("the stop key", move |state| state.set_agents_stopped(stop));
        compositor.dispatch(Duration::from_millis(10))?;
        go.send(())?;
        Ok(())
    };
    step(&mut compositor, true)?;
    step(&mut compositor, false)?;
    compositor.dispatch_until(TIMEOUT, |_| session.is_finished())?;
    let answers: Vec<Value> = session
        .join()
        .map_err(|_| anyhow::anyhow!("the MCP session panicked"))?
        .into_iter()
        .map(|a| a.unwrap_or_default())
        .collect();

    let [typed, refused, unread, clicked, journal] = &answers[..] else {
        anyhow::bail!("expected five answers, got {answers:?}");
    };
    assert_eq!(typed["result"]["isError"], false, "{typed}");
    for refusal in [refused, unread] {
        assert_eq!(refusal["result"]["isError"], true, "{refusal}");
        assert!(
            said(refusal).starts_with("refused: the user stopped every agent"),
            "{refusal}"
        );
    }
    assert_eq!(clicked["result"]["isError"], false, "{clicked}");
    compositor.dispatch_until(TIMEOUT, |_| clicks(&client) == 1)?;

    // In order, with the window, the refusal between.
    let lines: Vec<&str> = said(journal).lines().collect();
    let window = format!("on window {id} test.notes \"Notes\"");
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].ends_with(&format!("helper: type 2 characters {window}")),
        "{lines:?}"
    );
    assert!(
        lines[1].contains(&format!(
            "helper: click {window}; refused: the user stopped"
        )),
        "{lines:?}"
    );
    assert!(
        lines[2].ends_with(&format!("helper: click {window}")),
        "{lines:?}"
    );
    assert_eq!(journal["result"]["structuredContent"]["stopped"], false);

    // The user reads it with the read permission.
    let Reply::AgentsJournal(read) = compositor.request(Request::AgentsJournal)? else {
        anyhow::bail!("expected the journal");
    };
    assert_eq!(read.entries.len(), 3);
    assert!(!read.stopped);

    // Another agent reads only its own entries: none.
    let socket = compositor.api_socket().to_path_buf();
    let other = std::thread::spawn(move || {
        let mut client = hocus_api::Client::connect(&socket)?;
        client.set_timeout(Some(TIMEOUT))?;
        client.request(Request::Agent {
            name: "reader".into(),
        })?;
        client.request(Request::AgentsJournal)
    });
    compositor.dispatch_until(TIMEOUT, |_| other.is_finished())?;
    let Reply::AgentsJournal(own) = other
        .join()
        .map_err(|_| anyhow::anyhow!("the client panicked"))??
    else {
        anyhow::bail!("expected the journal");
    };
    assert!(own.entries.is_empty(), "{own:?}");
    Ok(())
}
