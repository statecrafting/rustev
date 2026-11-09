//! The text matchers of `rustev.extract-rules/2` (spec 070, 3.2 and 3.3):
//! byte and integer operations only, no regular expression, no Unicode
//! table, no locale data and no floating point, possessive with no
//! backtracking, so the work per source byte is the bound 3.6.2 states.
use rustev_contract::extraction_documents::{ByteClass, DateFormat, ShapeElement, printable_byte};

/// The earliest occurrence of any term, ties going to declared term order,
/// as `(offset, term length)` (spec 070, 3.2.1). ASCII `A` to `Z` fold to
/// lower case in both text and term; other bytes compare exactly.
pub(crate) fn first_term(text: &[u8], terms: &[String]) -> Option<(usize, usize)> {
    terms
        .iter()
        .filter_map(|t| {
            text.windows(t.len())
                .position(|w| w.eq_ignore_ascii_case(t.as_bytes()))
                .map(|i| (i, t.len()))
        })
        .enumerate()
        .min_by_key(|(order, (i, _))| (*i, *order))
        .map(|(_, found)| found)
}

/// Spec 070, 3.2.2: spaces and tabs, at most one of `:`, `#` and `-`, then
/// spaces and tabs. A line break ends the skip.
fn skip(text: &[u8], mut p: usize) -> usize {
    let blank = |b: &u8| *b == b' ' || *b == b'\t';
    p += text[p..].iter().take_while(|b| blank(b)).count();
    if matches!(text.get(p), Some(b':' | b'#' | b'-')) {
        p += 1;
    }
    p + text[p..].iter().take_while(|b| blank(b)).count()
}

/// The position after the term occurrence and the skip, if a term occurs.
fn after_term(text: &[u8], terms: &[String]) -> Option<usize> {
    first_term(text, terms).map(|(i, len)| skip(text, i + len))
}

/// Spec 070, 3.2.3: classes compare bytes exactly.
fn in_class(b: u8, classes: &[ByteClass]) -> bool {
    classes.iter().any(|c| match c {
        ByteClass::Upper => b.is_ascii_uppercase(),
        ByteClass::Lower => b.is_ascii_lowercase(),
        ByteClass::Digit => b.is_ascii_digit(),
        ByteClass::Range([lo, hi]) => match (printable_byte(lo), printable_byte(hi)) {
            (Some(lo), Some(hi)) => (lo..=hi).contains(&b),
            _ => false,
        },
    })
}

/// Spec 070, 3.2.4: bytes above 0x7F are neither letters nor digits.
fn start_boundary(text: &[u8], p: usize) -> bool {
    p == 0 || !text[p - 1].is_ascii_alphanumeric()
}
fn end_boundary(text: &[u8], p: usize) -> bool {
    p == text.len() || !text[p].is_ascii_alphanumeric()
}

/// Exactly the captured bytes, which are printable ASCII by construction.
fn token(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(*b)).collect()
}

/// `token_after` (spec 070, 3.3.1): the maximal run of `charset` bytes after
/// the term and the skip, when its length is within `[min_len, max_len]`.
/// A longer run is never cut short.
pub(crate) fn token_after(
    text: &[u8],
    terms: &[String],
    charset: &[ByteClass],
    min_len: u8,
    max_len: u8,
) -> Option<String> {
    let p = after_term(text, terms)?;
    let n = text[p..]
        .iter()
        .take_while(|b| in_class(**b, charset))
        .count();
    (usize::from(min_len)..=usize::from(max_len))
        .contains(&n)
        .then(|| token(&text[p..p + n]))
}

/// The end of a possessive shape match at `start`, if any (spec 070, 3.3.2).
fn shape_at(text: &[u8], start: usize, shape: &[ShapeElement]) -> Option<usize> {
    let mut p = start;
    for e in shape {
        match e {
            ShapeElement::Repeat { class, min, max } => {
                let n = text[p..]
                    .iter()
                    .take(usize::from(*max))
                    .take_while(|b| in_class(**b, class))
                    .count();
                if n < usize::from(*min) {
                    return None;
                }
                p += n;
            }
            ShapeElement::OptionalSpace => {
                if text.get(p) == Some(&b' ') {
                    p += 1;
                }
            }
            ShapeElement::Byte(b) => {
                if text.get(p).copied() != printable_byte(b) {
                    return None;
                }
                p += 1;
            }
        }
    }
    end_boundary(text, p).then_some(p)
}

/// The `occurrence`-th match, starts tried left to right at start
/// boundaries, a later match searched from the end of the earlier one.
fn nth_match<T>(
    text: &[u8],
    occurrence: u8,
    at: impl Fn(usize) -> Option<(usize, T)>,
) -> Option<(usize, usize, T)> {
    let mut seen = 0;
    let mut start = 0;
    while start < text.len() {
        if start_boundary(text, start)
            && let Some((end, v)) = at(start)
            && end > start
        {
            seen += 1;
            if seen == occurrence {
                return Some((start, end, v));
            }
            start = end;
            continue;
        }
        start += 1;
    }
    None
}

/// `token_matching` (spec 070, 3.3.2).
pub(crate) fn token_matching(
    text: &[u8],
    shape: &[ShapeElement],
    occurrence: u8,
) -> Option<String> {
    nth_match(text, occurrence, |s| {
        shape_at(text, s, shape).map(|e| (e, ()))
    })
    .map(|(s, e, ())| token(&text[s..e]))
}

/// A maximal run of exactly `len` digits at `p` when `len` is fixed, or of
/// 1 or 2 digits when it is `None` (a run of 3 or more fails), as its value
/// and end.
fn digits(text: &[u8], p: usize, len: Option<usize>) -> Option<(u32, usize)> {
    let run = text[p..].iter().take_while(|b| b.is_ascii_digit()).count();
    let n = match len {
        Some(len) if run >= len => len,
        None if (1..=2).contains(&run) => run,
        _ => return None,
    };
    let v = text[p..p + n]
        .iter()
        .fold(0u32, |v, b| v * 10 + u32::from(b - b'0'));
    Some((v, p + n))
}

fn byte(text: &[u8], p: usize, b: u8) -> Option<usize> {
    (text.get(p) == Some(&b)).then_some(p + 1)
}

/// A month name at `p` followed by a space or `.`, as its month number and
/// end (spec 070, 3.3.3). Names compare under ASCII folding.
fn month(text: &[u8], p: usize, months: &[Vec<String>]) -> Option<(u32, usize)> {
    months.iter().zip(1..).find_map(|(names, m)| {
        names.iter().find_map(|name| {
            let end = p + name.len();
            (text.get(p..end)?.eq_ignore_ascii_case(name.as_bytes())
                && matches!(text.get(end), Some(b' ' | b'.')))
            .then_some((m, end))
        })
    })
}

fn optional(text: &[u8], p: usize, b: u8) -> usize {
    byte(text, p, b).unwrap_or(p)
}

/// One date match at `p` as `(year, month, day)` and its end.
fn date_at(
    text: &[u8],
    p: usize,
    format: DateFormat,
    months: &[Vec<String>],
) -> Option<(usize, (u32, u32, u32))> {
    let numeric = |sep: u8, day_first: bool| {
        let (a, p) = digits(text, p, None)?;
        let p = byte(text, p, sep)?;
        let (b, p) = digits(text, p, None)?;
        let p = byte(text, p, sep)?;
        let (y, p) = digits(text, p, Some(4))?;
        let (d, m) = if day_first { (a, b) } else { (b, a) };
        Some((p, (y, m, d)))
    };
    let (end, ymd) = match format {
        DateFormat::YmdDash => {
            let (y, p) = digits(text, p, Some(4))?;
            let p = byte(text, p, b'-')?;
            let (m, p) = digits(text, p, Some(2))?;
            let p = byte(text, p, b'-')?;
            let (d, p) = digits(text, p, Some(2))?;
            (p, (y, m, d))
        }
        DateFormat::DmyName => {
            let (d, p) = digits(text, p, None)?;
            let p = byte(text, p, b' ')?;
            let (m, p) = month(text, p, months)?;
            let p = byte(text, optional(text, p, b'.'), b' ')?;
            let (y, p) = digits(text, p, Some(4))?;
            (p, (y, m, d))
        }
        DateFormat::MdyName => {
            let (m, p) = month(text, p, months)?;
            let p = byte(text, optional(text, p, b'.'), b' ')?;
            let (d, p) = digits(text, p, None)?;
            let p = byte(text, optional(text, p, b','), b' ')?;
            let (y, p) = digits(text, p, Some(4))?;
            (p, (y, m, d))
        }
        DateFormat::DmySlash => numeric(b'/', true)?,
        DateFormat::MdySlash => numeric(b'/', false)?,
        DateFormat::DmyDot => numeric(b'.', true)?,
    };
    end_boundary(text, end).then_some((end, ymd))
}

/// Proleptic Gregorian validity (spec 070, 3.3.3).
fn calendar_valid((y, m, d): (u32, u32, u32)) -> bool {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let len = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=len).contains(&d)
}

/// `date` (spec 070, 3.3.3): the `occurrence`-th match rendered as
/// `YYYY-MM-DD`. A calendar-invalid match makes the rule not apply, and no
/// later match is tried.
pub(crate) fn date(
    text: &[u8],
    format: DateFormat,
    months: &[Vec<String>],
    occurrence: u8,
) -> Option<String> {
    let mut seen = 0;
    let mut start = 0;
    while start < text.len() {
        if start_boundary(text, start)
            && let Some((end, ymd)) = date_at(text, start, format, months)
        {
            if !calendar_valid(ymd) {
                return None;
            }
            seen += 1;
            if seen == occurrence {
                let (y, m, d) = ymd;
                return Some(format!("{y:04}-{m:02}-{d:02}"));
            }
            start = end;
            continue;
        }
        start += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_validity() {
        assert!(calendar_valid((2024, 2, 29)));
        assert!(calendar_valid((2000, 2, 29)));
        assert!(!calendar_valid((1900, 2, 29)));
        assert!(!calendar_valid((2026, 2, 29)));
        assert!(!calendar_valid((2026, 4, 31)));
        assert!(!calendar_valid((2026, 13, 1)));
        assert!(!calendar_valid((2026, 0, 1)));
        assert!(!calendar_valid((2026, 1, 0)));
    }

    #[test]
    fn skip_takes_one_separator_and_stops_at_a_line_break() {
        assert_eq!(skip(b": \tX", 0), 3);
        assert_eq!(skip(b": # X", 0), 2);
        assert_eq!(skip(b" -X", 0), 2);
        assert_eq!(skip(b"\nX", 0), 0);
        assert_eq!(skip(b" \n X", 0), 1);
    }
}
