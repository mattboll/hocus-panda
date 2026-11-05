//! `pw-dump --monitor`: PipeWire's own tool streams the graph's changes as
//! JSON, with no polling. Run on a thread of the caller's, never on a
//! loop that draws.

use std::{
    io::BufReader,
    process::{Child, Command, Stdio},
    sync::{Mutex, PoisonError},
};

use serde_json::Value;

use crate::Graph;

/// The `pw-dump` running: ended with the program, which it would
/// otherwise outlive until PipeWire's next change.
static DUMP: Mutex<Option<Child>> = Mutex::new(None);

/// Runs `pw-dump --monitor` until it ends, calling `changed` with the
/// graph after each of its updates; why it ended.
pub fn dump(mut changed: impl FnMut(&Graph)) -> String {
    let child = Command::new("pw-dump")
        .args(["--monitor", "--no-colors"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => return format!("pw-dump: {err}"),
    };
    let Some(out) = child.stdout.take() else {
        return "pw-dump: no output".to_owned();
    };
    *DUMP.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
    let mut graph = Graph::default();
    let updates =
        serde_json::Deserializer::from_reader(BufReader::new(out)).into_iter::<Vec<Value>>();
    for update in updates {
        let update = match update {
            Ok(update) => update,
            Err(err) => {
                tracing::warn!(%err, "pw-dump wrote what does not read");
                break;
            }
        };
        for object in update {
            graph.apply(object);
        }
        if graph.take_lost()
            && let Some(entries) = metadata()
        {
            graph.reset_metadata(&entries);
        }
        changed(&graph);
    }
    match end() {
        Some(Ok(status)) => format!("pw-dump ended: {status}"),
        Some(Err(err)) => format!("pw-dump: {err}"),
        None => "the program ends".to_owned(),
    }
}

fn end() -> Option<std::io::Result<std::process::ExitStatus>> {
    let mut child = DUMP.lock().unwrap_or_else(PoisonError::into_inner).take()?;
    let _ = child.kill();
    Some(child.wait())
}

/// The `default` metadata's entries, as `pw-metadata` lists them:
/// `update: id:0 key:'default.audio.sink' value:'{"name":"…"}' type:'…'`.
fn metadata() -> Option<Vec<(u32, String, String)>> {
    let out = Command::new("pw-metadata")
        .args(["-n", "default"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(entries(&String::from_utf8_lossy(&out.stdout)))
}

fn entries(listed: &str) -> Vec<(u32, String, String)> {
    let field = |line: &str, name: &str| -> Option<String> {
        let at = line.find(&format!("{name}:'"))? + name.len() + 2;
        let end = line[at..].find('\'')?;
        Some(line[at..at + end].to_owned())
    };
    listed
        .lines()
        .filter_map(|line| {
            let id = line
                .split("id:")
                .nth(1)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            Some((id, field(line, "key")?, field(line, "value")?))
        })
        .collect()
}

/// The program ends: its `pw-dump` too (by its own process).
pub fn stop() {
    end();
}

#[cfg(test)]
mod tests {
    #[test]
    fn pw_metadatas_lines_read() {
        let listed = "Found \"default\" metadata 29\n\
            update: id:0 key:'default.audio.sink' value:'{\"name\":\"tsink\"}' type:'Spa:String:JSON'\n\
            update: id:57 key:'target.object' value:'130' type:''\n";
        assert_eq!(
            super::entries(listed),
            [
                (
                    0,
                    "default.audio.sink".to_owned(),
                    "{\"name\":\"tsink\"}".to_owned()
                ),
                (57, "target.object".to_owned(), "130".to_owned()),
            ]
        );
    }
}
