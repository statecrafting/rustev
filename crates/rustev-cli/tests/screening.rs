//! Additive refusal-name fixtures for spec 035. No CLI behavior changes.
mod common;
use common::*;
use serde_json::{Value, json};

#[test]
fn categories_18_and_19_print_like_every_category() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rustev-core/tests/golden/screening.definition.json");
    let base: Value = serde_json::from_slice(&std::fs::read(source).unwrap()).unwrap();
    let s = Scratch::new("screening-refusals");
    for (category, name) in [(18, "invalid_screening"), (19, "unscreened_text")] {
        let mut def = base.clone();
        if category == 18 {
            def["screening"]["entries"][0]["on_unavailable"] = json!("fail_open");
        } else {
            def["screening"]["entries"]
                .as_array_mut()
                .unwrap()
                .remove(1);
            def["steps"] = json!([{"id":"model","body":{"semantic":{
                "operation":"classify","task":"synthetic/1","question":"choose",
                "options":["a","b"],"candidates":"none","requires":"label",
                "project":["input:request"],"for_each":[],
                "backend":{"min_determinism":"unspecified","artifact":"any","binding":"auto"},
                "calibration":"none","fallback":"none"
            }}}]);
        }
        s.write("screening.json", &serde_json::to_vec(&def).unwrap());
        let r = s.rustev(&["plan", "check", "--definition", "screening.json"]);
        let result = r.expect(5, "refused");
        assert_eq!(result["category"], category);
        assert_eq!(result["category_name"], name);
        cli_golden(&format!("refusal-{category:02}.json"), &r.stdout);
    }
}
