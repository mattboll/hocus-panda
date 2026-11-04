//! `hocus day planka-token`: the token read on standard input, checked
//! with a fake Planka server of the test's own on 127.0.0.1, and kept in
//! the file the bar's worker reads. No bus: never the session's keyring;
//! fresh XDG directories.

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output, Stdio},
};

const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.e30.good";

/// A server whose `/api/users/me` takes [`TOKEN`] alone: its address.
fn fake_planka() -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let Ok(reader) = stream.try_clone() else {
                continue;
            };
            let mut reader = BufReader::new(reader);
            let (mut first, mut good) = (String::new(), false);
            let _ = reader.read_line(&mut first);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                    break;
                }
                let line = line.to_lowercase();
                good |= line.trim() == format!("authorization: bearer {}", TOKEN.to_lowercase());
            }
            let (status, body) = if !first.starts_with("GET /api/users/me ") {
                ("404 Not Found", r#"{"code":"E_NOT_FOUND"}"#)
            } else if good {
                (
                    "200 OK",
                    r#"{"item":{"id":"100","name":"Alex","username":"alex"},"included":{"notificationServices":[]}}"#,
                )
            } else {
                ("401 Unauthorized", r#"{"code":"E_UNAUTHORIZED"}"#)
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(address)
}

/// `hocus day planka-token ARGS` with `token` on its standard input, in
/// the fresh home `home`.
fn run(home: &Path, args: &[&str], token: &str) -> std::io::Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hocus"))
        .args(["day", "planka-token"])
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(token.as_bytes())?;
    }
    child.wait_with_output()
}

fn configure(home: &Path, address: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(home.join("config/hocuspanda"))?;
    std::fs::write(
        home.join("config/hocuspanda/config.toml"),
        format!("[[day.tasks]]\nkind = \"planka\"\naddress = \"{address}/boards/1234\"\n"),
    )
}

#[test]
fn a_token_is_checked_then_kept_where_the_bar_reads_it() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let address = fake_planka()?;
    configure(home.path(), &address)?;
    let port = address.rsplit(':').next().unwrap_or("");
    let account = format!("planka-tasks-127.0.0.1-{port}-1234");
    let file = home.path().join("state/hocuspanda/accounts").join(&account);

    // Refused by the server: nothing kept.
    let refused = run(home.path(), &[], "an.old.token\n")?;
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("refused or expired"), "{said}");
    assert!(!file.exists());

    // Taken: kept in a file of the user's alone, under the account the
    // workers start.
    let kept = run(home.path(), &[], &format!("{TOKEN}\n"))?;
    let said = String::from_utf8_lossy(&kept.stderr);
    assert!(kept.status.success(), "{said}");
    assert!(said.contains("signed in as Alex"), "{said}");
    assert!(String::from_utf8_lossy(&kept.stdout).contains(&account));
    assert_eq!(std::fs::read_to_string(&file)?, TOKEN);
    assert_eq!(
        std::fs::metadata(&file)?.permissions().mode() & 0o777,
        0o600
    );

    // Named, and unchecked.
    let named = run(
        home.path(),
        &[&account, "--no-check"],
        "accessToken=a.b.c; httpOnlyToken=d",
    )?;
    assert!(named.status.success());
    assert_eq!(
        std::fs::read_to_string(&file)?,
        "accessToken=a.b.c; httpOnlyToken=d"
    );
    Ok(())
}

#[test]
fn without_a_board_or_a_token_nothing_is_kept() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    let none = run(home.path(), &[], "a.b.c\n")?;
    assert!(!none.status.success());
    assert!(String::from_utf8_lossy(&none.stderr).contains("no Planka board"));
    configure(home.path(), "https://planka.example.org")?;
    let empty = run(home.path(), &["--no-check"], "\n")?;
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("no token"));
    assert!(!home.path().join("state/hocuspanda/accounts").exists());
    Ok(())
}
