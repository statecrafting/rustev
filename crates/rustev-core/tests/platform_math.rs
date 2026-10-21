//! Spec 047, 3.3: no pure crate calls the platform's math library. `exp`
//! and `ln` go through `rustev_core::math`, which computes the same bits on
//! every target; a direct `f64` call would bring back the per-target
//! differences spec 027's differential corpus refuses.

use std::fs;
use std::path::{Path, PathBuf};

/// Platform math methods on `f64` whose results differ between targets.
const METHODS: &[&str] = &[
    "exp", "exp2", "exp_m1", "ln", "ln_1p", "log", "log2", "log10", "powf", "sin", "cos", "tan",
    "sin_cos", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh",
    "cbrt", "hypot",
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).expect("readable source directory") {
        let p = e.expect("directory entry").path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Each `file:line` calling a platform math method, comments excluded.
fn offences(text: &str, file: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        for m in METHODS {
            if code.contains(&format!(".{m}(")) || code.contains(&format!("f64::{m}(")) {
                found.push(format!("{file}:{}: {m}", n + 1));
            }
        }
    }
    found
}

#[test]
fn pure_crates_call_no_platform_math() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    for c in [
        "rustev-contract",
        "rustev-core",
        "rustev-eval",
        "rustev-surface",
    ] {
        let mut files = Vec::new();
        sources(&crates.join(c).join("src"), &mut files);
        assert!(!files.is_empty(), "{c} has sources");
        for f in files {
            // The one implementation itself.
            if f.ends_with("rustev-core/src/math.rs") {
                continue;
            }
            let text = fs::read_to_string(&f).expect("readable source");
            found.extend(offences(&text, &f.display().to_string()));
        }
    }
    assert!(
        found.is_empty(),
        "platform math calls (use rustev_core::math): {found:#?}"
    );
}

#[test]
fn the_scan_sees_a_direct_call() {
    // A seeded call of each form is found; the same text in a comment is not.
    assert_eq!(offences("let y = (v - max).exp();", "seed").len(), 1);
    assert_eq!(offences("sum -= p.ln();", "seed").len(), 1);
    assert_eq!(offences("let y = f64::ln(p);", "seed").len(), 1);
    assert!(offences("// was p.ln()", "seed").is_empty());
    assert!(offences("sum -= crate::math::ln(p);", "seed").is_empty());
}
