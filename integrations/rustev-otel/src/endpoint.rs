//! The collector endpoint allowlist (spec 028, 3.9.2): every OTLP
//! destination is checked here, before any connection or DNS lookup, and
//! refused unless its origin is exactly in the host's list.
//!
//! An origin is `scheme://host:port` with the scheme and host lowercased,
//! the default port made explicit and a trailing dot removed. A URL with
//! user information, a wildcard, an empty list, and plaintext to a host that
//! is not a loopback IP literal (`127.0.0.0/8` or `::1`; the name
//! `localhost` is refused, since its resolution is not checked) are
//! refused. `OTEL_*` environment variables cannot choose a destination: one
//! naming an origin outside the list is refused like a configured one.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

/// The environment variables that could name an OTLP destination.
pub const ENDPOINT_VARS: [&str; 4] = [
    "OTEL_EXPORTER_OTLP_ENDPOINT",
    "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
    "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
    "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
];

/// A URL scheme the exporters speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scheme {
    Http,
    Https,
}

/// A normalized origin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Origin {
    pub scheme: Scheme,
    /// Lowercase, no trailing dot; an IPv6 literal without brackets.
    pub host: String,
    pub port: u16,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = match self.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        };
        if self.host.contains(':') {
            write!(f, "{scheme}://[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{scheme}://{}:{}", self.host, self.port)
        }
    }
}

impl Origin {
    /// A loopback IP literal: `127.0.0.0/8` or `::1`.
    pub fn is_loopback_literal(&self) -> bool {
        if let Ok(v4) = self.host.parse::<Ipv4Addr>() {
            return v4.is_loopback();
        }
        self.host
            .parse::<Ipv6Addr>()
            .is_ok_and(|v6| v6 == Ipv6Addr::LOCALHOST)
    }
}

/// Why an endpoint or allowlist entry was refused. Never carries a header
/// value or other secret; an origin is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    /// The origin is not in the allowlist.
    EndpointNotAllowed { origin: String },
    /// The allowlist is empty, which refuses every endpoint.
    EmptyAllowlist,
    /// Not a URL this check reads; `what` says why.
    Malformed { what: &'static str },
    /// User information in the authority.
    UserInfo,
    /// A wildcard in a host or entry.
    Wildcard,
    /// Plaintext to a host that is not a loopback IP literal.
    PlaintextNotLoopback { origin: String },
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EndpointError::EndpointNotAllowed { origin } => {
                write!(f, "EndpointNotAllowed: {origin} is not in the allowlist")
            }
            EndpointError::EmptyAllowlist => f.write_str("the endpoint allowlist is empty"),
            EndpointError::Malformed { what } => write!(f, "malformed endpoint: {what}"),
            EndpointError::UserInfo => f.write_str("an endpoint may not carry user information"),
            EndpointError::Wildcard => f.write_str("an endpoint may not carry a wildcard"),
            EndpointError::PlaintextNotLoopback { origin } => write!(
                f,
                "{origin}: plaintext is allowed only to a loopback IP literal"
            ),
        }
    }
}

impl std::error::Error for EndpointError {}

fn malformed<T>(what: &'static str) -> Result<T, EndpointError> {
    Err(EndpointError::Malformed { what })
}

/// Parse a URL's origin; the path, if any, is returned beside it.
pub fn parse(url: &str) -> Result<(Origin, String), EndpointError> {
    let url = url.trim();
    if url.contains('*') {
        return Err(EndpointError::Wildcard);
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return malformed("no scheme");
    };
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "http" => Scheme::Http,
        "https" => Scheme::Https,
        _ => return malformed("the scheme is not http or https"),
    };
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return Err(EndpointError::UserInfo);
    }
    if authority.is_empty() {
        return malformed("no host");
    }
    let (host, port) = if let Some(after) = authority.strip_prefix('[') {
        let Some((h, tail)) = after.split_once(']') else {
            return malformed("an unclosed IPv6 literal");
        };
        if h.parse::<Ipv6Addr>().is_err() {
            return malformed("not an IPv6 literal");
        }
        let port = match tail {
            "" => None,
            p => match p.strip_prefix(':') {
                Some(p) => Some(p),
                None => return malformed("text after an IPv6 literal"),
            },
        };
        (h.to_ascii_lowercase(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_ascii_lowercase(), Some(p)),
            None => (authority.to_ascii_lowercase(), None),
        }
    };
    let host = host.strip_suffix('.').unwrap_or(&host).to_string();
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    {
        return malformed("not a host name or IP literal");
    }
    let port = match port {
        None => match scheme {
            Scheme::Http => 80,
            Scheme::Https => 443,
        },
        Some(p) => match p.parse::<u16>() {
            Ok(p) if p != 0 => p,
            _ => return malformed("not a port"),
        },
    };
    Ok((Origin { scheme, host, port }, path.to_string()))
}

/// The host's allowlist, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist(Vec<Origin>);

impl Allowlist {
    /// Every entry an origin (no path), none a wildcard or with user
    /// information; the list not empty.
    pub fn new(entries: &[String]) -> Result<Self, EndpointError> {
        if entries.is_empty() {
            return Err(EndpointError::EmptyAllowlist);
        }
        let mut out = vec![];
        for e in entries {
            let (o, path) = parse(e)?;
            if !(path.is_empty() || path == "/") {
                return malformed("an allowlist entry is an origin, without a path");
            }
            out.push(o);
        }
        Ok(Allowlist(out))
    }

    /// Check one destination: in the list, and plaintext only to a
    /// loopback IP literal.
    pub fn check(&self, url: &str) -> Result<Origin, EndpointError> {
        let (o, _) = parse(url)?;
        if !self.0.contains(&o) {
            return Err(EndpointError::EndpointNotAllowed {
                origin: o.to_string(),
            });
        }
        if o.scheme == Scheme::Http && !o.is_loopback_literal() {
            return Err(EndpointError::PlaintextNotLoopback {
                origin: o.to_string(),
            });
        }
        Ok(o)
    }

    /// Check `endpoint` and every `OTEL_*` endpoint variable `env` returns:
    /// an environment-supplied destination outside the list is refused
    /// like a configured one (I-3).
    pub fn check_all(
        &self,
        endpoint: &str,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Origin, EndpointError> {
        let o = self.check(endpoint)?;
        for var in ENDPOINT_VARS {
            if let Some(v) = env(var).filter(|v| !v.trim().is_empty()) {
                self.check(&v)?;
            }
        }
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(e: &[&str]) -> Allowlist {
        Allowlist::new(&e.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn origins_normalize() {
        let (o, path) = parse("HTTPS://Collector.Example.:443/v1/traces").unwrap();
        assert_eq!(o.to_string(), "https://collector.example:443");
        assert_eq!(path, "/v1/traces");
        assert_eq!(parse("http://127.0.0.1").unwrap().0.port, 80);
        assert_eq!(
            parse("http://[::1]:4317").unwrap().0.to_string(),
            "http://[::1]:4317"
        );
    }

    #[test]
    fn the_allowlist_admits_only_exact_origins() {
        let l = list(&["http://127.0.0.1:4318", "https://collector.example"]);
        assert!(l.check("http://127.0.0.1:4318/v1/traces").is_ok());
        assert!(l.check("https://COLLECTOR.example:443").is_ok());
        assert_eq!(
            l.check("http://127.0.0.1:4317"),
            Err(EndpointError::EndpointNotAllowed {
                origin: "http://127.0.0.1:4317".into()
            })
        );
        assert!(l.check("https://collector.example:8443").is_err());
        assert!(l.check("http://collector.example:443").is_err());
    }

    #[test]
    fn refusals() {
        assert_eq!(Allowlist::new(&[]), Err(EndpointError::EmptyAllowlist));
        assert_eq!(
            Allowlist::new(&["https://*.example".into()]),
            Err(EndpointError::Wildcard)
        );
        assert!(Allowlist::new(&["https://collector.example/v1".into()]).is_err());
        let l = list(&[
            "http://collector.example:4318",
            "http://localhost:4318",
            "http://127.0.0.1:4318",
        ]);
        assert!(matches!(
            l.check("http://collector.example:4318"),
            Err(EndpointError::PlaintextNotLoopback { .. })
        ));
        assert!(matches!(
            l.check("http://localhost:4318"),
            Err(EndpointError::PlaintextNotLoopback { .. })
        ));
        assert_eq!(
            l.check("http://user:secret@127.0.0.1:4318"),
            Err(EndpointError::UserInfo)
        );
        for bad in [
            "127.0.0.1:4318",
            "ftp://127.0.0.1",
            "http://",
            "http://h:x",
            "http://h:0",
        ] {
            assert!(l.check(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_environment_destination_outside_the_list_is_refused() {
        let l = list(&["http://127.0.0.1:4318"]);
        let env = |v: &str| {
            (v == "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT").then(|| "http://127.0.0.1:9".to_string())
        };
        assert_eq!(
            l.check_all("http://127.0.0.1:4318", env),
            Err(EndpointError::EndpointNotAllowed {
                origin: "http://127.0.0.1:9".into()
            })
        );
        assert!(l.check_all("http://127.0.0.1:4318", |_| None).is_ok());
    }
}
