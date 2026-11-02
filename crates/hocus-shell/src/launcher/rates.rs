//! The currencies' rates (5.2): the European Central Bank's daily
//! reference rates, against the euro, kept in
//! `$XDG_STATE_HOME/hocuspanda/ecb-rates.xml` as the bank writes them.
//!
//! The file is read on a thread of its own as the shell starts. When the
//! launcher opens and the file is a day old or more (or missing), a thread
//! fetches it again from the bank, at most once an hour after a failure:
//! offline, the last rates known are used, and the card says their date.
//! An open launcher shows the new rates when they come.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant, SystemTime},
};

/// Where the bank publishes them.
const URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
/// How old the rates kept get before they are fetched again.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
/// How long after a failed fetch the next one is tried.
const AGAIN: Duration = Duration::from_secs(60 * 60);
/// The most a response may hold: the file is about 1.5 kB.
const MOST: u64 = 1 << 20;

/// Rates of a day: how many of each currency (its code in lower case) a
/// euro is worth, the euro's own included.
#[derive(Debug, Clone, PartialEq)]
pub struct Rates {
    /// Their day, `YYYY-MM-DD`.
    pub date: String,
    pub per_eur: HashMap<String, f64>,
}

/// The rates in the bank's XML: the `Cube` with a `time`, and the `Cube`s
/// in it with a `currency` and a `rate`.
pub fn parse(xml: &str) -> Option<Rates> {
    let document = roxmltree::Document::parse(xml).ok()?;
    let day = document
        .descendants()
        .find(|n| n.has_tag_name("Cube") && n.attribute("time").is_some())?;
    let date = day.attribute("time")?.to_owned();
    let mut per_eur: HashMap<String, f64> = day
        .children()
        .filter_map(|n| {
            let code = n.attribute("currency")?;
            let rate: f64 = n.attribute("rate")?.parse().ok()?;
            let fine = code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic());
            (fine && rate.is_finite() && rate > 0.0).then(|| (code.to_ascii_lowercase(), rate))
        })
        .collect();
    if per_eur.is_empty() {
        return None;
    }
    per_eur.insert("eur".to_owned(), 1.0);
    Some(Rates { date, per_eur })
}

/// Whether rates kept since `kept` are to be fetched again at `now`.
pub fn stale(kept: Option<SystemTime>, now: SystemTime) -> bool {
    kept.is_none_or(|kept| now.duration_since(kept).is_ok_and(|age| age >= DAY))
}

/// Where the rates are, and whether they are being read or fetched.
#[derive(Debug, Default)]
enum Fetch {
    /// The file kept is being read.
    #[default]
    Reading,
    /// Read: since when the rates are kept, and when a fetch last failed.
    Kept {
        since: Option<SystemTime>,
        failed: Option<Instant>,
    },
    Fetching,
}

static RATES: Mutex<Option<Arc<Rates>>> = Mutex::new(None);
static FETCH: Mutex<Fetch> = Mutex::new(Fetch::Reading);

/// The rates as last read or fetched.
pub fn current() -> Option<Arc<Rates>> {
    RATES.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn file() -> Option<PathBuf> {
    super::apps::state_file("ecb-rates.xml")
}

fn set(fetch: Fetch) {
    *FETCH.lock().unwrap_or_else(PoisonError::into_inner) = fetch;
}

/// Reads the rates kept, on a thread of its own.
pub fn start() {
    let started = std::thread::Builder::new()
        .name("launcher's rates".to_owned())
        .spawn(|| {
            let since = file().and_then(|file| read(&file));
            set(Fetch::Kept {
                since,
                failed: None,
            });
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot read the currencies' rates");
        set(Fetch::Kept {
            since: None,
            failed: None,
        });
    }
}

/// Reads `file`'s rates, and says since when they are kept.
fn read(file: &Path) -> Option<SystemTime> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!(file = %file.display(), %err, "the currencies' rates not read");
            return None;
        }
    };
    let Some(rates) = parse(&text) else {
        tracing::warn!(file = %file.display(), "the currencies' rates kept are not the bank's");
        return None;
    };
    tracing::debug!(date = rates.date, "the currencies' rates kept");
    *RATES.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(rates));
    std::fs::metadata(file).and_then(|m| m.modified()).ok()
}

/// The launcher opens: the rates are fetched again if they are a day old.
pub fn refresh() {
    let Some(file) = file() else {
        return;
    };
    let due = {
        let mut fetch = FETCH.lock().unwrap_or_else(PoisonError::into_inner);
        let due = match &*fetch {
            Fetch::Kept { since, failed } => {
                stale(*since, SystemTime::now()) && failed.is_none_or(|at| at.elapsed() >= AGAIN)
            }
            Fetch::Reading | Fetch::Fetching => false,
        };
        if due {
            *fetch = Fetch::Fetching;
        }
        due
    };
    if !due {
        return;
    }
    let started = std::thread::Builder::new()
        .name("launcher's rates fetched".to_owned())
        .spawn(move || match fetch(&file) {
            Ok(since) => {
                set(Fetch::Kept {
                    since: Some(since),
                    failed: None,
                });
                super::changed();
            }
            Err(err) => {
                tracing::info!(%err, "the currencies' rates not fetched: the last ones are used");
                let since = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
                set(Fetch::Kept {
                    since,
                    failed: Some(Instant::now()),
                });
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot fetch the currencies' rates");
        set(Fetch::Kept {
            since: None,
            failed: Some(Instant::now()),
        });
    }
}

/// Fetches the rates into `file`; when they are kept from.
fn fetch(file: &Path) -> Result<SystemTime, Box<dyn std::error::Error + Send + Sync>> {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let text = agent
        .get(URL)
        .call()?
        .body_mut()
        .with_config()
        .limit(MOST)
        .read_to_string()?;
    let rates = parse(&text).ok_or("not the bank's rates")?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let new = file.with_extension("new");
    std::fs::write(&new, &text)?;
    std::fs::rename(&new, file)?;
    tracing::info!(date = rates.date, "the currencies' rates fetched");
    *RATES.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(rates));
    Ok(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bank's file, shortened.
    const ECB: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<gesmes:Envelope xmlns:gesmes=\"http://www.gesmes.org/xml/2002-08-01\" \
xmlns=\"http://www.ecb.int/vocabulary/2002-08-01/eurofxref\">
	<gesmes:subject>Reference rates</gesmes:subject>
	<gesmes:Sender><gesmes:name>European Central Bank</gesmes:name></gesmes:Sender>
	<Cube>
		<Cube time='2026-10-30'>
			<Cube currency='USD' rate='1.0812'/>
			<Cube currency='JPY' rate='163.42'/>
			<Cube currency='GBP' rate='0.8601'/>
			<Cube currency='bad' rate='-1'/>
		</Cube>
	</Cube>
</gesmes:Envelope>";

    #[test]
    fn the_banks_rates_are_read_and_kept_a_day() {
        let rates = parse(ECB).expect("rates");
        assert_eq!(rates.date, "2026-10-30");
        assert_eq!(rates.per_eur.get("usd"), Some(&1.0812));
        assert_eq!(rates.per_eur.get("eur"), Some(&1.0));
        assert_eq!(rates.per_eur.len(), 4);
        assert_eq!(parse("<Cube/>"), None);
        assert_eq!(parse("not xml"), None);
        let now = SystemTime::now();
        assert!(stale(None, now));
        assert!(!stale(Some(now - Duration::from_secs(3600)), now));
        assert!(stale(Some(now - DAY), now));
        // A clock set back: kept from the future, not fetched again.
        assert!(!stale(Some(now + DAY), now));
    }
}
