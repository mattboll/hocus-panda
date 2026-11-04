//! The HTTP client the network's adapters share (Planka, Google): `ureq`,
//! blocking, on an account's worker; rustls on ring, against the system's
//! certificates (the platform's verifier, no bundled roots).

use std::{sync::Arc, time::Duration};

use crate::source::{Result, SourceError};

/// An agent whose answers each take at most `timeout`. A status is an
/// answer to read, not an error; a redirect is said, not followed (a
/// sign-in page in front of an API is not its answer).
pub(crate) fn agent(timeout: Duration) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent(concat!("hocuspanda/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// What no answer means: offline, `what` failed (TLS said as such).
pub(crate) fn unanswered(what: &str, err: &ureq::Error) -> SourceError {
    match err {
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => {
            SourceError::Offline(format!("{what}: TLS: {err}"))
        }
        _ => SourceError::Offline(format!("{what}: {err}")),
    }
}

/// An answer's status and its text, `most` bytes at most.
pub(crate) fn read(
    answer: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    what: &str,
    most: u64,
) -> Result<(u16, String)> {
    let mut response = answer.map_err(|err| unanswered(what, &err))?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(most)
        .read_to_string()
        .map_err(|err| SourceError::Offline(format!("{what}: {err}")))?;
    Ok((status, text))
}
