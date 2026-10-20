//! Spec 027, 3.14: the binding's hand-written code contains no `unsafe`
//! block; only napi-rs's generated glue may use it.

#[test]
fn hand_written_code_has_no_unsafe() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        // Source files only; a module directory is walked by its own entry
        // if one is ever added, and fails here until it is.
        assert!(p.is_file(), "{}: the check reads files only", p.display());
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        for (i, line) in code_of(&text).lines().enumerate() {
            assert!(
                !has_unsafe_keyword(line),
                "{}:{}: hand-written unsafe",
                p.display(),
                i + 1
            );
        }
    }
}

/// `text` as code only: comments (`//` to the end of the line, `/* */`
/// across lines) and the contents of string literals (plain, byte and raw,
/// `r#"…"#`) blanked, newlines kept so line numbers hold. A comment marker
/// or quote inside a literal, or inside a char literal, never changes how
/// the rest is read, and text inside a string is not code.
fn code_of(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let (c, next) = (b[i], b.get(i + 1).copied());
        let ident_before = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if c == b'/' && next == Some(b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && next == Some(b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                if b[i] == b'\n' {
                    out.push(b'\n');
                }
                i += 1;
            }
            i += 2;
        } else if !ident_before && (c == b'r' || (c == b'b' && next == Some(b'r'))) && {
            let start = if c == b'b' { i + 2 } else { i + 1 };
            let hashes = b[start..].iter().take_while(|x| **x == b'#').count();
            b.get(start + hashes) == Some(&b'"')
        } {
            // A raw string: copied whole, up to `"` and as many `#`.
            let start = if c == b'b' { i + 2 } else { i + 1 };
            let hashes = b[start..].iter().take_while(|x| **x == b'#').count();
            let mut end = start + hashes + 1;
            while end < b.len()
                && !(b[end] == b'"'
                    && b[end + 1..]
                        .iter()
                        .take(hashes)
                        .filter(|x| **x == b'#')
                        .count()
                        == hashes)
            {
                end += 1;
            }
            let end = (end + 1 + hashes).min(b.len());
            blank(&b[i..end], &mut out);
            i = end;
        } else if c == b'"' {
            // A string (a byte string's `b` prefix is plain code before it),
            // escapes included.
            let mut end = i + 1;
            while end < b.len() && b[end] != b'"' {
                end += if b[end] == b'\\' { 2 } else { 1 };
            }
            let end = (end + 1).min(b.len());
            blank(&b[i..end], &mut out);
            i = end;
        } else if c == b'\'' && (b.get(i + 2) == Some(&b'\'') || next == Some(b'\\')) {
            // A char literal (`'"'`, `'\''`); a lifetime has no closing quote.
            // After an escape, the escaped character is part of the literal.
            let mut end = if next == Some(b'\\') { i + 3 } else { i + 2 };
            while end < b.len() && b[end] != b'\'' {
                end += 1;
            }
            let end = (end + 1).min(b.len());
            out.extend_from_slice(&b[i..end]);
            i = end;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `line` uses the keyword `unsafe`: the word itself, not inside a
/// longer identifier such as `unsafe_code`.
fn has_unsafe_keyword(line: &str) -> bool {
    let ident = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    line.match_indices("unsafe").any(|(i, w)| {
        !ident(line[..i].chars().next_back()) && !ident(line[i + w.len()..].chars().next())
    })
}

/// A literal as two quotes around nothing, its newlines kept.
fn blank(literal: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(b"\"\"");
    out.extend(literal.iter().filter(|c| **c == b'\n'));
}

#[test]
fn comments_and_string_contents_are_not_code() {
    assert_eq!(
        code_of(r#"let s = "a//b"; unsafe {}"#),
        r#"let s = ""; unsafe {}"#
    );
    assert_eq!(code_of(r#"let s = b"unsafe"; x"#), r#"let s = b""; x"#);
    assert_eq!(code_of("let x = 1; // unsafe in a comment"), "let x = 1; ");
    assert_eq!(code_of("a /* unsafe\nstill */ b"), "a \n b");
    assert_eq!(
        code_of(r##"let s = r#"a "quoted" // x"#; unsafe {}"##),
        r#"let s = ""; unsafe {}"#
    );
    assert_eq!(code_of("let s = \"line\nbreak\"; y"), "let s = \"\"\n; y");
    assert_eq!(
        code_of(r#"if c == b'"' { x } // unsafe"#),
        r#"if c == b'"' { x } "#
    );
    assert_eq!(
        code_of("fn f<'a>(x: &'a str) {} // unsafe"),
        "fn f<'a>(x: &'a str) {} "
    );
    assert_eq!(code_of(r"let q = '\''; // unsafe"), r"let q = '\''; ");
    assert!(code_of("fn f() { unsafe { g() } }").contains("unsafe"));
}

#[test]
fn the_keyword_is_found_and_identifiers_are_not() {
    assert!(has_unsafe_keyword("unsafe { x() }"));
    assert!(has_unsafe_keyword("pub unsafe fn f() {}"));
    assert!(!has_unsafe_keyword("#[allow(unsafe_code)]"));
    assert!(!has_unsafe_keyword("let not_unsafe = 1;"));
}
