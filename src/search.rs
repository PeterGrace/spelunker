//! Pattern matching against blob contents.

use crate::error::Result;

/// A single matching line within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 1-based line number, matching grep's convention.
    pub line_number: usize,
    /// The full text of the matching line (without the trailing newline).
    pub line: String,
}

/// What kind of match we're performing.
#[derive(Debug)]
pub enum Matcher {
    /// Match lines containing a fixed substring.
    Literal {
        /// The substring to search for.
        needle: String,
        /// If true, comparison is case-insensitive.
        ignore_case: bool,
    },
    /// Match lines using a compiled regular expression.
    Regex(regex::Regex),
}

impl Matcher {
    /// Construct a literal (fixed-string) matcher.
    ///
    /// # Arguments
    ///
    /// * `needle` - The substring to search for.
    /// * `ignore_case` - If true, the match ignores ASCII and Unicode case.
    pub fn literal(needle: impl Into<String>, ignore_case: bool) -> Self {
        Self::Literal {
            needle: needle.into(),
            ignore_case,
        }
    }

    /// Construct a regex matcher.
    ///
    /// # Arguments
    ///
    /// * `pattern` - The regex pattern string.
    /// * `ignore_case` - If true, wraps the pattern with `(?i)`.
    ///
    /// # Errors
    ///
    /// Returns `SpelunkerError::BadRegex` if the pattern is invalid.
    pub fn regex(pattern: &str, ignore_case: bool) -> Result<Self> {
        // Prepend the inline flag `(?i)` so the regex crate handles
        // case-folding — this avoids a separate code path in `scan`.
        let prefixed;
        let effective = if ignore_case {
            prefixed = format!("(?i){pattern}");
            prefixed.as_str()
        } else {
            pattern
        };
        Ok(Self::Regex(regex::Regex::new(effective)?))
    }

    /// Scan a byte slice and return every matching line.
    ///
    /// Non-UTF-8 bytes are replaced with the Unicode replacement character
    /// (`U+FFFD`) so that the scan never panics on binary content.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Raw file contents from git.
    ///
    /// # Returns
    ///
    /// A `Vec<Hit>` with one entry per matching line, in file order.
    pub fn scan(&self, bytes: &[u8]) -> Vec<Hit> {
        // `from_utf8_lossy` replaces invalid sequences with U+FFFD rather than
        // panicking — essential for binary or mixed-encoding blobs.
        let text = String::from_utf8_lossy(bytes);
        let mut hits = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            let matches = match self {
                Self::Literal {
                    needle,
                    ignore_case: true,
                } => line.to_lowercase().contains(&needle.to_lowercase()),
                Self::Literal {
                    needle,
                    ignore_case: false,
                } => line.contains(needle),
                Self::Regex(re) => re.is_match(line),
            };
            if matches {
                hits.push(Hit {
                    line_number: idx + 1,
                    line: line.to_string(),
                });
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_finds_substring_with_line_number() {
        let m = Matcher::literal("needle", false);
        let hits = m.scan(b"first line\nthis has needle\nthird line\n");
        assert_eq!(
            hits,
            vec![Hit {
                line_number: 2,
                line: "this has needle".to_string()
            }]
        );
    }

    #[test]
    fn literal_finds_multiple_hits() {
        let m = Matcher::literal("foo", false);
        let hits = m.scan(b"foo\nbar\nfoo\n");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line_number, 1);
        assert_eq!(hits[1].line_number, 3);
    }

    #[test]
    fn literal_no_match_returns_empty() {
        let m = Matcher::literal("absent", false);
        assert!(m.scan(b"nothing here\n").is_empty());
    }

    #[test]
    fn literal_case_sensitive_by_default() {
        let m = Matcher::literal("Needle", false);
        assert!(m.scan(b"a needle in a haystack\n").is_empty());
    }

    #[test]
    fn literal_empty_input_no_panic() {
        let m = Matcher::literal("anything", false);
        assert!(m.scan(b"").is_empty());
    }

    #[test]
    fn literal_ignore_case_matches_mixed_case() {
        let m = Matcher::literal("Needle", true);
        let hits = m.scan(b"a NEEDLE in a haystack\nno needle here either\n");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn literal_ignore_case_unicode_lowercasing() {
        let m = Matcher::literal("ÄPFEL", true);
        let hits = m.scan("ich mag äpfel\n".as_bytes());
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn regex_basic_match() {
        let m = Matcher::regex(r"foo\d+", false).expect("valid regex");
        let hits = m.scan(b"foo1\nbar\nfoo23\n");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line, "foo1");
        assert_eq!(hits[1].line, "foo23");
    }

    #[test]
    fn regex_invalid_pattern_errors() {
        let err = Matcher::regex("(", false).unwrap_err();
        assert!(matches!(err, crate::SpelunkerError::BadRegex(_)));
    }

    #[test]
    fn regex_ignore_case() {
        let m = Matcher::regex(r"HELLO", true).expect("valid regex");
        let hits = m.scan(b"hello world\nHello again\n");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn scan_lossy_decodes_invalid_utf8() {
        // 0xFF is invalid UTF-8; scan must not panic.
        let bytes = b"hello\xFFworld\nfoo\n";
        let m = Matcher::literal("world", false);
        let hits = m.scan(bytes);
        // First line decoded with replacement character still contains "world".
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line_number, 1);
    }
}
