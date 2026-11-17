//! Widening the desktop's content security policy for the test runs that point the app at Cash
//! Gear Workspace's mock (`CGS_WORKSPACE_API` under `CGS_DESKTOP_SMOKE`, Track S6). The shipped
//! policy is `tauri.conf.json`'s.

/// Whether `text` is exactly an origin: `http` or `https`, a host of letters, digits, dots and
/// hyphens (or `[::1]`), and an optional port. Nothing that could end a directive gets through.
pub fn is_origin(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let (host, port) = match rest.strip_prefix("[::1]") {
        Some(port) => ("[::1]", port),
        None => match rest.find(':') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, ""),
        },
    };
    let host_ok = host == "[::1]"
        || (!host.is_empty()
            && !host.starts_with(['.', '-'])
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-'));
    let port_ok = port.is_empty()
        || port.strip_prefix(':').is_some_and(|digits| {
            !digits.is_empty() && digits.len() <= 5 && digits.bytes().all(|b| b.is_ascii_digit())
        });
    host_ok && port_ok
}

/// `policy` with `origins` added to its `connect-src`, or `None` when one isn't an origin or the
/// policy has no `connect-src`.
pub fn widen_connect_src(policy: &str, origins: &[&str]) -> Option<String> {
    if !origins.iter().all(|origin| is_origin(origin)) {
        return None;
    }
    let mut found = false;
    let directives: Vec<String> = policy
        .split(';')
        .map(|directive| {
            let directive = directive.trim();
            if directive.split_whitespace().next() == Some("connect-src") {
                found = true;
                let mut widened = directive.to_owned();
                for origin in origins {
                    if !directive.split_whitespace().any(|source| source == *origin) {
                        widened.push(' ');
                        widened.push_str(origin);
                    }
                }
                widened
            } else {
                directive.to_owned()
            }
        })
        .filter(|directive| !directive.is_empty())
        .collect();
    found.then(|| directives.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_an_origin() {
        for origin in [
            "http://127.0.0.1:4280",
            "http://localhost:5280",
            "https://workspace-api.cashgearerp.com",
            "http://[::1]:4281",
            "https://acct.blob.core.windows.net",
        ] {
            assert!(is_origin(origin), "{origin}");
        }
        for text in [
            "",
            "127.0.0.1:4280",
            "ftp://host",
            "http://",
            "http://host/",
            "http://host/path",
            "http://host:",
            "http://host:123456",
            "http://host:12a",
            "http://host; script-src *",
            "http://host *",
            "http://.host",
            "http://user@host",
            "http://host?x",
            "HTTP://host",
            "http://[::2]:1",
        ] {
            assert!(!is_origin(text), "{text}");
        }
    }

    #[test]
    fn adds_origins_to_connect_src_only() {
        let policy = "default-src 'self'; connect-src 'self' ipc: https://api.cashgearerp.com; img-src 'self'";
        assert_eq!(
            widen_connect_src(
                policy,
                &[
                    "http://127.0.0.1:4280",
                    "http://127.0.0.1:4281",
                    "https://api.cashgearerp.com"
                ]
            )
            .as_deref(),
            Some(
                "default-src 'self'; connect-src 'self' ipc: https://api.cashgearerp.com \
                 http://127.0.0.1:4280 http://127.0.0.1:4281; img-src 'self'"
            )
        );
        assert_eq!(widen_connect_src(policy, &[]).as_deref(), Some(policy));
        assert_eq!(widen_connect_src(policy, &["http://x; script-src *"]), None);
        assert_eq!(widen_connect_src("default-src 'self'", &["http://x"]), None);
    }
}
