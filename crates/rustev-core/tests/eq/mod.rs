//! Shared SYNTHETIC fixtures of the evidence-quality tests (spec 032): a
//! lodging-style version 2 definition and its source descriptors.
#![allow(dead_code)]

use rustev_contract::Document;
use rustev_contract::definition::Definition;
use rustev_contract::source::SourceDescriptor;
use serde_json::{Value, json};

pub fn sources() -> Vec<SourceDescriptor> {
    let doc = |v: Value| SourceDescriptor::parse(v.to_string().as_bytes()).unwrap();
    vec![
        doc(
            json!({"schema": "rustev.source/1", "name": "catalogue", "version": "1.0.0",
            "facts": [{"name": "hotels", "provenance": "system-of-record", "grades": [
                {"path": "", "max_grade": "cached", "reliability": "reliable"}]}]}),
        ),
        doc(
            json!({"schema": "rustev.source/1", "name": "live-quote", "version": "1.0.0",
            "facts": [{"name": "quotes", "provenance": "third-party", "grades": [
                {"path": "", "max_grade": "authoritative", "reliability": "reliable"}]}]}),
        ),
        doc(
            json!({"schema": "rustev.source/1", "name": "geocoder", "version": "2.1.0",
            "facts": [
                {"name": "district", "provenance": "third-party", "grades": [
                    {"path": "", "max_grade": "authoritative", "reliability": "reliable"},
                    {"path": "kind", "max_grade": "authoritative",
                     "reliability": {"known_unreliable": {"note": "district labels drift"}}}]},
                {"name": "centre_subareas", "provenance": "third-party", "grades": [
                    {"path": "", "max_grade": "authoritative", "reliability": "reliable"}]}]}),
        ),
    ]
}

pub fn decimal_le(r: &str, v: &str) -> Value {
    json!({"cmp": {"left": {"ref": r}, "op": "le", "right": {"lit": {"decimal": v}}}})
}

/// A lodging-style version 2 definition: a cached catalogue, a live quote
/// map with presence predicates, a geocoded district whose `kind` label is
/// unreliable, a join, a merge and a filter over the merged records.
pub fn base() -> Value {
    json!({
        "schema": "rustev.definition/2", "name": "eq-lodging", "version": "0.1.0",
        "package": "synthetic",
        "inputs": [
            {"name": "hotels", "type": {"list": {"max_items": 20, "item": {"record": {"fields": [
                {"name": "id", "type": {"text": {"max_bytes": 16}}},
                {"name": "price", "type": "decimal"},
                {"name": "description", "type": {"text": {"max_bytes": 64}}}]}}}},
             "provenance": ["system-of-record"], "freshness": "not_required",
             "evidence": {"graded": {"sources": ["catalogue"], "caps": []}}},
            {"name": "quotes", "type": {"map": {"key_max_bytes": 16, "max_entries": 20,
                "value": {"record": {"fields": [
                    {"name": "price", "type": {"nullable": {"type": "decimal"}}},
                    {"name": "unit_price", "type": {"nullable": {"type": "decimal"}}}],
                    "present_when": {"any_non_null": ["price", "unit_price"]}}}}},
             "provenance": ["third-party"], "freshness": {"max_age_ms": 600000},
             "evidence": {"graded": {"sources": ["live-quote"], "caps": []}}},
            {"name": "district", "type": {"record": {"fields": [
                {"name": "name", "type": {"text": {"max_bytes": 32}}},
                {"name": "kind", "type": {"enum": {"options": ["centre", "suburb"]}}}]}},
             "provenance": ["third-party"], "freshness": "not_required",
             "evidence": {"graded": {"sources": ["geocoder"], "caps": []}}},
            {"name": "centre_subareas", "type": {"list": {"max_items": 20,
                "item": {"text": {"max_bytes": 32}}}},
             "provenance": ["third-party"], "freshness": "not_required",
             "evidence": {"graded": {"sources": ["geocoder"], "caps": []}}}
        ],
        "steps": [
            {"id": "joined", "body": {"exact": {"op": "join_keyed", "version": 1, "args": {
                "left": "input:hotels", "key": "id", "right": "input:quotes"}}}},
            {"id": "merged", "body": {"exact": {"op": "merge_fields", "version": 1, "args": {
                "base": "input:hotels", "overlay": "step:joined", "rules": [
                    {"field": "id", "rule": "base_only"},
                    {"field": "price", "rule": "overlay_only"},
                    {"field": "unit_price", "rule": "overlay_only"},
                    {"field": "description", "rule": "base_only"}]}}}},
            {"id": "affordable", "body": {"exact": {"op": "filter_with_reasons", "version": 1,
                "args": {"list": "step:joined", "id_field": "id", "rules": [
                    {"name": "under_budget", "predicate": decimal_le("item:match/price", "200")}]}}}}
        ],
        "policy": {"on_unresolved": [], "rules": [
            {"id": "fallback", "when": "always", "then": {"escalate": {"reason": "review"}}}],
            "adjustments": []},
        "limits": {"max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 0,
            "distribution_tolerance": "0.000001", "deadline_ms": 1000}
    })
}

pub fn def(v: &Value) -> Definition {
    Definition::parse(v.to_string().as_bytes()).unwrap()
}
