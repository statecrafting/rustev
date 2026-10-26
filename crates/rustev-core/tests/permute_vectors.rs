use rustev_core::ranking::permute::{DrawKey, draw, permutation, presentation_bit};
use serde::Deserialize;

#[derive(Deserialize)]
struct Permutation {
    size: usize,
    seed: String,
    step: String,
    instance: Vec<String>,
    j: u64,
    order: Vec<usize>,
    rejected: u64,
}
#[derive(Deserialize)]
struct Draw {
    seed: String,
    step: String,
    instance: Vec<String>,
    j: serde_json::Value,
    i: u64,
    value: u64,
    attempt: u64,
}
#[derive(Deserialize)]
struct Vectors {
    permutations: Vec<Permutation>,
    draws: Vec<Draw>,
    small_pool_search: serde_json::Value,
}

#[test]
fn independent_python_known_answers() {
    let vectors: Vectors = serde_json::from_str(include_str!("vectors/permute.json")).unwrap();
    assert_eq!(vectors.small_pool_search["seeds"], 10000);
    // No size-6 rejection found in the committed bounded search. A large
    // range draw below tests the actual rejection path independently.
    assert!(vectors.small_pool_search["found"].is_null());
    for v in vectors.permutations {
        assert_eq!(
            permutation(v.size, &v.seed, &v.step, &v.instance, v.j).unwrap(),
            v.order
        );
        let rejected: u64 = (1..v.size)
            .map(|i| {
                draw(
                    &v.seed,
                    &v.step,
                    &v.instance,
                    DrawKey::Permutation(v.j),
                    i as u64,
                )
                .unwrap()
                .1
            })
            .sum();
        assert_eq!(rejected, v.rejected);
    }
    assert!(vectors.draws.iter().any(|v| v.attempt > 0));
    for v in vectors.draws {
        let key = match v.j.as_str() {
            Some(key) => DrawKey::Comparison(key),
            None => DrawKey::Permutation(v.j.as_u64().unwrap()),
        };
        assert_eq!(
            draw(&v.seed, &v.step, &v.instance, key, v.i).unwrap(),
            (v.value, v.attempt)
        );
        if let DrawKey::Comparison(key) = key {
            assert_eq!(
                presentation_bit(&v.seed, &v.step, &v.instance, key).unwrap(),
                v.value == 1
            );
        }
    }
}
