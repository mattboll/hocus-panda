//! The models downloaded on first use (`hocus_meetings::models`): each
//! written beside its place (`.part`), checked against its SHA-256, then
//! renamed; how far it is, said as it goes.

use std::{
    io::{Read, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use hocus_meetings::models::{self, Model};
use sha2::{Digest, Sha256};

/// How often the progress is said, at most.
const EVERY: Duration = Duration::from_millis(500);

/// The agent the models come through: rustls on ring, against the
/// system's certificates; redirects followed (the models' hosts hand
/// them over to their storage).
pub(crate) fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .max_redirects(5)
        .user_agent(concat!("hocuspanda/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// What `dir` lacks of `models`, downloaded into it; `said(done, total)`
/// as it goes.
pub fn missing(dir: &Path, models: &[Model], said: &mut dyn FnMut(u64, u64)) -> anyhow::Result<()> {
    let wanted = models::missing_of(dir, models);
    if wanted.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let total: u64 = wanted.iter().map(|m| m.bytes).sum();
    let agent = agent();
    let mut before = 0;
    for model in wanted {
        one(&agent, dir, model, &mut |done| said(before + done, total))?;
        before += model.bytes;
    }
    said(total, total);
    Ok(())
}

pub(crate) fn one(
    agent: &ureq::Agent,
    dir: &Path,
    model: Model,
    said: &mut dyn FnMut(u64),
) -> anyhow::Result<()> {
    let part = dir.join(format!("{}.part", model.file));
    let mut answer = agent
        .get(model.url)
        .call()
        .map_err(|err| anyhow::anyhow!("{}: {err}", model.url))?;
    let mut body = answer
        .body_mut()
        .with_config()
        .limit(model.bytes + 1)
        .reader();
    let mut file = std::io::BufWriter::new(std::fs::File::create(&part)?);
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    let (mut done, mut last) = (0u64, Instant::now());
    loop {
        let n = body.read(&mut buffer)?;
        let Some(read) = buffer.get(..n).filter(|r| !r.is_empty()) else {
            break;
        };
        hash.update(read);
        file.write_all(read)?;
        done += n as u64;
        if last.elapsed() >= EVERY {
            said(done.min(model.bytes));
            last = Instant::now();
        }
    }
    file.flush()?;
    drop(file);
    let sum: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != model.bytes || sum != model.sha256 {
        let _ = std::fs::remove_file(&part);
        anyhow::bail!(
            "{}: not the model expected ({done} bytes, SHA-256 {sum})",
            model.url
        );
    }
    std::fs::rename(&part, dir.join(model.file))?;
    Ok(())
}
