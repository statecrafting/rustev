#!/usr/bin/env python3
"""The nDCG discount table of spec 041, 3.4.3 and 3.11.3.

For i from 1 to 100, D[i] is the binary64 value nearest to
1 / log2(i + 1), computed with the Python 3 standard library `decimal` at
60 significant digits and rounded half to even to binary64. Each entry is
written as its 16-hex-digit bit pattern; a Rust test compares the table
embedded in `rustev-eval` with this file bit for bit.

    python3 -I discounts.py           # write discounts.json
    python3 -I discounts.py --check   # regenerate and compare bytes
"""

import decimal
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MAX_POSITION = 100


def discount(i):
    ctx = decimal.Context(prec=60, rounding=decimal.ROUND_HALF_EVEN)
    x = decimal.Decimal(i + 1)
    log2 = ctx.divide(x.ln(ctx), decimal.Decimal(2).ln(ctx))
    exact = ctx.divide(decimal.Decimal(1), log2)
    # float() of a Decimal rounds its exact value half to even.
    return float(exact), format(exact, "f")


def render():
    rows = []
    for i in range(1, MAX_POSITION + 1):
        value, text = discount(i)
        bits = struct.pack(">d", value).hex()
        rows.append(
            "    {" + f'"i": {i}, "bits": {json.dumps(bits)}, "decimal": {json.dumps(text)}' + "}"
        )
    head = '{\n  "generator": "discounts.py",\n  "digits": 60,\n  "discounts": [\n'
    return (head + ",\n".join(rows) + "\n  ]\n}\n").encode()


def main():
    path = os.path.join(HERE, "discounts.json")
    data = render()
    if sys.argv[1:] == ["--check"]:
        with open(path, "rb") as f:
            if f.read() != data:
                sys.exit("discounts.json differs from its generator")
        return
    if sys.argv[1:]:
        sys.exit("usage: discounts.py [--check]")
    with open(path, "wb") as f:
        f.write(data)


if __name__ == "__main__":
    main()
