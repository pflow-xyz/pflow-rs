# crmath corpus

`pow.txt`: one case per line, `x y r` as 16-hex-digit IEEE 754 bit patterns,
with `r` = x^y correctly rounded (ties to even). Written by
`scripts/crmath/corpus.py` from Python's `decimal` module at 120 significant
digits (integer exponents exactly); special cases from `math.pow` (C99 Annex F).

Byte-identical copies, each checked bit for bit against its own port:

| Copy | Port | Test |
|---|---|---|
| here | `pflow_solver::crmath::pow` (native, wasm32) | `tests/crmath_corpus.rs` |
| go-pflow `crmath/testdata/pow.txt` | `crmath.Pow` | `crmath/pow_test.go` |
| pflow-xyz `parity/crmath/pow.txt` | `internal/crmath.Pow`, `crPow` in `public/petri-solver.js` | `internal/crmath/pow_test.go`, `public/petri-crpow_test.ts` |

Regenerate only after a deliberate change to the corpus script, and re-copy
all three.
