//! Closed segment ancestry wire shape and canonical round trips (spec 066).
use rustev_contract::bounded::parse_bounded;
use rustev_contract::canonical::canonical_bytes;
use rustev_contract::definition::Predicate;
use rustev_contract::limits::DEFINITION_V1;
use serde_json::json;

#[test]
fn ancestry_canonical_round_trip_and_closed_shape() {
    for sep in [":", "/", "|", "."] {
        let doc = json!({"under":{"value":{"ref":"input:location"},"ancestor":{"lit":{"text":"us"}},"separator":sep}});
        let bytes = serde_json::to_vec(&doc).unwrap();
        let parsed: Predicate = parse_bounded(&bytes, &DEFINITION_V1).unwrap();
        let canonical = canonical_bytes(&parsed).unwrap();
        assert_eq!(canonical, canonical_bytes(&doc).unwrap());
        assert_eq!(
            parse_bounded::<Predicate>(&canonical, &DEFINITION_V1).unwrap(),
            parsed
        );
        for field in ["value", "ancestor", "separator"] {
            let mut bad = doc.clone();
            bad["under"].as_object_mut().unwrap().remove(field);
            assert!(
                parse_bounded::<Predicate>(&serde_json::to_vec(&bad).unwrap(), &DEFINITION_V1)
                    .is_err()
            );
        }
        let mut bad = doc.clone();
        bad["under"]["unknown"] = json!(true);
        assert!(
            parse_bounded::<Predicate>(&serde_json::to_vec(&bad).unwrap(), &DEFINITION_V1).is_err()
        );
        for field in ["value", "ancestor", "separator"] {
            let duplicate = format!(
                "{{\"under\":{{\"{field}\":{},{} }}}}",
                doc["under"][field],
                serde_json::to_string(&doc["under"])
                    .unwrap()
                    .trim_start_matches('{')
                    .trim_end_matches('}')
            );
            assert!(parse_bounded::<Predicate>(duplicate.as_bytes(), &DEFINITION_V1).is_err());
        }
    }
    for sep in ["-", "::", "", "é", "\\"] {
        let doc = json!({"under":{"value":{"lit":{"text":"a:b"}},"ancestor":{"lit":{"text":"a"}},"separator":sep}});
        assert!(
            parse_bounded::<Predicate>(&serde_json::to_vec(&doc).unwrap(), &DEFINITION_V1).is_err()
        );
    }
}
