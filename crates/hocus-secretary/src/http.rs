//! HTTP for her models, as the sources do it: `ureq`, blocking (on a
//! call's worker thread, never her loop), rustls on ring against the
//! system's certificates, no redirect followed. In clear only to this
//! machine (a local model's server, a test's fake).

use std::{sync::Arc, time::Duration};

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
        .user_agent(concat!("hocuspanda-secretary/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// An address on this machine: `http://127.0.0.1:…`, `http://[::1]:…`,
/// `http://localhost:…`.
pub fn local(address: &str) -> bool {
    ["http://127.0.0.1:", "http://[::1]:", "http://localhost:"]
        .iter()
        .any(|l| address.starts_with(l))
}

/// Whether `address` may be called: over TLS, or in clear to this
/// machine only.
pub fn allowed(address: &str) -> bool {
    address.starts_with("https://") || local(address)
}
