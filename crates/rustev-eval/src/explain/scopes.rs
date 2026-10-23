//! Recorded parameter scopes, in the traversal order of spec 040, 3.6.1.
use super::facts::{J, escape, matched};
use super::*;
use serde_json::Value;

pub(super) struct Scope<'a> {
    pub param: String,
    pub kind: ValueKind,
    pub value: &'a Value,
    pub path: String,
    pub linked: Option<String>,
}
pub(super) fn walk(set: &evidence::Set) -> Vec<Scope<'_>> {
    let mut scopes = vec![];
    if let Some(params) = set
        .get("record", &format!("{J}/outcome/propose/params"))
        .and_then(Value::as_object)
    {
        for (name, value) in params {
            let linked = matched(set).and_then(|(_, _, path)| {
                set.get("plan", &format!("{path}/then/propose/params"))?
                    .as_array()?
                    .iter()
                    .find(|p| p["name"].as_str() == Some(name))?
                    .pointer("/value/ref")?
                    .as_str()?
                    .strip_prefix("step:")
                    .map(str::to_string)
            });
            visit(
                name,
                value,
                format!("{J}/outcome/propose/params/{}", escape(name)),
                &linked,
                &mut scopes,
            );
        }
    }
    scopes
}
// Recursion is bounded: every value here comes from a document that passed
// its typed, bounded parse (`Document::parse`, at most `max_depth` nesting;
// serde_json also stops at 128), so the depth is the document limit.
fn visit<'a>(
    param: &str,
    value: &'a Value,
    path: String,
    linked: &Option<String>,
    out: &mut Vec<Scope<'a>>,
) {
    for (key, kind) in [
        ("filtered", ValueKind::Filtered),
        ("shortlist", ValueKind::Shortlist),
        ("ranking", ValueKind::Ranking),
    ] {
        if let Some(value) = value.get(key) {
            out.push(Scope {
                param: param.into(),
                kind,
                value,
                path: format!("{path}/{key}"),
                linked: linked.clone(),
            });
            return;
        }
    }
    if let Some(list) = value.get("list").and_then(Value::as_array) {
        for (i, value) in list.iter().enumerate() {
            visit(param, value, format!("{path}/list/{i}"), linked, out);
        }
    } else if let Some(record) = value.get("record").and_then(Value::as_object) {
        for (key, value) in record {
            visit(
                param,
                value,
                format!("{path}/record/{}", escape(key)),
                linked,
                out,
            );
        }
    }
}
impl Scope<'_> {
    pub fn member(&self, item: &str) -> Option<(usize, String)> {
        let key = match self.kind {
            ValueKind::Filtered => "eligible",
            ValueKind::Shortlist => "ids",
            ValueKind::Ranking => "entries",
        };
        self.value[key]
            .as_array()?
            .iter()
            .position(|v| {
                if self.kind == ValueKind::Ranking {
                    v["candidate"].as_str() == Some(item)
                } else {
                    v.as_str() == Some(item)
                }
            })
            .map(|i| (i, format!("{}/{key}/{i}", self.path)))
    }
    pub fn declaration(&self, set: &evidence::Set) -> Option<String> {
        let step = self.linked.as_deref()?;
        let n = set
            .get("plan", "/definition/steps")?
            .as_array()?
            .iter()
            .position(|s| s["id"].as_str() == Some(step))?;
        Some(format!("/definition/steps/{n}/body/exact"))
    }
}
