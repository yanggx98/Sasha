//! Streaming regex matching over received bytes, shared by `send --expect` and
//! `monitor --until`.

use super::exit::CliError;

/// Most bytes kept while waiting for a match; older bytes are dropped first.
pub const MATCH_BUFFER_MAX: usize = 1024 * 1024;

pub struct ExpectMatcher {
    re: regex::bytes::Regex,
    buf: Vec<u8>,
}

impl ExpectMatcher {
    /// A pattern that is not a valid regex is a usage error.
    pub fn new(pattern: &str) -> Result<Self, CliError> {
        let re = regex::bytes::Regex::new(pattern)
            .map_err(|err| CliError::usage(format!("invalid --expect/--until pattern: {err}")))?;
        Ok(Self {
            re,
            buf: Vec::new(),
        })
    }

    /// Appends `chunk`; once the pattern matches, returns the bytes from the start of the
    /// buffer through the end of the match and keeps whatever follows for later matches.
    pub fn push(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > MATCH_BUFFER_MAX {
            let excess = self.buf.len() - MATCH_BUFFER_MAX;
            self.buf.drain(..excess);
        }
        let end = self.re.find(&self.buf)?.end();
        Some(self.buf.drain(..end).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::exit::CliExit;

    #[test]
    fn invalid_regex_is_a_usage_error() {
        let err = ExpectMatcher::new("(").err().expect("must fail");
        assert_eq!(err.exit, CliExit::Usage);
    }

    #[test]
    fn matches_across_chunks_and_returns_through_match_end() {
        let mut matcher = ExpectMatcher::new("READY").unwrap();
        assert_eq!(matcher.push(b"OK"), None);
        assert_eq!(matcher.push(b"\r\nREA"), None);
        assert_eq!(matcher.push(b"DY\r\n"), Some(b"OK\r\nREADY".to_vec()));
        // The trailing CRLF stays buffered for the next match.
        assert_eq!(matcher.push(b"READY"), Some(b"\r\nREADY".to_vec()));
    }

    #[test]
    fn handles_non_utf8_bytes() {
        let mut matcher = ExpectMatcher::new("OK").unwrap();
        assert_eq!(
            matcher.push(&[0xFF, b'O', b'K']),
            Some(vec![0xFF, b'O', b'K'])
        );
    }

    #[test]
    fn buffer_is_bounded() {
        let mut matcher = ExpectMatcher::new("OK").unwrap();
        let junk = vec![b'x'; 512 * 1024];
        for _ in 0..4 {
            assert_eq!(matcher.push(&junk), None);
        }
        assert!(matcher.buf.len() <= MATCH_BUFFER_MAX);
        assert!(matcher.push(b"OK").is_some());
    }
}
