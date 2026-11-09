//! Her loop driven directly, as a connection would, on a clock of her own.

#![allow(dead_code)]

pub mod http;

use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use hocus_secretary::{
    brain::{Brain, Event, Options},
    clock::Clock,
    sources::Fixture,
    store::Store,
};
use hocus_secretary_link::{Client, FromSecretary, Status, ToSecretary, VERSION, line, read};

pub const START: &str = "2026-11-09T08:00:00Z";

pub fn clock() -> Clock {
    Clock::virtual_at(START.parse().expect("a time"))
}

pub fn fixture(text: &str) -> Fixture {
    toml::from_str(text).expect("a fixture")
}

/// Her loop on a thread, and one connection to it.
#[derive(Debug)]
pub struct Driver {
    events: Sender<Event>,
    lines: Receiver<String>,
    brain: JoinHandle<Store>,
    next: u64,
}

impl Driver {
    pub fn start(store: Store, options: Options) -> Self {
        let brain = Brain::new(store, options);
        let events = brain.sender();
        let brain = std::thread::spawn(move || brain.run());
        let (writer, lines) = mpsc::channel();
        events
            .send(Event::Connected {
                conn: 1,
                writer,
                closer: None,
            })
            .expect("her loop");
        let driver = Self {
            events,
            lines,
            brain,
            next: 1,
        };
        driver.send(&ToSecretary::Hello {
            version: VERSION,
            client: Client::Test,
            lang: None,
        });
        assert!(matches!(driver.receive(), FromSecretary::Hello { .. }));
        driver
    }

    pub fn send(&self, message: &ToSecretary) {
        self.events
            .send(Event::Line {
                conn: 1,
                line: line(message).trim_end().to_owned(),
            })
            .expect("her loop");
    }

    pub fn receive(&self) -> FromSecretary {
        let text = self
            .lines
            .recv_timeout(Duration::from_secs(20))
            .expect("a line from her");
        read(&text).expect("a line that reads")
    }

    pub fn request(&mut self, make: impl FnOnce(u64) -> ToSecretary) -> FromSecretary {
        let id = self.next;
        self.next += 1;
        self.send(&make(id));
        // Her typing and her answer's parts until the last, joined.
        let mut parts = String::new();
        loop {
            match self.receive() {
                message if message.id() != Some(id) => {}
                FromSecretary::Typing { .. } => {}
                FromSecretary::Answer { text, done, .. } => {
                    parts.push_str(&text);
                    if done {
                        return FromSecretary::Answer {
                            id,
                            text: parts,
                            done,
                        };
                    }
                }
                message => return message,
            }
        }
    }

    pub fn status(&mut self) -> Status {
        match self.request(|id| ToSecretary::Status { id }) {
            FromSecretary::Status { status, .. } => *status,
            other => panic!("not a status: {other:?}"),
        }
    }

    /// Her clock moved on, and every sync it made due done.
    pub fn advance(&mut self, seconds: u64) -> Status {
        self.send(&ToSecretary::Clock { advance: seconds });
        self.settle()
    }

    /// Waits until her status says `done`.
    pub fn until(&mut self, what: &str, done: impl Fn(&Status) -> bool) -> Status {
        let start = Instant::now();
        loop {
            let status = self.status();
            if done(&status) {
                return status;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "never {what}: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Her answer to a question, whole.
    pub fn answer_to(&mut self, text: &str) -> String {
        let mut answer = String::new();
        let id = self.next;
        self.next += 1;
        self.send(&ToSecretary::Ask {
            id,
            text: text.to_owned(),
        });
        loop {
            match self.receive() {
                FromSecretary::Answer { id: i, text, done } if i == id => {
                    answer.push_str(&text);
                    if done {
                        return answer;
                    }
                }
                FromSecretary::Error {
                    id: Some(i),
                    message,
                } if i == id => {
                    panic!("she could not answer: {message}")
                }
                _ => {}
            }
        }
    }

    /// Waits until no source syncs.
    pub fn settle(&mut self) -> Status {
        let start = Instant::now();
        loop {
            let status = self.status();
            if status.sources.iter().all(|s| !s.busy) {
                return status;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "never settled: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn stop(self) -> Store {
        self.events.send(Event::Stop).expect("her loop");
        self.brain.join().expect("her loop ended")
    }
}

pub fn options(sources: Vec<Fixture>) -> Options {
    Options {
        model: None,
        sources: sources
            .into_iter()
            .map(|f| Box::new(f) as Box<dyn hocus_secretary::sources::Source>)
            .collect(),
        key: "file".to_owned(),
        ..Options::default()
    }
}
