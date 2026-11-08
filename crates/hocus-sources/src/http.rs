//! HTTPS for CardDAV and the school calendar, as `hocus-day` does it:
//! `ureq`, blocking, rustls on ring against the system's certificates, no
//! redirect followed.

use std::{sync::Arc, time::Duration};

use crate::{Result, SourceError};

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
        .allow_non_standard_methods(true)
        .user_agent(concat!("hocuspanda/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// An answer's status and text, `most` bytes at most.
pub(crate) fn read(
    answer: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    what: &str,
    most: u64,
) -> Result<(u16, String)> {
    let mut response = answer.map_err(|err| SourceError::Offline(format!("{what}: {err}")))?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(most)
        .read_to_string()
        .map_err(|err| SourceError::Offline(format!("{what}: {err}")))?;
    Ok((status, text))
}

/// An address on this machine (a test's fake): `http://127.0.0.1:…`,
/// `http://[::1]:…`, `http://localhost:…`.
pub(crate) fn local(address: &str) -> bool {
    ["http://127.0.0.1:", "http://[::1]:", "http://localhost:"]
        .iter()
        .any(|l| address.starts_with(l))
}
