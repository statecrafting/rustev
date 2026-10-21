//! Spec 027, 3.11.3: the corpus covers every call site, its inputs are what
//! 3.11.3 asks for, and this target's digests are the committed ones.

use rustev_fp_corpus::{INPUTS, SITES, inputs};
use std::path::Path;
use std::process::Command;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."))
}

/// Rust source with comments (line, block, nested) and string, raw string
/// (`r"..."`, `r#"..."#`, byte forms too) and character literals blanked, so
/// only code is scanned for calls.
fn code_only(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
        } else if (c == 'r' || (c == 'b' && next == Some('r')))
            && !b[..i]
                .last()
                .is_some_and(|p| p.is_alphanumeric() || *p == '_')
        {
            // A raw string: `r`, `br`, then `#`s, a quote, and the same
            // number of `#`s after the closing quote. Anything else is code.
            let mut j = i + if c == 'b' { 2 } else { 1 };
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) != Some(&'"') {
                out.push(c);
                i += 1;
                continue;
            }
            j += 1;
            loop {
                if j >= b.len() {
                    break;
                }
                if b[j] == '"' && (1..=hashes).all(|k| b.get(j + k) == Some(&'#')) {
                    j += 1 + hashes;
                    break;
                }
                j += 1;
            }
            i = j;
            out.push_str("\"\"");
        } else if c == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                i += if b[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            out.push_str("\"\"");
        } else if c == '\'' && (b.get(i + 2) == Some(&'\'') || next == Some('\\')) {
            // A character literal (`'"'`, `'\''`); a lifetime has no closing quote.
            i += if next == Some('\\') { 4 } else { 3 };
            out.push_str("' '");
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[test]
fn the_scan_sees_code_only() {
    let src = "let a = math::exp(x); // math::ln(y)\n/* math::ln(z) /* nested */ math::exp(w) */\nlet s = \"math::ln(q)\"; let c = '\"'; let b = math::ln(p);\nlet r = r#\"math::exp(u) \"quoted\" \"#; let t = br\"math::ln(v)\"; let attr = 1;";
    let code = code_only(src);
    assert_eq!(code.matches("math::exp(").count(), 1, "{code}");
    assert_eq!(code.matches("math::ln(").count(), 1, "{code}");
}

/// Each `rustev_core::math` call in a pure crate's sources, as
/// `(file, function)`, `math.rs` itself excluded.
fn math_calls() -> Vec<(String, &'static str)> {
    let mut found = Vec::new();
    for c in [
        "crates/rustev-contract",
        "crates/rustev-core",
        "crates/rustev-eval",
        "crates/rustev-surface",
    ] {
        let mut stack = vec![root().join(c).join("src")];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).expect("source directory") {
                let p = e.expect("entry").path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "rs") || p.ends_with("rustev-core/src/math.rs")
                {
                    continue;
                }
                // `/`-separated on every host, as the sites name them.
                let rel = p
                    .strip_prefix(root())
                    .expect("under the root")
                    .display()
                    .to_string()
                    .replace('\\', "/");
                let code = code_only(&std::fs::read_to_string(&p).expect("source"));
                for f in ["exp", "ln"] {
                    for _ in code.matches(&format!("math::{f}(")) {
                        found.push((rel.clone(), f));
                    }
                }
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_call_site_is_in_the_corpus() {
    // A new call of `rustev_core::math` in a pure crate needs its own site
    // (3.11.3: each call site reachable from a pure command); spec 047's
    // test already refuses a platform math call that bypasses the module.
    let mut sites: Vec<(String, &str)> = SITES
        .iter()
        .map(|s| {
            let f = if s.id.ends_with(".exp") { "exp" } else { "ln" };
            (s.file.to_string(), f)
        })
        .collect();
    sites.sort();
    assert_eq!(
        math_calls(),
        sites,
        "the pure crates' math calls and the corpus sites differ"
    );
}

#[test]
fn inputs_are_what_3_11_3_asks_for() {
    const { assert!(INPUTS >= 100_000) };
    for s in &SITES {
        let v = inputs(s);
        assert_eq!(v.len(), INPUTS, "{}", s.id);
        assert!(v.iter().all(|x| !x.is_nan()), "{}: no NaN input", s.id);
        let subnormal = v.iter().filter(|x| x.is_subnormal()).count();
        assert!(subnormal > 0, "{}: subnormal inputs", s.id);
        let large = v.iter().filter(|x| x.abs() > 1.0e100).count();
        let small = v
            .iter()
            .filter(|x| x.abs() < 1.0e-100 && **x != 0.0)
            .count();
        assert!(
            large > 0 || s.id.starts_with("eval."),
            "{}: large magnitudes",
            s.id
        );
        assert!(small > 0, "{}: tiny magnitudes", s.id);
    }
    // The two evaluation sites see the same domain from distinct draws.
    assert_ne!(inputs(&SITES[2]), inputs(&SITES[3]));
}

#[test]
fn this_target_matches_the_committed_digests() {
    let expected = root().join("tools/rustev-conformance/fp-corpus/expected.json");
    let out = Command::new(env!("CARGO_BIN_EXE_rustev-fp-corpus"))
        .arg("check")
        .arg(&expected)
        .output()
        .expect("the corpus runner");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
