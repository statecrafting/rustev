"""Independent spec 039 3.8.4 oracle. Prints the committed JSON to stdout."""
import hashlib
import json


def draw(seed, step, instance, j, i):
    limit = (2**64 // (i + 1)) * (i + 1)
    attempt = 0
    while True:
        payload = json.dumps([seed, step, instance, j, i, attempt],
                             ensure_ascii=False, separators=(",", ":")).encode()
        digest = hashlib.sha256(b"rustev.permute/1\0" + payload).digest()
        value = int.from_bytes(digest[:8], "big")
        if value < limit:
            return value % (i + 1), attempt
        attempt += 1


def permutation(size, seed, step, instance, j):
    values = list(range(size))
    rejected = 0
    for i in range(size - 1, 0, -1):
        r, attempts = draw(seed, step, instance, j, i)
        rejected += attempts
        values[i], values[r] = values[r], values[i]
    return values, rejected


vectors = []
for size, seed, step, instance, j in [
    (1, "one", "rank", [], 0),
    (2, "two", "rank", [], 0),
    (3, "three", "rank", [], 1),
    (5, "five", 'rank"\\\n', ["a/b", "é"], 2),
    (8, "eight", "rank", [], 7),
    (6, "six", "rank", ["nested", "0"], 0),
    (0, "empty", "rank", [], 0),
]:
    order, rejected = permutation(size, seed, step, instance, j)
    vectors.append(dict(size=size, seed=seed, step=step, instance=instance,
                        j=j, order=order, rejected=rejected))

# Small pool rejection is extremely rare. Search a reproducible bounded set.
found = None
for number in range(10000):
    seed = f"search{number}"
    order, rejected = permutation(6, seed, "rank", [], 0)
    if rejected:
        found = dict(seed=seed, order=order, rejected=rejected)
        break

# Exercise real rejection without allocating an enormous pool: i+1=2**63+1
# rejects almost half of all draws. This is an arithmetic vector, not a pool.
draws = []
for number in range(100):
    seed = f"reject{number}"
    value, attempt = draw(seed, "rank", [], 0, 2**63)
    if attempt:
        draws.append(dict(seed=seed, step="rank", instance=[], j=0,
                          i=2**63, value=value, attempt=attempt))
        break
assert draws, "bounded rejection search failed"
for j, i in [(0, 2**64 - 1), (0, 7), ("0", 1), ("0.1.2", 1), ("12", 1)]:
    value, attempt = draw("bits", "rank", ["x"], j, i)
    draws.append(dict(seed="bits", step="rank", instance=["x"], j=j,
                      i=i, value=value, attempt=attempt))

print(json.dumps(dict(permutations=vectors, draws=draws,
                      small_pool_search=dict(size=6, seeds=10000, found=found)),
                 ensure_ascii=False, indent=2))
