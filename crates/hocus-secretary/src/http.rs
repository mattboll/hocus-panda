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

/// Where an address goes, as the HTTP client itself reads it (`ureq`'s
/// own parser: what is checked here is what is called).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Over TLS, anywhere.
    Tls,
    /// In clear, to this machine.
    LocalClear,
    /// In clear to another machine, or not an address she calls.
    Refused,
}

/// A host that is this machine: `localhost`, 127.0.0.0/8, `::1` (an
/// address written whole: `127.1` or a number are names to look up, and
/// refused).
fn loopback(host: &str) -> bool {
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.to_canonical().is_loopback())
}

/// `address` read whole: its scheme, and its host compared, never its
/// first characters (`http://127.0.0.1:x@evil.example/` begins as a local
/// address and goes to `evil.example`). One that names a user or a
/// password before its host is refused whatever follows (#768).
fn reach(address: &str) -> Reach {
    let Ok(uri) = address.parse::<ureq::http::Uri>() else {
        return Reach::Refused;
    };
    let (Some(scheme), Some(authority)) = (uri.scheme_str(), uri.authority()) else {
        return Reach::Refused;
    };
    let host = authority.host();
    if host.is_empty() || authority.as_str().contains('@') {
        return Reach::Refused;
    }
    // After its host, a port in digits or nothing: the parser takes
    // `127.0.0.1:11434.evil.example` for the host `127.0.0.1` and a port
    // it has not read yet, and what a client makes of that is its own.
    let port = authority.as_str().strip_prefix(host).unwrap_or("?");
    let port_reads = port.is_empty()
        || port
            .strip_prefix(':')
            .is_some_and(|p| p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok());
    if !port_reads {
        return Reach::Refused;
    }
    match scheme {
        "https" => Reach::Tls,
        "http" if loopback(host) => Reach::LocalClear,
        _ => Reach::Refused,
    }
}

/// An address on this machine, in clear: `http://127.0.0.1:…`,
/// `http://[::1]:…`, `http://localhost:…`, with or without a port. What
/// says "nothing leaves this machine" (the local triage) stands on it.
pub fn local(address: &str) -> bool {
    reach(address) == Reach::LocalClear
}

/// Whether `address` may be called: over TLS, or in clear to this
/// machine only.
pub fn allowed(address: &str) -> bool {
    reach(address) != Reach::Refused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_is_local_whatever_its_port_and_case() {
        for address in [
            "http://127.0.0.1:11434",
            "http://127.0.0.1:11434/v1",
            "http://127.0.0.1",
            "http://127.0.0.1/v1/chat/completions",
            "http://127.8.9.10:8080/v1",
            "http://localhost:11434/v1",
            "http://localhost",
            "http://LOCALHOST:11434/v1",
            "HTTP://LocalHost:11434/v1",
            "http://[::1]:11434/v1",
            "http://[::1]",
            "http://[0:0:0:0:0:0:0:1]:8080/",
            "http://[::ffff:127.0.0.1]:8080/",
        ] {
            assert!(local(address), "{address} is this machine");
            assert!(allowed(address), "{address} may be called");
        }
    }

    #[test]
    fn an_address_that_only_begins_as_a_local_one_is_not() {
        for address in [
            // A user and a password: the host is what follows the @.
            "http://127.0.0.1:x@evil.example/",
            "http://127.0.0.1:11434@evil.example/v1",
            "http://localhost:11434@evil.example/v1",
            "http://localhost@evil.example/",
            "http://[::1]:80@evil.example/",
            // Even to this machine: an address names no user.
            "http://user:secret@127.0.0.1:11434/v1",
            "http://user@localhost/",
            // A name that begins as one.
            "http://localhost.evil.example/",
            "http://localhost.evil.example:11434/v1",
            "http://127.0.0.1.evil.example/",
            "http://127.0.0.1.evil.example:11434/v1",
            "http://localhost.:11434/",
            // Not an address written whole: a name to look up.
            "http://127.1:11434/",
            "http://2130706433/",
            "http://0x7f.0.0.1/",
            "http://0.0.0.0:11434/",
            "http://[::]:11434/",
            "http://[::ffff:10.0.0.1]/",
            // Another machine.
            "http://192.168.1.10:11434/v1",
            "http://evil.example/127.0.0.1:11434",
            "http://evil.example/?http://127.0.0.1:11434",
            "http://evil.example#@127.0.0.1/",
            // What reads as two addresses, or none.
            "http://127.0.0.1:11434\\@evil.example/",
            "http://127.0.0.1:11434 @evil.example/",
            "http://127.0.0.1:port/",
            "http://127.0.0.1:/",
            "http://127.0.0.1:99999/",
            "http://127.0.0.1:11434.evil.example/",
            "http://localhost:11434.evil.example/v1",
            "http://[::1]:11434.evil.example/",
            "http:127.0.0.1:11434",
            "http:///v1",
            "127.0.0.1:11434",
            "localhost",
            "/v1",
            "",
            "ftp://127.0.0.1/",
            "file:///etc/passwd",
        ] {
            assert!(!local(address), "{address:?} is not this machine");
            assert!(!allowed(address), "{address:?} is not called in clear");
        }
    }

    #[test]
    fn tls_goes_anywhere_but_names_no_user_and_is_not_local() {
        for address in [
            "https://api.anthropic.com",
            "https://api.anthropic.com/v1/messages",
            "HTTPS://API.ANTHROPIC.COM",
            "https://127.0.0.1:8443/v1",
        ] {
            assert!(allowed(address), "{address}");
            assert!(!local(address), "{address}: TLS is not the local claim");
        }
        for address in [
            "https://api.anthropic.com@evil.example/",
            "https://user:secret@api.anthropic.com/",
            "https://",
            "https:///v1",
            "https://api.anthropic.com:443.evil.example/",
        ] {
            assert!(!allowed(address), "{address:?}");
        }
    }
}
