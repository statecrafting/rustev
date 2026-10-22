#!/usr/bin/env python3
"""Reference implementation of spec 029, sections 3.3 and 3.4.

Written from the spec's text, not from the Rust code, with the Python 3
standard library only. From `cases.json` it writes `expected.json`, every
binary64 value as its 16-hex-digit bit pattern, which a Rust test compares
bit for bit. It also checks its generators against the known-answer vectors
in `generators.json`, which the authors' reference C implementations
produced (`generators.c`).

    python3 -I reference.py           # write expected.json
    python3 -I reference.py --check   # regenerate and compare bytes
"""

import hashlib
import json
import math
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MASK = (1 << 64) - 1
ONE = 10**9  # Decimal units: 9 fractional digits


def hexf(x):
    return struct.pack(">d", x).hex()


def unhex(h):
    return struct.unpack(">d", bytes.fromhex(h))[0]


def decimal_units(s):
    whole, _, frac = s.partition(".")
    return int(whole) * ONE + int((frac + "0" * 9)[:9])


# 3.4.2: SplitMix64 and xoshiro256**, on wrapping unsigned 64-bit integers.
class SplitMix64:
    def __init__(self, x):
        self.x = x & MASK

    def next(self):
        self.x = (self.x + 0x9E3779B97F4A7C15) & MASK
        z = self.x
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)


def rotl(x, k):
    return ((x << k) | (x >> (64 - k))) & MASK


class Xoshiro:
    def __init__(self, s):
        self.s = list(s)

    @staticmethod
    def seeded(x):
        sm = SplitMix64(x)
        return Xoshiro([sm.next() for _ in range(4)])

    def next(self):
        s = self.s
        result = (rotl((s[1] * 5) & MASK, 7) * 9) & MASK
        t = (s[1] << 17) & MASK
        s[2] ^= s[0]
        s[3] ^= s[1]
        s[1] ^= s[2]
        s[0] ^= s[3]
        s[2] ^= t
        s[3] = rotl(s[3], 45)
        return result

    # 3.4.4: a uniform index in [0, n) by rejection.
    def index(self, n):
        rem = (1 << 64) % n
        while True:
            u = self.next()
            if rem != 0 and u >= (1 << 64) - rem:
                continue
            return u % n


# 3.4.3: tagged_digest(tag, b) is sha256(tag || 0x00 || b).
def stream_seed(seed, label):
    h = hashlib.sha256()
    h.update(b"rustev.bootstrap/1")
    h.update(b"\x00")
    h.update(seed.encode() + b"\x00" + label.encode())
    return int.from_bytes(h.digest()[:8], "big")


def stream(seed, label):
    return Xoshiro.seeded(stream_seed(seed, label))


def total_key(x):
    """The IEEE 754 total order, so zeros of either sign sort one way.

    `b` is the bit pattern as a signed integer. A negative value keeps its
    sign (Python's `^` with a non-negative mask never clears it) and has
    its other 63 bits flipped, so a larger magnitude sorts lower:
    -inf < -2.0 < -1.0 < -0.0 < 0.0 < 1.0. `cases.json` holds negative
    values and both zeros, so `expected.json` checks this order.
    """
    b = struct.unpack(">q", struct.pack(">d", x))[0]
    return b ^ ((b >> 63) & 0x7FFFFFFFFFFFFFFF)


# Nearest rank (spec 004, 3.5.7): the ceil(q * n)th smallest, one-based.
def nearest_rank(samples, q_units):
    if not samples:
        return None, "no sample"
    v = sorted(samples, key=total_key)
    n = len(v)
    rank = min(max(-(-(q_units * n) // ONE), 1), n)
    return v[rank - 1], None


# 3.3.3 and 3.4.1: a case's contributions over runs, summed in run order,
# then combined over a sequence of cases in sequence order.
def cluster(form, contributions):
    present = [x for x in contributions if x is not None]
    if form == "count_ratio":
        return (sum(x[0] for x in present), sum(x[1] for x in present))
    if form == "mean":
        num = 0.0
        for x in present:
            num = num + unhex(x[0])
        return (num, sum(x[1] for x in present))
    return [unhex(x) for x in present]


def statistic(metric, clusters):
    form = metric["form"]
    if form == "count_ratio":
        num = sum(c[0] for c in clusters)
        den = sum(c[1] for c in clusters)
        if den == 0:
            return None, "zero denominator"
        return float(num) / float(den), None
    if form == "mean":
        num, den = 0.0, 0
        for c in clusters:
            num = num + c[0]
            den += c[1]
        if den == 0:
            return None, "zero denominator"
        v = num / float(den)
        return (v, None) if math.isfinite(v) else (None, "non-finite value")
    samples = []
    for c in clusters:
        samples.extend(c)
    return nearest_rank(samples, decimal_units(metric["quantile"]))


def figure(v, why):
    return hexf(v) if why is None else {"unknown": why}


# 3.3.2: per-run values, mean, sd, min, max; 3.3.3: pooled.
def run_stats(metric, rows, runs, deterministic):
    values = [statistic(metric, [cluster(metric["form"], [row[r]]) for row in rows]) for r in range(runs)]
    pooled = statistic(metric, [cluster(metric["form"], row) for row in rows])
    unknown = [str(r) for r, (v, why) in enumerate(values) if why is not None]
    out = {"metric": metric["name"], "runs": [figure(v, w) for v, w in values]}
    if unknown:
        u = {"unknown": "unknown at runs " + ", ".join(unknown)}
        out.update(mean=u, sd=u, min=u, max=u)
    else:
        v = [x for x, _ in values]
        total = 0.0
        for x in v:
            total = total + x
        mean = total / float(runs)
        if runs == 1:
            sd = hexf(0.0) if deterministic else {"unknown": "one run"}
        else:
            ss = 0.0
            for x in v:
                d = x - mean
                ss = ss + d * d
            sd = hexf(math.sqrt(ss / float(runs - 1)))
        out.update(
            mean=hexf(mean),
            sd=sd,
            min=hexf(min(v, key=total_key)),
            max=hexf(max(v, key=total_key)),
        )
    out["pooled"] = figure(*pooled)
    return out


# 3.4.7: exact integer ranks.
def ranks(level_units, b):
    lower = max(1, -(-((ONE - level_units) * b) // (2 * ONE)))
    upper = max(1, -(-((ONE + level_units) * b) // (2 * ONE)))
    return lower, upper


# 3.4.1 and 3.4.5: one stream per cohort; every metric uses the same B
# resamples of C consecutive draws; no resample is dropped.
def intervals(config, cohort, metrics, rows_by_metric, runs):
    c = len(cohort["cases"])
    b = config["resamples"]
    if b * c * runs > 10**9:
        raise SystemExit("bootstrap-too-large")
    clusters = [[cluster(m["form"], row) for row in rows] for m, rows in zip(metrics, rows_by_metric)]
    values = [[] for _ in metrics]
    undefined = [0 for _ in metrics]
    rng = stream(config["seed"], "cohort/" + cohort["name"])
    for _ in range(b):
        draws = [rng.index(c) for _ in range(c)]
        for i, m in enumerate(metrics):
            v, why = statistic(m, [clusters[i][d] for d in draws])
            if why is None:
                values[i].append(v)
            else:
                undefined[i] += 1
    lo, hi = ranks(decimal_units(config["level"]), b)
    out = []
    for m, v, u in zip(metrics, values, undefined):
        if u:
            out.append({"metric": m["name"], "unknown": f"{u} undefined resamples", "undefined": u})
        else:
            v.sort(key=total_key)
            out.append({"metric": m["name"], "lower": hexf(v[lo - 1]), "upper": hexf(v[hi - 1])})
    return out


def indices(rng, n, count):
    return [rng.index(n) for _ in range(count)]


def check_generators():
    with open(os.path.join(HERE, "generators.json")) as f:
        g = json.load(f)
    for e in g["splitmix64"]:
        sm = SplitMix64(int(e["seed"]))
        assert [str(sm.next()) for _ in e["outputs"]] == e["outputs"], e["seed"]
    for e in g["xoshiro256starstar"]:
        x = Xoshiro([int(s) for s in e["state"]])
        assert [str(x.next()) for _ in e["outputs"]] == e["outputs"], e["state"]
    for e in g["seeded"]:
        x = Xoshiro.seeded(int(e["splitmix64_seed"]))
        assert [str(x.next()) for _ in e["outputs"]] == e["outputs"], e["splitmix64_seed"]


def expected():
    with open(os.path.join(HERE, "cases.json")) as f:
        doc = json.load(f)
    configs = doc["configs"]
    out = {"schema": "rustev.bootstrap-expected/1", "streams": [], "ranks": [], "sets": []}
    for cfg in configs:
        labels = ["cohort/all", "cohort/hard", "run/0", "run/4"]
        out["streams"].append(
            {
                "seed": cfg["seed"],
                "labels": [{"label": l, "splitmix64_state": str(stream_seed(cfg["seed"], l))} for l in labels],
                "run_seeds": [str(stream(cfg["seed"], f"run/{r}").next()) for r in range(5)],
                "indices_7": indices(stream(cfg["seed"], "cohort/all"), 7, 12),
            }
        )
        lo, hi = ranks(decimal_units(cfg["level"]), cfg["resamples"])
        out["ranks"].append({"level": cfg["level"], "resamples": cfg["resamples"], "lower": lo, "upper": hi})
    for st in doc["sets"]:
        runs, det = st["runs"], st["deterministic"]
        cohorts = []
        for co in st["cohorts"]:
            rows = [[m["cases"][i] for i in co["cases"]] for m in st["metrics"]]
            cohorts.append(
                {
                    "name": co["name"],
                    "stats": [run_stats(m, r, runs, det) for m, r in zip(st["metrics"], rows)],
                    "intervals": [intervals(cfg, co, st["metrics"], rows, runs) for cfg in configs],
                }
            )
        out["sets"].append({"name": st["name"], "cohorts": cohorts})
    return out


def dump(v, ind=0):
    """Indented JSON with every leaf object or array on one line."""
    sp = " " * ind
    flat = lambda x: not isinstance(x, (dict, list)) or (
        isinstance(x, dict) and all(not isinstance(y, dict) and not (isinstance(y, list) and any(isinstance(z, (dict, list)) for z in y)) for y in x.values())
    ) or (isinstance(x, list) and all(not isinstance(y, (dict, list)) for y in x))
    if flat(v):
        return json.dumps(v)
    if isinstance(v, dict):
        return "{\n" + ",\n".join(f"{sp} {json.dumps(k)}: {dump(x, ind + 1)}" for k, x in v.items()) + "\n" + sp + "}"
    return "[\n" + ",\n".join(sp + " " + dump(x, ind + 1) for x in v) + "\n" + sp + "]"


def main():
    check_generators()
    text = (dump(expected()) + "\n").encode()
    path = os.path.join(HERE, "expected.json")
    if sys.argv[1:] == ["--check"]:
        with open(path, "rb") as f:
            if f.read() != text:
                sys.exit("expected.json differs from the reference output")
        print("expected.json matches")
    elif sys.argv[1:]:
        sys.exit("usage: reference.py [--check]")
    else:
        with open(path, "wb") as f:
            f.write(text)


main()
