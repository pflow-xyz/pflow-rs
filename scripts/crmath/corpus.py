#!/usr/bin/env python3
"""The crmath pow corpus: (x, y, x^y correctly rounded) as bit patterns.

The reference is Python's decimal module at 120 significant digits (integer
exponents exactly, at 2000), rounded to a double by float(), which is
correctly rounded. Special cases come from math.pow (C99 Annex F).
Deterministic: a fixed seed. Writes one case per line, three hex words:

    python3 scripts/crmath/corpus.py > crates/pflow-solver/tests/fixtures/crmath/pow.txt
"""
import math
import random
import struct
from decimal import Decimal, localcontext

def b2f(b): return struct.unpack("<d", struct.pack("<Q", b))[0]
def f2b(f): return struct.unpack("<Q", struct.pack("<d", f))[0]

def ref(x, y):
    if (x == 0 or math.isinf(x) or math.isnan(x) or math.isinf(y) or math.isnan(y)
            or y == 0 or x < 0 or x == 1):
        try:
            return math.pow(x, y)
        except (OverflowError, ValueError):
            return None
    if y == int(y) and abs(y) <= 64:
        with localcontext() as c:
            c.prec = 4000
            c.Emin, c.Emax = -999999, 999999
            v = Decimal(x) ** int(y)
            return float(v)
    with localcontext() as c:
        c.prec = 120
        c.Emin, c.Emax = -999999, 999999
        t = Decimal(y) * Decimal(x).ln()
        if t > 710:
            return math.inf
        if t < -746:
            return 0.0
        return float(t.exp())

rng = random.Random(20261028)
cases = []
def add(x, y):
    r = ref(x, y)
    if r is not None:
        cases.append((x, y, r))

# A. The ODE step controller: (1/err)^(1/(order+1)).
for i in range(6000):
    err = 10 ** rng.uniform(-14, 8)
    add(1.0 / err, 1.0 / (rng.choice([2, 3, 4, 5, 6, 7, 8, 9])))
# B. Any positive x, moderate y.
for i in range(1500):
    x = b2f(rng.randrange(1, 0x7FF0000000000000))
    add(x, rng.uniform(-8, 8))
# C. Any positive x, wide y.
for i in range(1000):
    x = b2f(rng.randrange(1, 0x7FF0000000000000))
    add(x, rng.choice([-1, 1]) * 10 ** rng.uniform(-20, 3))
# D. x near 1, large y.
for i in range(1000):
    x = 1.0 + rng.choice([-1, 1]) * 10 ** rng.uniform(-16, -1)
    add(x, rng.choice([-1, 1]) * 10 ** rng.uniform(0, 18))
# E. Small integer y, x with few significant bits: exact results and midpoints.
for i in range(1000):
    m = rng.randrange(1, 1 << rng.randrange(1, 28))
    x = math.ldexp(m, rng.randrange(-60, 10))
    add(x, float(rng.choice([2, 3, 4, -1, -2, 5, 7])))
for k in range(20, 30):
    add(1.0 + 2.0 ** -k, 2.0)
    add(1.0 - 2.0 ** -k, 2.0)
# F. Near overflow and through the subnormals.
for i in range(1000):
    add(2.0, rng.uniform(1020, 1025))
    add(2.0, -rng.uniform(1015, 1076))
for i in range(500):
    x = 10 ** rng.uniform(1, 300)
    add(x, 709.5 / math.log(x) * rng.uniform(0.999, 1.0005))
    add(x, -745 / math.log(x) * rng.uniform(0.95, 1.001))
# G. Special cases, negative bases.
sp = [0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5, math.inf, -math.inf, math.nan, 3.0, -3.0]
for x in sp:
    for y in sp + [0.25, -0.25, 1e300, -1e300, 2.0 ** 53 + 2, 2.0 ** 53 - 1]:
        add(x, y)
for i in range(300):
    x = -10 ** rng.uniform(-50, 50)
    add(x, float(rng.randrange(-40, 40)))
    add(x, rng.uniform(-3, 3))

for x, y, r in cases:
    rb = f2b(r) if not math.isnan(r) else 0x7FF8000000000000
    print(f"{f2b(x):016x} {f2b(y):016x} {rb:016x}")
