//! The changelog of a release, generated from the governed corpus (spec
//! 031, 3.10 and R-4).
//!
//! [`render`] is a pure function of [`Inputs`]: the specs at the previous
//! release and at the candidate (as `spec-spine registry list --json`
//! reports them, never read from `.statecraft/derived/` directly), the
//! decision rows that approved each spec, the commits that touched each
//! changed spec, and the compatibility facts of 3.6 and 3.7. Two runs on
//! the same inputs produce identical bytes. Nothing here is written by hand;
//! a reviewed limitations section may follow the generated text.

use std::collections::BTreeMap;

use serde::Deserialize;

/// One spec as the registry lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub id: String,
    pub title: String,
    pub status: String,
    pub implementation: String,
    #[serde(default)]
    pub amends: Vec<String>,
    /// Section anchor to digest; the approved text is every section but
    /// the implementation record.
    #[serde(default)]
    pub section_digests: BTreeMap<String, String>,
}

impl Spec {
    /// The digests of the approved text: every section but the
    /// implementation record, which changes as work lands.
    pub fn text_digests(&self) -> BTreeMap<&str, &str> {
        self.section_digests
            .iter()
            .filter(|(k, _)| !is_implementation_record(k))
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

/// The implementation record's section anchor: `implementation-record`,
/// or one numbered by any section path (`6-implementation-record`,
/// `3-2-implementation-record`). A section that only mentions
/// implementation records (such as `3-10-changelog-from-implementation-records`)
/// is approved text.
fn is_implementation_record(anchor: &str) -> bool {
    let mut rest = anchor;
    // Drop each leading `<digits>-` section number.
    while let Some((head, tail)) = rest.split_once('-') {
        if head.is_empty() || !head.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        rest = tail;
    }
    rest == "implementation-record"
}

/// `spec-spine registry list --json`.
#[derive(Debug, Deserialize)]
pub struct Registry {
    pub items: Vec<Spec>,
}

/// A reference plan's identity at the previous release and now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanIdentity {
    pub plan: String,
    pub before: Option<String>,
    pub after: String,
}

/// Everything the changelog states.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub version: String,
    /// The previous release's tag, or none for the first release.
    pub previous: Option<String>,
    pub commit: String,
    pub before: Vec<Spec>,
    pub after: Vec<Spec>,
    /// Spec id to the decision rows (`A-27`, ...) that approved it.
    pub approvals: BTreeMap<String, Vec<String>>,
    /// Spec id to `(short commit, subject)` of the commits since the
    /// previous release that touched it, oldest first.
    pub commits: BTreeMap<String, Vec<(String, String)>>,
    pub msrv: String,
    pub edition: String,
    pub plan_ids: Vec<PlanIdentity>,
    /// Superseded document or protocol versions with their end dates (3.6.2).
    pub superseded: Vec<(String, String)>,
}

/// Why a spec is in the changelog.
fn change(before: Option<&Spec>, after: &Spec) -> Option<String> {
    match before {
        None => Some("new".into()),
        Some(b) => {
            let mut what = Vec::new();
            if b.status != after.status {
                what.push(format!("status {} to {}", b.status, after.status));
            }
            if b.implementation != after.implementation {
                what.push(format!(
                    "implementation {} to {}",
                    b.implementation, after.implementation
                ));
            }
            if b.text_digests() != after.text_digests() {
                what.push("approved text changed".into());
            }
            let sorted = |v: &[String]| {
                let mut v = v.to_vec();
                v.sort();
                v
            };
            if sorted(&b.amends) != sorted(&after.amends) {
                what.push("amendment edges changed".into());
            }
            (!what.is_empty()).then(|| what.join("; "))
        }
    }
}

/// The changelog, as Markdown.
pub fn render(i: &Inputs) -> String {
    let mut out = String::new();
    let since = i
        .previous
        .as_deref()
        .map_or("the first release".to_string(), |p| format!("since `{p}`"));
    out += &format!("# Rustev {}\n\n", i.version);
    out += &format!(
        "Generated from the governed corpus at commit `{}`, {since} (spec 031, 3.10).\n\n",
        i.commit
    );

    out += "## Compatibility\n\n";
    out += &format!("- MSRV {}, edition {}.\n", i.msrv, i.edition);
    let changed: Vec<&PlanIdentity> = i
        .plan_ids
        .iter()
        .filter(|p| p.before.as_deref().is_some_and(|b| b != p.after))
        .collect();
    let mut plans: Vec<&PlanIdentity> = i.plan_ids.iter().collect();
    plans.sort_by(|a, b| a.plan.cmp(&b.plan));
    if i.previous.is_none() {
        out += "- Plan identities: first release, nothing to compare. Reference plans:\n";
    } else if changed.is_empty() {
        out += "- Plan identities: no reference plan's `PlanId` changed.\n";
    } else {
        out += "- Plan identities: CHANGED for the plans marked below (a breaking release under 3.6.4).\n";
    }
    for p in plans {
        let mark = match p.before.as_deref() {
            Some(b) if b != p.after => format!(" (was `{b}`)"),
            _ => String::new(),
        };
        out += &format!("  - `{}`: `{}`{mark}\n", p.plan, p.after);
    }
    if i.superseded.is_empty() {
        out += "- Superseded document and protocol versions: none.\n";
    } else {
        let mut s = i.superseded.clone();
        s.sort();
        for (version, end) in s {
            out += &format!("- `{version}` is superseded; it stays readable until {end}.\n");
        }
    }
    out += "\n## Specifications\n\n";

    let before: BTreeMap<&str, &Spec> = i.before.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut after: Vec<&Spec> = i.after.iter().collect();
    after.sort_by(|a, b| a.id.cmp(&b.id));
    let mut any = false;
    for s in after {
        let Some(why) = change(before.get(s.id.as_str()).copied(), s) else {
            continue;
        };
        any = true;
        out += &format!("### {}: {}\n\n", s.id, s.title);
        out += &format!(
            "- {why}; now `{}`, implementation `{}`.\n",
            s.status, s.implementation
        );
        if let Some(rows) = i.approvals.get(&s.id).filter(|r| !r.is_empty()) {
            out += &format!("- Approved: {}.\n", rows.join(", "));
        }
        if !s.amends.is_empty() {
            let mut a = s.amends.clone();
            a.sort();
            out += &format!("- Amends: {}.\n", a.join(", "));
        }
        if let Some(cs) = i.commits.get(&s.id).filter(|c| !c.is_empty()) {
            out += "- Commits:\n";
            for (sha, subject) in cs {
                out += &format!("  - `{sha}` {subject}\n");
            }
        }
        out += "\n";
    }
    let removed: Vec<&Spec> = before
        .values()
        .filter(|b| !i.after.iter().any(|s| s.id == b.id))
        .copied()
        .collect();
    for s in &removed {
        any = true;
        out += &format!(
            "### {}: {}\n\n- removed from the corpus.\n\n",
            s.id, s.title
        );
    }
    if !any {
        out += "No specification changed.\n";
    }
    out
}

/// The decision rows of `docs/decisions/00-founding-decisions.md` that
/// record an approval of each spec: a row whose text says the owner
/// ratified or approved spec `NNN`.
pub fn approvals(decisions: &str, specs: &[Spec]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in decisions.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let (Some(id), Some(text)) = (cells.get(1), cells.get(2)) else {
            continue;
        };
        if !id.starts_with("A-") {
            continue;
        }
        for s in specs {
            let number = s.id.split('-').next().unwrap_or("");
            let named = [
                format!("ratified spec `{number}`"),
                format!("approved spec `{number}`"),
            ];
            if named.iter().any(|n| text.contains(n.as_str())) {
                out.entry(s.id.clone()).or_default().push((*id).to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, status: &str, imp: &str, text: &str) -> Spec {
        Spec {
            id: id.into(),
            title: format!("title of {id}"),
            status: status.into(),
            implementation: imp.into(),
            amends: vec![],
            section_digests: BTreeMap::from([
                ("1-purpose".into(), text.into()),
                ("implementation-record".into(), format!("{imp}-record")),
            ]),
        }
    }

    fn inputs() -> Inputs {
        Inputs {
            version: "0.2.0".into(),
            previous: Some("v0.1.0".into()),
            commit: "abc".into(),
            before: vec![
                spec("001-a", "approved", "complete", "t1"),
                spec("002-b", "approved", "pending", "t2"),
                spec("003-c", "draft", "pending", "t3"),
            ],
            after: vec![
                spec("002-b", "approved", "complete", "t2"),
                spec("001-a", "approved", "complete", "t1"),
                spec("003-c", "approved", "pending", "t3x"),
                spec("004-d", "draft", "pending", "t4"),
            ],
            approvals: BTreeMap::from([("003-c".into(), vec!["A-9".into()])]),
            commits: BTreeMap::from([(
                "002-b".into(),
                vec![("1234567".into(), "impl(002): x".into())],
            )]),
            msrv: "1.88".into(),
            edition: "2024".into(),
            plan_ids: vec![PlanIdentity {
                plan: "support".into(),
                before: Some("sha256:1".into()),
                after: "sha256:1".into(),
            }],
            superseded: vec![],
        }
    }

    #[test]
    fn only_changed_specs_are_listed_in_id_order() {
        let text = render(&inputs());
        assert!(!text.contains("001-a"), "{text}");
        let b = text.find("### 002-b").unwrap();
        let c = text.find("### 003-c").unwrap();
        let d = text.find("### 004-d").unwrap();
        assert!(b < c && c < d);
        assert!(text.contains("implementation pending to complete"));
        assert!(text.contains("status draft to approved; approved text changed"));
        assert!(text.contains("- Approved: A-9."));
        assert!(text.contains("`1234567` impl(002): x"));
        assert!(text.contains("### 004-d: title of 004-d\n\n- new;"));
        assert!(text.contains("no reference plan's `PlanId` changed"));
    }

    #[test]
    fn rendering_is_deterministic_and_order_independent() {
        let a = render(&inputs());
        let mut i = inputs();
        i.after.reverse();
        i.before.reverse();
        // 3.10: a second run on the same inputs gives identical bytes.
        assert_eq!(a, render(&inputs()), "two runs on the same inputs");
        // And the order the registry lists specs in does not matter.
        assert_eq!(a, render(&i), "the same inputs in reverse order");
    }

    #[test]
    fn a_changed_plan_identity_is_stated_as_breaking() {
        let mut i = inputs();
        i.plan_ids[0].after = "sha256:2".into();
        let text = render(&i);
        assert!(text.contains("CHANGED"));
        assert!(text.contains("`support`: `sha256:2` (was `sha256:1`)"));
    }

    #[test]
    fn the_implementation_record_alone_is_not_a_text_change() {
        let mut i = inputs();
        i.after = vec![spec("001-a", "approved", "complete", "t1")];
        i.after[0]
            .section_digests
            .insert("implementation-record".into(), "other".into());
        i.before = vec![spec("001-a", "approved", "complete", "t1")];
        assert!(render(&i).contains("No specification changed."));
    }

    #[test]
    fn only_the_implementation_record_section_is_excluded() {
        assert!(is_implementation_record("implementation-record"));
        assert!(is_implementation_record("6-implementation-record"));
        assert!(is_implementation_record("3-2-implementation-record"));
        assert!(!is_implementation_record(
            "3-10-changelog-from-implementation-records"
        ));
        assert!(!is_implementation_record(
            "3-4-binding-records-before-implementation"
        ));
        // A changed section that only mentions the records is a text change.
        let mut i = inputs();
        i.before = vec![spec("001-a", "approved", "complete", "t1")];
        i.after = vec![spec("001-a", "approved", "complete", "t1")];
        i.after[0].section_digests.insert(
            "3-10-changelog-from-implementation-records".into(),
            "new".into(),
        );
        assert!(render(&i).contains("approved text changed"));
    }

    #[test]
    fn a_changed_amendment_edge_is_listed() {
        let mut i = inputs();
        i.before = vec![spec("001-a", "approved", "complete", "t1")];
        i.after = vec![spec("001-a", "approved", "complete", "t1")];
        i.after[0].amends = vec!["000-x".into()];
        let text = render(&i);
        assert!(text.contains("### 001-a"), "{text}");
        assert!(text.contains("amendment edges changed"));
    }

    #[test]
    fn a_removed_spec_is_listed_with_its_title() {
        let mut i = inputs();
        i.after.retain(|s| s.id != "001-a");
        assert!(render(&i).contains("### 001-a: title of 001-a\n\n- removed from the corpus."));
    }

    #[test]
    fn approvals_are_read_from_decision_rows() {
        let specs = vec![spec("031-x", "approved", "pending", "t")];
        let doc = "| A-27 | The owner ratified spec `031` (x). | 2026-10-16 |\n| R-1 | spec `031` mentioned. |\n| A-28 | approved spec `0310`? no |\n";
        assert_eq!(
            approvals(doc, &specs),
            BTreeMap::from([("031-x".into(), vec!["A-27".to_string()])])
        );
    }
}
