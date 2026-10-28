#!/usr/bin/env python3
"""Constants for crmath's pow, from Python's decimal module at 120 digits.

Prints them in Rust, Go and JS syntax (bit patterns, so no language's
decimal parser is trusted). Run: python3 scripts/crmath/consts.py rust|go|js
"""
import struct
import sys
from decimal import Decimal, getcontext

getcontext().prec = 120

def bits(f):
    return struct.unpack("<Q", struct.pack("<d", f))[0]

def dd(v):
    hi = float(v)
    lo = float(v - Decimal(hi))
    return hi, lo

LN2 = Decimal(2).ln()
C1 = struct.unpack("<d", struct.pack("<Q", 0x3FE62E42FEE00000))[0]
C2 = float(LN2 - Decimal(C1))
C3 = float(LN2 - Decimal(C1) - Decimal(C2))
INV_LN2 = float(1 / LN2)

N_ODD = 23   # 1/(2k+1), k = 0..22: ln((1+s)/(1-s)) to 2^-110 for |s| < 0.1716
N_FACT = 26  # 1/n!, n = 0..25: exp(r) to 2^-115 for |r| < 0.36

inv_odd = [dd(Decimal(1) / (2 * k + 1)) for k in range(N_ODD)]
fact = Decimal(1)
inv_fact = []
for n in range(N_FACT):
    if n:
        fact *= n
    inv_fact.append(dd(Decimal(1) / fact))

# ln 2 in fixed point, 320 fractional bits, rounded down: 5 limbs, low first.
ln2_fx = int(LN2 * (Decimal(2) ** 320))
limbs = [(ln2_fx >> (64 * i)) & 0xFFFFFFFFFFFFFFFF for i in range(5)]

lang = sys.argv[1] if len(sys.argv) > 1 else "rust"
if lang == "rust":
    f = lambda x: f"f64::from_bits(0x{bits(x):016X})"
    print(f"const LN2_1: f64 = {f(C1)};\nconst LN2_2: f64 = {f(C2)};\nconst LN2_3: f64 = {f(C3)};\nconst INV_LN2: f64 = {f(INV_LN2)};")
    print(f"const INV_ODD: [Dd; {N_ODD}] = [")
    for h, l in inv_odd:
        print(f"    Dd::new({f(h)}, {f(l)}),")
    print("];")
    print(f"const INV_FACT: [Dd; {N_FACT}] = [")
    for h, l in inv_fact:
        print(f"    Dd::new({f(h)}, {f(l)}),")
    print("];")
    print("const LN2_FX: Fx = Fx([" + ", ".join(f"0x{x:016X}" for x in limbs) + ", 0]);")
elif lang == "go":
    print(f"\tln2_1   = 0x{bits(C1):016X}\n\tln2_2   = 0x{bits(C2):016X}\n\tln2_3   = 0x{bits(C3):016X}\n\tinvLn2  = 0x{bits(INV_LN2):016X}")
    print("invOdd:")
    for h, l in inv_odd:
        print(f"\t{{0x{bits(h):016X}, 0x{bits(l):016X}}},")
    print("invFact:")
    for h, l in inv_fact:
        print(f"\t{{0x{bits(h):016X}, 0x{bits(l):016X}}},")
    print("ln2Fx: " + ", ".join(f"0x{x:016X}" for x in limbs))
elif lang == "js":
    print(f"LN2_1 0x{bits(C1):016X}n\nLN2_2 0x{bits(C2):016X}n\nLN2_3 0x{bits(C3):016X}n\nINV_LN2 0x{bits(INV_LN2):016X}n")
    print("INV_ODD", [(f"0x{bits(h):016X}", f"0x{bits(l):016X}") for h, l in inv_odd])
    print("INV_FACT", [(f"0x{bits(h):016X}", f"0x{bits(l):016X}") for h, l in inv_fact])
    print("LN2_FX 0x" + format(ln2_fx, "X") + "n")
