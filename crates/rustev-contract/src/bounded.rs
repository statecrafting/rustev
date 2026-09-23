//! Bounded JSON: resource limits checked on raw bytes before typed parsing.
//!
//! [`scan`] validates that a byte slice is exactly one RFC 8259 JSON value and
//! that it stays within every [`ParseLimits`] bound, without recursion and
//! without building any value. [`parse_bounded`] runs typed deserialization
//! only after the scan accepts.
//!
//! Reading bytes from a socket, file or other transport is the caller's
//! responsibility: this module judges a slice already in memory, so the
//! transport must bound its own buffering (for example at `max_bytes`).

use std::collections::HashSet;
use std::fmt;

/// Resource bounds for one JSON document. There is no default: every caller
/// states its limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum total input length in bytes, checked before any content is read.
    pub max_bytes: usize,
    /// Maximum container nesting; a top-level array or object is depth 1.
    pub max_depth: usize,
    /// Maximum raw (still escaped) byte length between a string's quotes.
    /// Applies to object keys and string values alike.
    pub max_string_bytes: usize,
    /// Maximum members of any one array or object.
    pub max_collection_len: usize,
    /// Maximum values in the document. Every scalar, array and object counts
    /// one; object keys do not count.
    pub max_total_values: usize,
    /// Maximum raw byte length of a number literal.
    pub max_number_bytes: usize,
    /// When false, a number with a fraction or exponent part is refused.
    pub allow_fractional_numbers: bool,
}

/// What an accepted scan observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanStats {
    /// Input length in bytes.
    pub bytes: usize,
    /// Deepest container nesting reached (0 for a top-level scalar).
    pub max_depth_seen: usize,
    /// Values counted under [`ParseLimits::max_total_values`].
    pub values: usize,
}

/// Why a scan refused its input. Offsets are byte offsets into the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundError {
    /// The input is longer than `max_bytes`. Reported before reading content.
    TooLarge { limit: usize, actual: usize },
    /// A container would nest deeper than `max_depth`.
    TooDeep { limit: usize, offset: usize },
    /// A string's raw length exceeds `max_string_bytes`.
    StringTooLong { limit: usize, offset: usize },
    /// An array or object has more than `max_collection_len` members.
    CollectionTooLong { limit: usize, offset: usize },
    /// The document has more than `max_total_values` values.
    TooManyValues { limit: usize, offset: usize },
    /// A number literal is longer than `max_number_bytes`.
    NumberTooLong { limit: usize, offset: usize },
    /// A fraction or exponent where `allow_fractional_numbers` is false.
    FractionalNumber { offset: usize },
    /// A key repeated within one object, compared after decoding escapes.
    /// `key` is truncated to 64 characters.
    DuplicateKey { key: String, offset: usize },
    /// The input is not JSON at `offset`; `expected` names what was expected.
    Syntax {
        offset: usize,
        expected: &'static str,
    },
    /// A string contains bytes that are not valid UTF-8.
    InvalidUtf8 { offset: usize },
    /// Non-whitespace follows the single top-level value.
    TrailingData { offset: usize },
    /// The input is empty or whitespace only.
    Empty,
}

impl fmt::Display for BoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use BoundError::*;
        match self {
            TooLarge { limit, actual } => {
                write!(f, "input is {actual} bytes, limit {limit}")
            }
            TooDeep { limit, offset } => {
                write!(f, "nesting exceeds depth {limit} at byte {offset}")
            }
            StringTooLong { limit, offset } => {
                write!(f, "string exceeds {limit} bytes at byte {offset}")
            }
            CollectionTooLong { limit, offset } => {
                write!(f, "collection exceeds {limit} members at byte {offset}")
            }
            TooManyValues { limit, offset } => {
                write!(f, "document exceeds {limit} values at byte {offset}")
            }
            NumberTooLong { limit, offset } => {
                write!(f, "number exceeds {limit} bytes at byte {offset}")
            }
            FractionalNumber { offset } => {
                write!(f, "fractional number not allowed at byte {offset}")
            }
            DuplicateKey { key, offset } => {
                write!(f, "duplicate key {key:?} at byte {offset}")
            }
            Syntax { offset, expected } => {
                write!(f, "expected {expected} at byte {offset}")
            }
            InvalidUtf8 { offset } => write!(f, "invalid UTF-8 at byte {offset}"),
            TrailingData { offset } => write!(f, "trailing data at byte {offset}"),
            Empty => write!(f, "empty input"),
        }
    }
}

impl std::error::Error for BoundError {}

/// Why [`parse_bounded`] produced no value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The bounded scan refused the input; no typed value was built.
    Bound(BoundError),
    /// The input was within bounds but does not match the target type
    /// (including unknown fields where the type denies them).
    Schema { message: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Bound(e) => write!(f, "bounds: {e}"),
            ParseError::Schema { message } => write!(f, "schema: {message}"),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::Bound(e) => Some(e),
            ParseError::Schema { .. } => None,
        }
    }
}

impl From<BoundError> for ParseError {
    fn from(e: BoundError) -> Self {
        ParseError::Bound(e)
    }
}

/// Scan `bytes`, then deserialize them into `T`.
///
/// Allocation discipline: the scan allocates only its container stack (at
/// most `max_depth` frames) and one key set per open object (their total size
/// is bounded by the input length, itself bounded by `max_bytes`). Typed
/// deserialization runs only after the scan accepts, so its allocations are
/// bounded by the accepted input. Reading the bytes from any transport is the
/// caller's job and must be bounded separately.
///
/// `serde_json` applies its own recursion limit of 128; a `max_depth` above
/// that can still yield a [`ParseError::Schema`] for deep input.
pub fn parse_bounded<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    limits: &ParseLimits,
) -> Result<T, ParseError> {
    scan(bytes, limits)?;
    serde_json::from_slice(bytes).map_err(|e| ParseError::Schema {
        message: e.to_string(),
    })
}

/// Validate `bytes` as exactly one JSON value within `limits`.
///
/// The length check against `max_bytes` happens before any byte is examined.
/// Every other bound is checked incrementally, at the byte where it is first
/// exceeded. The scan is iterative, so hostile nesting cannot exhaust the
/// stack.
pub fn scan(bytes: &[u8], limits: &ParseLimits) -> Result<ScanStats, BoundError> {
    if bytes.len() > limits.max_bytes {
        return Err(BoundError::TooLarge {
            limit: limits.max_bytes,
            actual: bytes.len(),
        });
    }
    Scanner {
        b: bytes,
        pos: 0,
        limits,
        stack: Vec::new(),
        values: 0,
        max_depth_seen: 0,
    }
    .run()
}

enum Frame {
    Array { len: usize },
    Object { len: usize, keys: HashSet<String> },
}

struct Scanner<'a> {
    b: &'a [u8],
    pos: usize,
    limits: &'a ParseLimits,
    stack: Vec<Frame>,
    values: usize,
    max_depth_seen: usize,
}

enum Step {
    Value,
    AfterValue,
}

fn syntax(offset: usize, expected: &'static str) -> BoundError {
    BoundError::Syntax { offset, expected }
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    fn run(mut self) -> Result<ScanStats, BoundError> {
        self.skip_ws();
        if self.pos == self.b.len() {
            return Err(BoundError::Empty);
        }
        let mut step = Step::Value;
        loop {
            step = match step {
                Step::Value => self.value()?,
                Step::AfterValue => {
                    self.skip_ws();
                    match self.stack.last() {
                        None => {
                            if self.pos != self.b.len() {
                                return Err(BoundError::TrailingData { offset: self.pos });
                            }
                            return Ok(ScanStats {
                                bytes: self.b.len(),
                                max_depth_seen: self.max_depth_seen,
                                values: self.values,
                            });
                        }
                        Some(Frame::Array { .. }) => match self.peek() {
                            Some(b',') => {
                                self.pos += 1;
                                self.skip_ws();
                                self.begin_member()?;
                                Step::Value
                            }
                            Some(b']') => {
                                self.pos += 1;
                                self.stack.pop();
                                Step::AfterValue
                            }
                            _ => return Err(syntax(self.pos, "',' or ']'")),
                        },
                        Some(Frame::Object { .. }) => match self.peek() {
                            Some(b',') => {
                                self.pos += 1;
                                self.skip_ws();
                                self.key()?;
                                Step::Value
                            }
                            Some(b'}') => {
                                self.pos += 1;
                                self.stack.pop();
                                Step::AfterValue
                            }
                            _ => return Err(syntax(self.pos, "',' or '}'")),
                        },
                    }
                }
            };
        }
    }

    /// Count a new member of the innermost container.
    fn begin_member(&mut self) -> Result<(), BoundError> {
        let limit = self.limits.max_collection_len;
        let offset = self.pos;
        let len = match self.stack.last_mut() {
            Some(Frame::Array { len }) | Some(Frame::Object { len, .. }) => len,
            None => return Ok(()),
        };
        *len += 1;
        if *len > limit {
            return Err(BoundError::CollectionTooLong { limit, offset });
        }
        Ok(())
    }

    fn count_value(&mut self) -> Result<(), BoundError> {
        self.values += 1;
        if self.values > self.limits.max_total_values {
            return Err(BoundError::TooManyValues {
                limit: self.limits.max_total_values,
                offset: self.pos,
            });
        }
        Ok(())
    }

    fn push(&mut self, frame: Frame) -> Result<(), BoundError> {
        if self.stack.len() >= self.limits.max_depth {
            return Err(BoundError::TooDeep {
                limit: self.limits.max_depth,
                offset: self.pos,
            });
        }
        self.stack.push(frame);
        self.max_depth_seen = self.max_depth_seen.max(self.stack.len());
        Ok(())
    }

    /// Parse an object key and its colon. Positioned at the opening quote.
    fn key(&mut self) -> Result<(), BoundError> {
        if self.peek() != Some(b'"') {
            return Err(syntax(self.pos, "object key"));
        }
        self.begin_member()?;
        let offset = self.pos;
        let key = self.string(true)?.unwrap_or_default();
        if let Some(Frame::Object { keys, .. }) = self.stack.last_mut() {
            if keys.contains(&key) {
                return Err(BoundError::DuplicateKey {
                    key: key.chars().take(64).collect(),
                    offset,
                });
            }
            keys.insert(key);
        }
        self.skip_ws();
        if self.peek() != Some(b':') {
            return Err(syntax(self.pos, "':'"));
        }
        self.pos += 1;
        self.skip_ws();
        Ok(())
    }

    /// Parse the start of one value. Positioned at its first byte.
    fn value(&mut self) -> Result<Step, BoundError> {
        let Some(c) = self.peek() else {
            return Err(syntax(self.pos, "value"));
        };
        match c {
            b'{' => {
                self.count_value()?;
                self.push(Frame::Object {
                    len: 0,
                    keys: HashSet::new(),
                })?;
                self.pos += 1;
                self.skip_ws();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    self.stack.pop();
                    return Ok(Step::AfterValue);
                }
                self.key()?;
                Ok(Step::Value)
            }
            b'[' => {
                self.count_value()?;
                self.push(Frame::Array { len: 0 })?;
                self.pos += 1;
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    self.stack.pop();
                    return Ok(Step::AfterValue);
                }
                self.begin_member()?;
                Ok(Step::Value)
            }
            b'"' => {
                self.count_value()?;
                self.string(false)?;
                Ok(Step::AfterValue)
            }
            b'-' | b'0'..=b'9' => {
                self.count_value()?;
                self.number()?;
                Ok(Step::AfterValue)
            }
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            _ => Err(syntax(self.pos, "value")),
        }
    }

    fn literal(&mut self, word: &'static [u8]) -> Result<Step, BoundError> {
        self.count_value()?;
        if self.b[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(Step::AfterValue)
        } else {
            Err(syntax(self.pos, "literal true, false or null"))
        }
    }

    fn digits(&mut self, start: usize) -> Result<usize, BoundError> {
        let mut n = 0;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
            n += 1;
            self.check_number_len(start)?;
        }
        Ok(n)
    }

    fn check_number_len(&self, start: usize) -> Result<(), BoundError> {
        if self.pos - start > self.limits.max_number_bytes {
            return Err(BoundError::NumberTooLong {
                limit: self.limits.max_number_bytes,
                offset: start,
            });
        }
        Ok(())
    }

    fn number(&mut self) -> Result<(), BoundError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
            self.check_number_len(start)?;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                self.check_number_len(start)?;
            }
            Some(b'1'..=b'9') => {
                self.digits(start)?;
            }
            _ => return Err(syntax(self.pos, "digit")),
        }
        if self.peek() == Some(b'.') {
            if !self.limits.allow_fractional_numbers {
                return Err(BoundError::FractionalNumber { offset: self.pos });
            }
            self.pos += 1;
            self.check_number_len(start)?;
            if self.digits(start)? == 0 {
                return Err(syntax(self.pos, "digit"));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            if !self.limits.allow_fractional_numbers {
                return Err(BoundError::FractionalNumber { offset: self.pos });
            }
            self.pos += 1;
            self.check_number_len(start)?;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
                self.check_number_len(start)?;
            }
            if self.digits(start)? == 0 {
                return Err(syntax(self.pos, "digit"));
            }
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, BoundError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let d = match self.peek() {
                Some(c @ b'0'..=b'9') => c - b'0',
                Some(c @ b'a'..=b'f') => c - b'a' + 10,
                Some(c @ b'A'..=b'F') => c - b'A' + 10,
                _ => return Err(syntax(self.pos, "hex digit")),
            };
            v = v * 16 + u32::from(d);
            self.pos += 1;
        }
        Ok(v)
    }

    /// Scan a string positioned at its opening quote. Returns the decoded
    /// text when `decode` is true, and allocates nothing otherwise.
    fn string(&mut self, decode: bool) -> Result<Option<String>, BoundError> {
        let open = self.pos;
        self.pos += 1;
        let start = self.pos;
        let limit = self.limits.max_string_bytes;
        let mut out = decode.then(String::new);
        loop {
            if self.pos - start > limit {
                return Err(BoundError::StringTooLong {
                    limit,
                    offset: open,
                });
            }
            let Some(c) = self.peek() else {
                return Err(syntax(self.pos, "closing '\"'"));
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let esc = self.peek();
                    let ch = match esc {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            let at = self.pos - 1;
                            self.pos += 1;
                            let hi = self.hex4()?;
                            let cp = match hi {
                                0xD800..=0xDBFF => {
                                    if !self.b[self.pos..].starts_with(b"\\u") {
                                        return Err(syntax(at, "valid surrogate pair"));
                                    }
                                    self.pos += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&lo) {
                                        return Err(syntax(at, "valid surrogate pair"));
                                    }
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                }
                                0xDC00..=0xDFFF => {
                                    return Err(syntax(at, "valid surrogate pair"));
                                }
                                v => v,
                            };
                            if let Some(s) = out.as_mut() {
                                // Surrogates are excluded above, so this is a scalar value.
                                s.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                            }
                            continue;
                        }
                        _ => return Err(syntax(self.pos, "escape character")),
                    };
                    self.pos += 1;
                    if let Some(s) = out.as_mut() {
                        s.push(ch);
                    }
                }
                0x00..=0x1F => {
                    return Err(syntax(self.pos, "escaped control character"));
                }
                0x20..=0x7F => {
                    self.pos += 1;
                    if let Some(s) = out.as_mut() {
                        s.push(char::from(c));
                    }
                }
                _ => {
                    let width = match c {
                        0xC2..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF4 => 4,
                        _ => return Err(BoundError::InvalidUtf8 { offset: self.pos }),
                    };
                    let end = self.pos + width;
                    let Some(span) = self.b.get(self.pos..end) else {
                        return Err(BoundError::InvalidUtf8 { offset: self.pos });
                    };
                    let Ok(text) = std::str::from_utf8(span) else {
                        return Err(BoundError::InvalidUtf8 { offset: self.pos });
                    };
                    if let Some(s) = out.as_mut() {
                        s.push_str(text);
                    }
                    self.pos = end;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: ParseLimits = ParseLimits {
        max_bytes: 1 << 20,
        max_depth: 64,
        max_string_bytes: 1 << 16,
        max_collection_len: 1 << 16,
        max_total_values: 1 << 20,
        max_number_bytes: 64,
        allow_fractional_numbers: true,
    };

    fn ok(s: &str, l: &ParseLimits) -> ScanStats {
        scan(s.as_bytes(), l).unwrap_or_else(|e| panic!("{s:?}: {e}"))
    }

    fn err(s: &str, l: &ParseLimits) -> BoundError {
        scan(s.as_bytes(), l).expect_err(s)
    }

    #[test]
    fn accepts_valid_documents() {
        for s in [
            "0",
            "-0",
            "1.5e-3",
            "-12E+4",
            "true",
            "false",
            "null",
            "\"\"",
            "\"a\\n\\u00e9\\ud83d\\ude00\"",
            "\"é😀\"",
            " [ ] ",
            "{}",
            "{\"a\":[1,{\"b\":null}],\"c\":\"d\"}",
            "\r\n\t[1 , 2]\n",
        ] {
            ok(s, &WIDE);
        }
    }

    #[test]
    fn stats() {
        let s = ok("{\"a\":[1,2,{}],\"b\":\"x\"}", &WIDE);
        assert_eq!(s.max_depth_seen, 3);
        // object, array, 1, 2, {}, "x"
        assert_eq!(s.values, 6);
        assert_eq!(ok("7", &WIDE).max_depth_seen, 0);
    }

    #[test]
    fn syntax_errors() {
        for s in [
            "[1,]",
            "[1 2]",
            "{\"a\" 1}",
            "{\"a\":}",
            "{1:2}",
            "{\"a\":1,}",
            "tru",
            "nul",
            "01x",
            "[01]",
            "-",
            "1.",
            "1e",
            "1e+",
            ".5",
            "+1",
            "\"abc",
            "\"\\x\"",
            "\"\\u12G4\"",
            "[",
            "{",
            "'a'",
            "\"a\tb\"",
            "\"\u{1}\"",
        ] {
            let e = err(s, &WIDE);
            assert!(
                matches!(
                    e,
                    BoundError::Syntax { .. } | BoundError::TrailingData { .. }
                ),
                "{s:?}: {e:?}"
            );
        }
    }

    #[test]
    fn empty_and_trailing() {
        assert_eq!(err("", &WIDE), BoundError::Empty);
        assert_eq!(err(" \n\t ", &WIDE), BoundError::Empty);
        assert_eq!(err("1 2", &WIDE), BoundError::TrailingData { offset: 2 });
        assert_eq!(err("{} {}", &WIDE), BoundError::TrailingData { offset: 3 });
        assert_eq!(err("01", &WIDE), BoundError::TrailingData { offset: 1 });
    }

    #[test]
    fn too_large_checked_first() {
        let l = ParseLimits {
            max_bytes: 4,
            ..WIDE
        };
        ok("[12]", &l);
        // Invalid content but over the limit: size is reported, not syntax.
        assert_eq!(
            err("#####", &l),
            BoundError::TooLarge {
                limit: 4,
                actual: 5
            }
        );
        let zero = ParseLimits {
            max_bytes: 0,
            ..WIDE
        };
        assert_eq!(err("", &zero), BoundError::Empty);
    }

    #[test]
    fn depth_limit() {
        let l = ParseLimits {
            max_depth: 3,
            ..WIDE
        };
        assert_eq!(ok("[[[1]]]", &l).max_depth_seen, 3);
        assert_eq!(ok("{\"a\":{\"b\":[]}}", &l).max_depth_seen, 3);
        assert_eq!(
            err("[[[[1]]]]", &l),
            BoundError::TooDeep {
                limit: 3,
                offset: 3
            }
        );
        let flat = ParseLimits {
            max_depth: 0,
            ..WIDE
        };
        ok("\"scalar\"", &flat);
        assert!(matches!(err("[]", &flat), BoundError::TooDeep { .. }));
    }

    #[test]
    fn deep_nesting_is_refused_without_recursion() {
        let n = 10_000;
        let s = "[".repeat(n) + &"]".repeat(n);
        assert!(matches!(
            err(&s, &WIDE),
            BoundError::TooDeep { limit: 64, .. }
        ));
        let deep = ParseLimits {
            max_depth: n,
            ..WIDE
        };
        assert_eq!(ok(&s, &deep).max_depth_seen, n);
        let obj = "{\"a\":".repeat(n) + "1" + &"}".repeat(n);
        assert_eq!(ok(&obj, &deep).max_depth_seen, n);
    }

    #[test]
    fn string_limit() {
        let l = ParseLimits {
            max_string_bytes: 3,
            ..WIDE
        };
        ok("\"abc\"", &l);
        ok("{\"abc\":\"xyz\"}", &l);
        // Escapes count by raw length: "\n" is 2 raw bytes.
        ok("\"a\\n\"", &l);
        assert_eq!(
            err("\"abcd\"", &l),
            BoundError::StringTooLong {
                limit: 3,
                offset: 0
            }
        );
        assert!(matches!(
            err("{\"abcd\":1}", &l),
            BoundError::StringTooLong { .. }
        ));
        assert!(matches!(
            err("\"\\u0041\"", &l),
            BoundError::StringTooLong { .. }
        ));
        // Multi-byte characters count their bytes: "é" is 2 bytes, "éé" is 4.
        ok("\"é\"", &l);
        assert!(matches!(
            err("\"éé\"", &l),
            BoundError::StringTooLong { .. }
        ));
    }

    #[test]
    fn string_limit_fires_before_end_of_input() {
        let l = ParseLimits {
            max_string_bytes: 3,
            ..WIDE
        };
        // Unterminated: an unbounded scan would report a syntax error at the end.
        assert!(matches!(
            err("\"abcdefgh", &l),
            BoundError::StringTooLong { .. }
        ));
    }

    #[test]
    fn collection_limit() {
        let l = ParseLimits {
            max_collection_len: 2,
            ..WIDE
        };
        ok("[1,2]", &l);
        ok("{\"a\":1,\"b\":2}", &l);
        assert_eq!(
            err("[1,2,3]", &l),
            BoundError::CollectionTooLong {
                limit: 2,
                offset: 5
            }
        );
        assert!(matches!(
            err("{\"a\":1,\"b\":2,\"c\":3}", &l),
            BoundError::CollectionTooLong { .. }
        ));
        // Incremental: refused before the malformed tail is reached.
        assert!(matches!(
            err("[1,2,3,#", &l),
            BoundError::CollectionTooLong { .. }
        ));
    }

    #[test]
    fn total_value_limit() {
        let l = ParseLimits {
            max_total_values: 3,
            ..WIDE
        };
        ok("[1,2]", &l);
        ok("{\"a\":1,\"b\":2}", &l);
        assert_eq!(
            err("[1,2,3]", &l),
            BoundError::TooManyValues {
                limit: 3,
                offset: 5
            }
        );
        assert!(matches!(
            err("[[],[],[]]", &l),
            BoundError::TooManyValues { .. }
        ));
    }

    #[test]
    fn number_limit() {
        let l = ParseLimits {
            max_number_bytes: 4,
            ..WIDE
        };
        ok("1234", &l);
        ok("-123", &l);
        ok("1.25", &l);
        assert_eq!(
            err("12345", &l),
            BoundError::NumberTooLong {
                limit: 4,
                offset: 0
            }
        );
        assert!(matches!(
            err("[-1234]", &l),
            BoundError::NumberTooLong { offset: 1, .. }
        ));
        assert!(matches!(err("1.234", &l), BoundError::NumberTooLong { .. }));
        assert!(matches!(err("1e+10", &l), BoundError::NumberTooLong { .. }));
    }

    #[test]
    fn fractional_numbers() {
        let l = ParseLimits {
            allow_fractional_numbers: false,
            ..WIDE
        };
        ok("-42", &l);
        ok("[0,1,2]", &l);
        assert_eq!(err("1.5", &l), BoundError::FractionalNumber { offset: 1 });
        assert_eq!(err("[1e3]", &l), BoundError::FractionalNumber { offset: 2 });
        assert_eq!(err("2E-1", &l), BoundError::FractionalNumber { offset: 1 });
        ok("1.5", &WIDE);
        ok("1e3", &WIDE);
    }

    #[test]
    fn duplicate_keys() {
        assert_eq!(
            err("{\"a\":1,\"a\":2}", &WIDE),
            BoundError::DuplicateKey {
                key: "a".into(),
                offset: 7
            }
        );
        // Compared after decoding escapes.
        assert!(matches!(
            err("{\"a\":1,\"\\u0061\":2}", &WIDE),
            BoundError::DuplicateKey { .. }
        ));
        assert!(matches!(
            err("{\"\\/\":1,\"/\":2}", &WIDE),
            BoundError::DuplicateKey { .. }
        ));
        // Same key in sibling and nested objects is fine.
        ok("{\"a\":{\"a\":1},\"b\":[{\"a\":1},{\"a\":2}]}", &WIDE);
        // Duplicate only in an inner object is still refused.
        assert!(matches!(
            err("{\"x\":{\"a\":1,\"a\":2}}", &WIDE),
            BoundError::DuplicateKey { .. }
        ));
        // Reported key is truncated to 64 characters.
        let long = "k".repeat(100);
        let s = format!("{{\"{long}\":1,\"{long}\":2}}");
        match err(&s, &WIDE) {
            BoundError::DuplicateKey { key, .. } => assert_eq!(key.chars().count(), 64),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn surrogates() {
        ok("\"\\ud83d\\ude00\"", &WIDE);
        ok("\"\\uD83D\\uDE00\"", &WIDE);
        for s in [
            "\"\\ud83d\"",
            "\"\\ud83dx\"",
            "\"\\ud83d\\u0041\"",
            "\"\\ude00\"",
            "\"\\ude00\\ud83d\"",
            "\"\\ud83d\\n\"",
        ] {
            assert!(matches!(err(s, &WIDE), BoundError::Syntax { .. }), "{s}");
        }
        // Decoded pair participates in duplicate detection.
        assert!(matches!(
            err("{\"😀\":1,\"\\ud83d\\ude00\":2}", &WIDE),
            BoundError::DuplicateKey { .. }
        ));
    }

    #[test]
    fn invalid_utf8() {
        let cases: [&[u8]; 6] = [
            b"\"\xff\"",
            b"\"\xc0\xaf\"",
            b"\"\xe0\x80\xaf\"",
            b"\"\xed\xa0\x80\"",
            b"\"\xf5\x80\x80\x80\"",
            b"\"\xc3",
        ];
        for c in cases {
            assert_eq!(
                scan(c, &WIDE),
                Err(BoundError::InvalidUtf8 { offset: 1 }),
                "{c:?}"
            );
        }
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Doc {
        name: String,
        n: u32,
    }

    #[test]
    fn parse_bounded_typed() {
        let d: Doc = parse_bounded(br#"{"name":"x","n":3}"#, &WIDE).unwrap();
        assert_eq!(
            d,
            Doc {
                name: "x".into(),
                n: 3
            }
        );
        let e = parse_bounded::<Doc>(br#"{"name":"x","n":3,"extra":1}"#, &WIDE).unwrap_err();
        assert!(matches!(e, ParseError::Schema { ref message } if message.contains("extra")));
        // serde_json alone would silently keep the last duplicate; the scan refuses.
        let e = parse_bounded::<Doc>(br#"{"name":"x","n":3,"n":4}"#, &WIDE).unwrap_err();
        assert!(matches!(
            e,
            ParseError::Bound(BoundError::DuplicateKey { .. })
        ));
        // A bound refusal happens before the typed parse is attempted.
        let l = ParseLimits {
            max_string_bytes: 2,
            ..WIDE
        };
        let e = parse_bounded::<Doc>(br#"{"name":"xyz","n":3}"#, &l).unwrap_err();
        assert!(matches!(
            e,
            ParseError::Bound(BoundError::StringTooLong { .. })
        ));
        let e = parse_bounded::<Doc>(
            br#"{"name":"x","n":3.5}"#,
            &ParseLimits {
                allow_fractional_numbers: false,
                ..WIDE
            },
        )
        .unwrap_err();
        assert!(matches!(
            e,
            ParseError::Bound(BoundError::FractionalNumber { .. })
        ));
        assert!(e.to_string().starts_with("bounds:"));
    }

    #[test]
    fn display_is_informative() {
        let e = BoundError::TooLarge {
            limit: 1,
            actual: 2,
        };
        assert_eq!(e.to_string(), "input is 2 bytes, limit 1");
    }
}
