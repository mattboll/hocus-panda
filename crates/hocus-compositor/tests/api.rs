//! The control socket as a public interface (docs/api.md, #833), headless,
//! no shell: the version line; each line the page documents, given the
//! answer its example says; a subscriber told of the windows, a desktop,
//! the screens and an agent; one that does not read told how many events
//! it lost.
//!
//! The page is the reference: its `control` blocks are run as they are
//! written, and every event a test sees is held to the `event` block of
//! its name, so that the page and the desktop cannot drift apart unseen.

use crate::common;

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use anyhow::Context;
use common::{Command, KEY_1, KEY_2, KEY_LEFTMETA, WAIT, chord, open_with};
use hocus_compositor::Headless;
use serde_json::Value;

/// The page.
const PAGE: &str = include_str!("../../../docs/api.md");

/// The lines of each block of the page fenced as `fence`.
fn blocks(fence: &str) -> Vec<Vec<&'static str>> {
    let opening = format!("```{fence}");
    let mut out = Vec::new();
    let mut lines = PAGE.lines();
    while let Some(line) = lines.next() {
        if line.trim_end() == opening {
            out.push(
                lines
                    .by_ref()
                    .take_while(|l| !l.starts_with("```"))
                    .collect(),
            );
        }
    }
    out
}

/// A `control` block: the line written, the answer's lines (`…` any
/// text), and whether further lines may follow.
#[derive(Debug)]
struct Example {
    line: String,
    answer: Vec<String>,
    more: bool,
}

fn examples() -> Vec<Example> {
    blocks("control")
        .into_iter()
        .map(|block| {
            let line = block
                .iter()
                .find_map(|l| l.strip_prefix("> "))
                .expect("a control block writes a line")
                .to_owned();
            let mut answer: Vec<String> = block
                .iter()
                .filter_map(|l| l.strip_prefix('<'))
                .map(|l| l.strip_prefix(' ').unwrap_or(l).to_owned())
                .collect();
            let more = answer.last().is_some_and(|l| l == "…");
            if more {
                answer.pop();
            }
            Example { line, answer, more }
        })
        .collect()
}

/// Whether `text` is what `pattern` says, `…` standing for any text.
fn matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('…').collect();
    let [first, middle @ .., last] = parts.as_slice() else {
        return pattern == text;
    };
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// The documented events, by name.
fn documented_events() -> Vec<(String, Value)> {
    blocks("event")
        .into_iter()
        .flatten()
        .map(|line| {
            let event: Value = serde_json::from_str(line).expect("an event block holds JSON");
            let name = event["event"]
                .as_str()
                .expect("an event has a name")
                .to_owned();
            (name, event)
        })
        .collect()
}

/// Whether `got` has `doc`'s shape: the same keys, values of the same
/// types; `null` on either side stands for any value.
fn same_shape(doc: &Value, got: &Value) -> bool {
    match (doc, got) {
        (Value::Null, _) | (_, Value::Null) => true,
        (Value::Object(d), Value::Object(g)) => {
            d.len() == g.len()
                && d.iter()
                    .all(|(k, v)| g.get(k).is_some_and(|w| same_shape(v, w)))
        }
        (Value::Array(d), Value::Array(g)) => match d.first() {
            Some(first) => g.iter().all(|w| same_shape(first, w)),
            None => g.is_empty(),
        },
        (Value::String(_), Value::String(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::Bool(_), Value::Bool(_)) => true,
        _ => false,
    }
}

/// `event` has the shape of the documented event of its name.
#[track_caller]
fn documented(event: &Value) {
    let name = event["event"].as_str().unwrap_or_default();
    let docs = documented_events();
    let doc = docs
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("docs/api.md documents no event {name:?}: {event}"));
    assert!(
        same_shape(&doc.1, event),
        "{event} is not shaped as docs/api.md's {}",
        doc.1
    );
}

fn socket(hp: &Headless) -> anyhow::Result<PathBuf> {
    hp.control_socket()
        .map(Path::to_path_buf)
        .context("no control socket")
}

/// Writes `line` on a connection of its own and reads the answer: until
/// the desktop closes it, or its first `lines` lines when it may stay
/// open. Off the loop, which runs meanwhile.
fn exchange(hp: &mut Headless, line: &str, lines: Option<usize>) -> anyhow::Result<Vec<String>> {
    let path = socket(hp)?;
    let line = line.to_owned();
    let (told, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let talked = (|| -> std::io::Result<Vec<String>> {
            let mut stream = UnixStream::connect(&path)?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.write_all(format!("{line}\n").as_bytes())?;
            match lines {
                Some(n) => BufReader::new(stream).lines().take(n).collect(),
                None => {
                    // A report's connection is read until it ends.
                    stream.shutdown(std::net::Shutdown::Write)?;
                    let mut text = String::new();
                    stream.read_to_string(&mut text)?;
                    Ok(text.lines().map(str::to_owned).collect())
                }
            }
        })();
        let _ = told.send(talked);
    });
    let until = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < until {
        hp.dispatch(Duration::from_millis(5))?;
        if let Ok(talked) = answer.try_recv() {
            return Ok(talked?);
        }
    }
    anyhow::bail!("no answer")
}

#[test]
fn the_version_line_says_the_interface_and_the_package() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let answer = exchange(&mut hp, "msg version", None)?;
    assert_eq!(answer.len(), 1, "{answer:?}");
    let said: Value = serde_json::from_str(&answer[0])?;
    assert_eq!(said["api"], hocus_agents::API);
    assert_eq!(said["oldest"], hocus_agents::API_OLDEST);
    assert_eq!(said["hocuspanda"], env!("CARGO_PKG_VERSION"));
    // `hocus msg version` prints it as it came.
    let hocus = common::hocus_program();
    if !hocus.exists() {
        common::missing("hocus, built beside the tests (cargo programs)");
    }
    let path = socket(&hp)?;
    let (told, printed) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = told.send(
            std::process::Command::new(hocus)
                .arg("--socket")
                .arg(path)
                .args(["msg", "version"])
                .output(),
        );
    });
    let output = loop {
        hp.dispatch(Duration::from_millis(5))?;
        if let Ok(output) = printed.try_recv() {
            break output?;
        }
    };
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("{}\n", answer[0])
    );
    Ok(())
}

#[test]
fn each_documented_line_gets_its_documented_answer() -> anyhow::Result<()> {
    let mut hp = common::compositor()?;
    let examples = examples();
    assert!(examples.len() >= 13, "{examples:?}");
    for example in &examples {
        let lines = example.more.then_some(example.answer.len());
        let answer = exchange(&mut hp, &example.line, lines)?;
        assert!(
            answer.len() == example.answer.len()
                && example
                    .answer
                    .iter()
                    .zip(&answer)
                    .all(|(pattern, line)| matches(pattern, line)),
            "{:?} answered {answer:?}, docs/api.md says {:?}",
            example.line,
            example.answer
        );
    }
    // The reports did what they say: the agent is there.
    assert!(hp.agents().iter().any(|a| a.name == "build"));
    Ok(())
}

/// A subscriber's events as they come, read on a thread of their own.
struct Subscriber {
    events: mpsc::Receiver<Value>,
    seen: Vec<Value>,
}

impl Subscriber {
    fn start(hp: &mut Headless, kinds: &str) -> anyhow::Result<Self> {
        let mut stream = UnixStream::connect(socket(hp)?)?;
        stream.write_all(format!("msg subscribe {kinds}\n").as_bytes())?;
        let (told, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let Ok(event) = serde_json::from_str::<Value>(&line) else {
                    panic!("not JSON: {line:?}");
                };
                if told.send(event).is_err() {
                    break;
                }
            }
        });
        let mut subscriber = Self {
            events,
            seen: Vec::new(),
        };
        subscriber.next(hp, |e| e["event"] == "subscribed")?;
        Ok(subscriber)
    }

    /// Runs the compositor until an event `is`; the events before it are
    /// kept too.
    fn next(&mut self, hp: &mut Headless, is: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let until = std::time::Instant::now() + WAIT;
        while std::time::Instant::now() < until {
            hp.dispatch(Duration::from_millis(5))?;
            while let Ok(event) = self.events.try_recv() {
                self.seen.push(event.clone());
                if is(&event) {
                    return Ok(event);
                }
            }
        }
        anyhow::bail!("never came; seen: {:?}", self.seen)
    }
}

#[test]
fn a_subscriber_is_told_windows_desktops_screens_and_agents() -> anyhow::Result<()> {
    let mut hp = common::two_screens()?;
    let mut sub = Subscriber::start(&mut hp, "window,desktop,screen,agent")?;
    // A window opened, given the keyboard, then closed.
    let (_, client) = open_with(&mut hp, "api-window", (400, 300), false)?;
    let opened = sub.next(&mut hp, |e| e["event"] == "window-opened")?;
    assert_eq!(opened["window"]["app_id"], "api-window");
    let id = opened["window"]["id"].clone();
    sub.next(&mut hp, |e| {
        e["event"] == "window-focused" && e["window"]["id"] == id
    })?;
    // Another desktop, and back.
    chord(&mut hp, &[KEY_LEFTMETA, KEY_2]);
    let switched = sub.next(&mut hp, |e| e["event"] == "desktop-switched")?;
    assert_eq!(switched["screen"]["name"], "HEADLESS-1");
    assert_eq!(switched["desktop"]["number"], 2);
    chord(&mut hp, &[KEY_LEFTMETA, KEY_1]);
    sub.next(&mut hp, |e| {
        e["event"] == "desktop-switched" && e["desktop"]["number"] == 1
    })?;
    client.commands.send(Command::Close)?;
    let closed = sub.next(&mut hp, |e| e["event"] == "window-closed")?;
    assert_eq!(closed["window"]["id"], id);
    // A screen unplugged, then plugged again.
    hp.unplug("HEADLESS-2")?;
    let gone = sub.next(&mut hp, |e| e["event"] == "screen-unplugged")?;
    assert_eq!(gone["screen"]["name"], "HEADLESS-2");
    hp.plug("HEADLESS-2")?;
    sub.next(&mut hp, |e| {
        e["event"] == "screen-plugged" && e["screen"]["name"] == "HEADLESS-2"
    })?;
    // An agent's state, then its end.
    exchange(&mut hp, "agent command build working - build", None)?;
    let agent = sub.next(&mut hp, |e| e["event"] == "agent-state")?;
    assert_eq!(agent["agent"]["name"], "build");
    assert_eq!(agent["agent"]["state"], "working");
    exchange(&mut hp, "agent command build ended - build", None)?;
    sub.next(&mut hp, |e| {
        e["event"] == "agent-state" && e["agent"]["state"] == "ended"
    })?;
    // Every event as the page says it, and every one it documents seen
    // (but the loss, the next test's).
    for event in &sub.seen {
        documented(event);
    }
    for (name, _) in documented_events() {
        assert!(
            name == "lost" || sub.seen.iter().any(|e| e["event"] == name.as_str()),
            "no {name} seen: {:?}",
            sub.seen
        );
    }
    Ok(())
}

#[test]
fn a_subscriber_that_does_not_read_is_told_how_many_it_lost() -> anyhow::Result<()> {
    const AGENTS: usize = 1000;
    let mut hp = common::compositor()?;
    let path = socket(&hp)?;
    // Subscribed, then not read while the agents come.
    let mut stream = UnixStream::connect(&path)?;
    stream.write_all(b"msg subscribe agent\n")?;
    anyhow::ensure!(
        hp.dispatch_until(WAIT, |hp| hp.subscribers() == 1)?,
        "never subscribed"
    );
    let reports = std::thread::spawn(move || -> std::io::Result<()> {
        for chunk in (0..AGENTS).collect::<Vec<_>>().chunks(64) {
            let mut report = UnixStream::connect(&path)?;
            let lines: String = chunk
                .iter()
                .map(|i| format!("agent command a{i} working - a{i}\n"))
                .collect();
            report.write_all(lines.as_bytes())?;
            report.shutdown(std::net::Shutdown::Write)?;
        }
        Ok(())
    });
    let until = std::time::Instant::now() + WAIT;
    while hp.agents().len() < AGENTS {
        anyhow::ensure!(
            std::time::Instant::now() < until,
            "the agents never all came"
        );
        hp.dispatch(Duration::from_millis(5))?;
    }
    reports.join().map_err(|_| anyhow::anyhow!("reports"))??;
    // Now read: every agent is either told or counted as lost.
    stream.set_read_timeout(Some(WAIT))?;
    let (told, read) = mpsc::channel();
    std::thread::spawn(move || {
        let mut events: Vec<Value> = Vec::new();
        let (mut agents, mut lost) = (0, 0);
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                break;
            };
            match event["event"].as_str() {
                Some("agent-state") => agents += 1,
                Some("lost") => lost += event["count"].as_u64().unwrap_or_default() as usize,
                _ => {}
            }
            events.push(event);
            if agents + lost >= AGENTS {
                break;
            }
        }
        let _ = told.send(events);
    });
    let events = loop {
        hp.dispatch(Duration::from_millis(5))?;
        if let Ok(events) = read.try_recv() {
            break events;
        }
    };
    let told = events
        .iter()
        .filter(|e| e["event"] == "agent-state")
        .count();
    let lost: u64 = events
        .iter()
        .filter(|e| e["event"] == "lost")
        .map(|e| e["count"].as_u64().unwrap_or_default())
        .sum();
    assert!(lost > 0, "nothing lost of {AGENTS}");
    assert_eq!(told + lost as usize, AGENTS);
    assert!(told >= hocus_compositor::EVENTS_KEPT, "{told} told");
    for event in &events {
        documented(event);
    }
    Ok(())
}
