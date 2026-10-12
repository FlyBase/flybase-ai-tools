//! Job ids: 16 random bytes as unpadded base64url (22 characters of `[A-Za-z0-9_-]`). `JobId::parse` is
//! the only way a request string becomes a `JobId`, and so the only way a request reaches a path.
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::fmt;

/// Shortest and longest id `parse` accepts (the MCP's own id check uses the same bounds).
pub const MIN_LEN: usize = 8;
pub const MAX_LEN: usize = 64;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(String);

impl JobId {
    /// A fresh unguessable id from the thread-local CSPRNG.
    #[allow(clippy::new_without_default)]
    pub fn new() -> JobId {
        let bytes: [u8; 16] = rand::random();
        JobId(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Accepts exactly `[A-Za-z0-9_-]{8,64}`; anything else (including `.`, `/`, `%`, and non-ASCII)
    /// is `None`, so a parsed id is always a single safe path component.
    pub fn parse(s: &str) -> Option<JobId> {
        let ok = (MIN_LEN..=MAX_LEN).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        ok.then(|| JobId(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JobId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn new_ids_have_the_documented_shape_and_parse() {
        for _ in 0..200 {
            let id = JobId::new();
            assert_eq!(id.as_str().len(), 22, "{id}");
            assert!(
                id.as_str()
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                "{id}"
            );
            assert_eq!(JobId::parse(id.as_str()), Some(id));
        }
    }

    #[test]
    fn new_ids_are_unique() {
        let ids: HashSet<String> = (0..10_000).map(|_| JobId::new().to_string()).collect();
        assert_eq!(ids.len(), 10_000);
    }

    #[test]
    fn parse_bounds_and_charset() {
        assert!(JobId::parse("abcdefgh").is_some());
        assert!(JobId::parse("A-b_9-Z_").is_some());
        assert!(JobId::parse(&"a".repeat(64)).is_some());
        assert!(JobId::parse("abcdefg").is_none(), "7 chars");
        assert!(JobId::parse(&"a".repeat(65)).is_none(), "65 chars");
        assert!(JobId::parse("").is_none());
        for bad in [
            "../../etc/passwd",
            "..abcdefgh",
            "abcdefgh/..",
            "abc/defgh",
            "abc\\defgh",
            "abcdefgh.",
            "abcd%2e%2e",
            "abcdefgh\0",
            "abcd efgh",
            "abcdéfghi",
            "abcdefgh\n",
            "=abcdefgh",
        ] {
            assert!(JobId::parse(bad).is_none(), "{bad:?} must be rejected");
        }
    }
}
