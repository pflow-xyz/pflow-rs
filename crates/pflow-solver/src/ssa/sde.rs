//! Chemical Langevin SDE (`MethodSDE` in go-pflow): the third leg of
//! Petri.jl's ODEProblem/JumpProblem/SDEProblem trio (go-pflow ROADMAP.md
//! G6), ported from go-pflow's `stochastic/sde.go`. Continuous state, but
//! with the net's own intrinsic firing noise — Euler-Maruyama over the same
//! propensities and stoichiometry [`compile`] already extracts for SSA.
//!
//! Refuses a gated model (any read arc, inhibitor or reachable capacity)
//! exactly as go-pflow's `Forecast`/`SimulateSDE` do: a firing instant is
//! what those need, and continuous diffusion has none.
//!
//! Part of the byte-exact contract the way SSA is, though `ssa-spec.md`
//! itself does not (yet) cover SDE: `tests/sde_parity.rs` asserts `==` on
//! every double of `tests/fixtures/sde/*.json` (go-pflow's `cmd/sde-goldens`
//! output, vendored via `go-pflow.lock`), and pflow-xyz's `parity/sde/` now
//! holds the same five fixtures against `petri-sde.js`. go-pflow remains the
//! reference implementation for the Gaussian sampler specifically
//! (`stochastic/portable_test.go`'s `TestPortableNormalVectors`, checked
//! directly against those vectors).

use super::{plog, CompiledModel, SsaError, SsaModel, Xoshiro256};

/// How many Euler-Maruyama steps run between each reported sample point.
/// Matches go-pflow's `sdeInternalSubsteps` exactly — chosen empirically
/// against the consistency tests, not derived, and deliberately not a public
/// option for the same reason it isn't one there.
const INTERNAL_SUBSTEPS: usize = 20;

/// [`super::SsaOptions`] under another name: identical shape, kept distinct
/// so a caller can't pass one where the other is meant by accident.
#[derive(Debug, Clone, PartialEq)]
pub struct SdeOptions {
    pub horizon: f64,
    pub samples: usize,
    pub realizations: usize,
    pub seed: u64,
}

impl Default for SdeOptions {
    fn default() -> Self {
        Self {
            horizon: 10.0,
            samples: 101,
            realizations: 1,
            seed: 1,
        }
    }
}

/// Ensemble result. Mirrors [`super::SsaResult`] plus the refusal fields
/// go-pflow's `Result.Diverged`/`Reason`/`Caveats` carry — SDE can refuse a
/// model outright, which SSA never does.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SdeResult {
    pub places: Vec<String>,
    pub times: Vec<f64>,
    pub values: Vec<Vec<f64>>,
    pub stddev: Option<Vec<Vec<f64>>>,
    pub final_: Vec<f64>,
    pub diverged: bool,
    pub reason: String,
    pub caveats: Vec<String>,
}

/// `combinations` (spec-pinned, integer `m`) generalized to continuous state:
/// the same falling-factorial product evaluated at a real `x`, which agrees
/// with `combinations(m, w)` at every non-negative integer `m` — the
/// property that makes this the continuum limit rather than an arbitrary
/// generalization (`combinations_real_agrees_at_integers` below).
///
/// Below `x = w - 1` the product can go negative (e.g. x=0.5, w=2:
/// 0.5×-0.5/2 = -0.125) — not a bug, the same "wrong near zero" behaviour
/// go-pflow's copy documents; callers clamp the resulting propensity at
/// zero rather than let a negative term flip a sign.
pub fn combinations_real(x: f64, w: i64) -> f64 {
    if w <= 0 {
        return 1.0;
    }
    let mut result = 1.0;
    for i in 0..w {
        result *= x - i as f64;
        result /= (i + 1) as f64;
    }
    result
}

struct SdeTerm {
    place: usize,
    weight: i64,
}

struct SdeTransition {
    rate: f64,
    terms: Vec<SdeTerm>,
    delta: Vec<f64>,
}

fn propensity(t: &SdeTransition, x: &[f64]) -> f64 {
    let mut a = t.rate;
    for term in &t.terms {
        a *= combinations_real(x[term.place], term.weight);
        if a <= 0.0 {
            return 0.0;
        }
    }
    a
}

fn compile_sde(model: &CompiledModel) -> Vec<SdeTransition> {
    let n = model.place_ids.len();
    model
        .transitions
        .iter()
        .map(|t| {
            let mut delta = vec![0.0; n];
            let mut terms = Vec::new();
            for &(p, w, kinetic) in &t.inputs {
                delta[p] -= w as f64;
                if kinetic {
                    terms.push(SdeTerm {
                        place: p,
                        weight: w,
                    });
                }
            }
            for &(p, w) in &t.outputs {
                delta[p] += w as f64;
            }
            SdeTransition {
                rate: t.rate,
                terms,
                delta,
            }
        })
        .collect()
}

/// True if the model has anything an SDE (or the ODE) cannot express: a read
/// arc, an inhibitor, a non-kinetic input, or a reachable capacity. Mirrors
/// go-pflow's `Model.Gating()` word for word (`metamodel/firing.go`) at the
/// level this module can see it (post-`compile`, which already resolved
/// read/inhibitor/non-kinetic/capacity into `Compiled`'s own fields) rather
/// than re-deriving it from the raw model — the wording (counts, `%v`-style
/// `[a b c]` place lists) is part of the byte-exact contract the SDE goldens'
/// `diverged`/`reason`/`caveats` fields pin (`gates`/`coffeeshop`), not
/// incidental prose.
///
/// The `guards`/`delayed`/`staged` clauses `Gating()` also emits have no
/// counterpart here: `SsaModel` carries no guard expressions or stage counts
/// for this module to see, and none of the vendored SDE fixtures exercise a
/// delayed transition, so that clause is left approximate (delay is still
/// refused, just without go-pflow's per-transition `%v` id list) rather than
/// invented against nothing to check it.
fn gating_reasons(model: &CompiledModel) -> Vec<String> {
    let mut out = Vec::new();

    let reads: usize = model.transitions.iter().map(|t| t.reads.len()).sum();
    let inhibits: usize = model.transitions.iter().map(|t| t.inhibits.len()).sum();
    let static_arcs: usize = model
        .transitions
        .iter()
        .map(|t| t.inputs.iter().filter(|&&(_, _, kinetic)| !kinetic).count())
        .sum();

    if reads > 0 {
        out.push(format!(
            "{reads} read arc(s) gate a firing without consuming; a continuous solver cannot test them"
        ));
    }
    if inhibits > 0 {
        out.push(format!(
            "{inhibits} inhibitor arc(s) block a firing above a threshold; a continuous solver cannot test them"
        ));
    }
    if static_arcs > 0 {
        out.push(format!(
            "{static_arcs} non-kinetic input arc(s) gate and consume without scaling the rate; a mass-action solver has no way to omit them from the rate law"
        ));
    }

    // Distinct places a capacity is declared *and* reachable on (some
    // transition's net delta there is positive), in place-declaration order —
    // the same "raised" test `compile` already applied when populating
    // `Compiled::caps`.
    let mut caps: Vec<&str> = Vec::new();
    for (p, id) in model.place_ids.iter().enumerate() {
        if model
            .transitions
            .iter()
            .any(|t| t.caps.iter().any(|&(cp, _, _)| cp == p))
        {
            caps.push(id.as_str());
        }
    }
    if !caps.is_empty() {
        out.push(format!(
            "capacity is declared on [{}] but is a post-firing bound, which has no continuous analogue",
            caps.join(" ")
        ));
    }

    if model.transitions.iter().any(|t| t.delay > 0.0) {
        out.push(
            "a delay is a deterministic timer — inputs consumed at start, outputs a fixed time later — which mass action cannot express"
                .to_string(),
        );
    }

    out
}

fn sde_path(
    trs: &[SdeTransition],
    x0: &[f64],
    times: &[f64],
    rng: &mut GaussianSampler,
) -> Vec<Vec<f64>> {
    let n = x0.len();
    let mut out: Vec<Vec<f64>> = Vec::with_capacity(times.len());
    let mut x = x0.to_vec();
    out.push(x.clone());

    let dt_outer = if times.len() > 1 {
        (times[times.len() - 1] - times[0]) / (times.len() - 1) as f64
    } else {
        0.0
    };
    let dt = dt_outer / INTERNAL_SUBSTEPS as f64;
    let sqrt_dt = dt.sqrt();

    let mut drift = vec![0.0; n];
    for _gi in 1..times.len() {
        if dt > 0.0 {
            for _sub in 0..INTERNAL_SUBSTEPS {
                for d in drift.iter_mut() {
                    *d = 0.0;
                }
                for t in trs {
                    let a = propensity(t, &x);
                    if a == 0.0 {
                        continue;
                    }
                    let noise = a.sqrt() * sqrt_dt * rng.normal();
                    for (p, &d) in t.delta.iter().enumerate() {
                        if d == 0.0 {
                            continue;
                        }
                        drift[p] += d * a * dt;
                        x[p] += d * noise;
                    }
                }
                for p in 0..n {
                    x[p] += drift[p];
                    if x[p] < 0.0 {
                        x[p] = 0.0;
                    }
                    drift[p] = 0.0;
                }
            }
        }
        out.push(x.clone());
    }
    out
}

/// Wraps [`Xoshiro256`] with the Marsaglia polar method's spare-value cache
/// — exactly go-pflow's `portableSampler` (its `x` field plus
/// `hasSpare`/`spare`). Needs only `sqrt` (IEEE-754-exact and identical
/// across every conformant runtime, unlike `ln`) and [`plog`], both already
/// part of this module's contract — deliberately not Box-Muller, which would
/// need a second portable transcendental (sin/cos) this codebase has no
/// reference implementation for.
///
/// The cache is load-bearing, not an optimization: two consecutive
/// `normal()` calls on an unrejected (u1, u2) pair return `u1*mul` then
/// `u2*mul` from that SAME pair. A version that redraws every call produces
/// a different stream from go-pflow's from the second value on.
struct GaussianSampler {
    rng: Xoshiro256,
    spare: Option<f64>,
}

impl GaussianSampler {
    fn new(seed: u64) -> Self {
        Self {
            rng: Xoshiro256::new(seed),
            spare: None,
        }
    }

    fn normal(&mut self) -> f64 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        loop {
            let u1 = 2.0 * self.rng.uniform() - 1.0;
            let u2 = 2.0 * self.rng.uniform() - 1.0;
            let sq = u1 * u1 + u2 * u2;
            if sq > 0.0 && sq < 1.0 {
                let mul = (-2.0 * plog(sq) / sq).sqrt();
                self.spare = Some(u2 * mul);
                return u1 * mul;
            }
        }
    }
}

/// The chemical Langevin assumption every non-refused SDE result carries —
/// go-pflow's `ChemicalLangevinAssumption`, word for word.
pub const CHEMICAL_LANGEVIN_ASSUMPTION: &str = "this engine approximates the discrete firing process as continuous diffusion (the chemical Langevin equation), which is accurate when populations are large enough that the gap between SSA and this engine's mean is small (see the model's own consistency margin) and breaks down near zero, where a place's state is clamped rather than allowed to go negative.";

pub fn simulate_sde(model: &SsaModel, opts: &SdeOptions) -> Result<SdeResult, SsaError> {
    let compiled = super::compile(model)?;
    simulate_sde_compiled(&compiled, opts)
}

pub fn simulate_sde_compiled(
    model: &CompiledModel,
    opts: &SdeOptions,
) -> Result<SdeResult, SsaError> {
    if opts.samples < 2 {
        return Err(SsaError::BadOptions("samples must be >= 2".into()));
    }
    if opts.realizations < 1 {
        return Err(SsaError::BadOptions("realizations must be >= 1".into()));
    }
    if !opts.horizon.is_finite() || opts.horizon <= 0.0 {
        return Err(SsaError::BadOptions(
            "horizon must be positive and finite".into(),
        ));
    }

    let times = super::sample_grid(opts.horizon, opts.samples);

    let caveats = gating_reasons(model);
    if !caveats.is_empty() {
        return Ok(SdeResult {
            places: model.place_ids.clone(),
            times,
            values: Vec::new(),
            stddev: None,
            final_: Vec::new(),
            diverged: true,
            reason: format!(
                "this model constrains firing in ways continuous diffusion cannot express, so the SDE would silently model an unconstrained system. Use the discrete engine (Simulate). Specifically: {}",
                caveats.join("; ")
            ),
            caveats,
        });
    }

    let trs = compile_sde(model);
    let n_places = model.place_ids.len();
    let x0: Vec<f64> = model.initial.iter().map(|&m| m as f64).collect();

    let base = if opts.seed == 0 { 1 } else { opts.seed };
    let mut sums: Vec<Vec<f64>> = vec![vec![0.0; opts.samples]; n_places];
    let mut sumsq: Vec<Vec<f64>> = vec![vec![0.0; opts.samples]; n_places];

    for r in 0..opts.realizations {
        let mut rng = GaussianSampler::new(base.wrapping_add(r as u64));
        let path = sde_path(&trs, &x0, &times, &mut rng);
        for (gi, x) in path.iter().enumerate() {
            for (p, &v) in x.iter().enumerate() {
                sums[p][gi] += v;
                sumsq[p][gi] += v * v;
            }
        }
    }

    let n = opts.realizations as f64;
    let mut values: Vec<Vec<f64>> = vec![vec![0.0; opts.samples]; n_places];
    let mut stddev: Option<Vec<Vec<f64>>> = if opts.realizations > 1 {
        Some(vec![vec![0.0; opts.samples]; n_places])
    } else {
        None
    };
    for p in 0..n_places {
        for i in 0..opts.samples {
            let mean = sums[p][i] / n;
            values[p][i] = mean;
            if let Some(sd) = stddev.as_mut() {
                let mut variance = (sumsq[p][i] / n) - (mean * mean);
                if variance < 0.0 {
                    variance = 0.0;
                }
                sd[p][i] = variance.sqrt();
            }
        }
    }
    let final_: Vec<f64> = values.iter().map(|v| v[opts.samples - 1]).collect();

    Ok(SdeResult {
        places: model.place_ids.clone(),
        times,
        values,
        stddev,
        final_,
        diverged: false,
        reason: String::new(),
        caveats: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::combinations;
    use super::*;

    // Go's reference vectors, portable_test.go's TestPortableNormalVectors —
    // Go is the reference implementation for normal() (no external spec, the
    // way there is for wait()/uniform()), so this is the cross-language
    // check for this port, not a self-consistency test.
    #[test]
    fn normal_matches_go_reference_vectors() {
        let mut rng = GaussianSampler::new(42);
        let want: [u64; 5] = [
            0xbfe7_3d2f_eb0f_b377,
            0xbfcb_0880_2869_3f9c,
            0x3fcc_5e21_f781_2a4c,
            0x3fe0_ba8b_b0c5_fa51,
            0x3fdd_b514_bfac_5b4e,
        ];
        for (i, &w) in want.iter().enumerate() {
            let v = rng.normal();
            assert_eq!(v.to_bits(), w, "normal()[{i}]");
        }
    }

    #[test]
    fn combinations_real_agrees_at_integers() {
        for m in 0..=10i64 {
            for w in 0..=4i64 {
                let want = combinations(m, w);
                let got = combinations_real(m as f64, w);
                assert!(
                    (got - want).abs() < 1e-9,
                    "combinations_real({m}, {w}) = {got}, want {want}"
                );
            }
        }
    }

    #[test]
    fn combinations_real_goes_negative_below_w_minus_1() {
        let got = combinations_real(0.5, 2);
        assert!((got - (-0.125)).abs() < 1e-12, "got {got}");
    }

    #[test]
    fn combinations_real_weight_1_is_identity() {
        for x in [0.0, 0.3, 1.0, 5.7, 100.0] {
            assert_eq!(combinations_real(x, 1), x);
        }
    }

    // Fixtures mirroring go-pflow's stochastic/consistency_test.go — same
    // shapes (linear chain, SIR at scale, weight-2 dimerisation), reproduced
    // here rather than shared because they live in a private `tests` module
    // in `ssa::tests`, not reachable from this sibling module.

    fn chain() -> super::super::SsaModel {
        use super::super::{SsaArc, SsaPlace, SsaTransition};
        super::super::SsaModel {
            places: vec![
                SsaPlace {
                    id: "a".into(),
                    initial: 100,
                    capacity: 0,
                },
                SsaPlace {
                    id: "b".into(),
                    initial: 0,
                    capacity: 0,
                },
                SsaPlace {
                    id: "c".into(),
                    initial: 0,
                    capacity: 0,
                },
            ],
            transitions: vec![
                SsaTransition {
                    id: "ab".into(),
                    rate: 1.0,
                    delay: 0.0,
                },
                SsaTransition {
                    id: "bc".into(),
                    rate: 1.0,
                    delay: 0.0,
                },
            ],
            arcs: vec![
                SsaArc::flow("a", "ab"),
                SsaArc::flow("ab", "b"),
                SsaArc::flow("b", "bc"),
                SsaArc::flow("bc", "c"),
            ],
        }
    }

    fn sir(scale: i64) -> super::super::SsaModel {
        use super::super::{SsaArc, SsaPlace, SsaTransition};
        super::super::SsaModel {
            places: vec![
                SsaPlace {
                    id: "S".into(),
                    initial: 990 * scale,
                    capacity: 0,
                },
                SsaPlace {
                    id: "I".into(),
                    initial: 10 * scale,
                    capacity: 0,
                },
                SsaPlace {
                    id: "R".into(),
                    initial: 0,
                    capacity: 0,
                },
            ],
            transitions: vec![
                SsaTransition {
                    id: "infect".into(),
                    rate: 0.0005 / scale as f64,
                    delay: 0.0,
                },
                SsaTransition {
                    id: "recover".into(),
                    rate: 0.1,
                    delay: 0.0,
                },
            ],
            arcs: vec![
                SsaArc::flow("S", "infect"),
                SsaArc::flow("I", "infect"),
                SsaArc::flow_weighted("infect", "I", 2),
                SsaArc::flow("I", "recover"),
                SsaArc::flow("recover", "R"),
            ],
        }
    }

    fn dimer() -> super::super::SsaModel {
        use super::super::{SsaArc, SsaPlace, SsaTransition};
        super::super::SsaModel {
            places: vec![
                SsaPlace {
                    id: "A".into(),
                    initial: 50,
                    capacity: 0,
                },
                SsaPlace {
                    id: "B".into(),
                    initial: 0,
                    capacity: 0,
                },
            ],
            transitions: vec![
                SsaTransition {
                    id: "dimerise".into(),
                    rate: 0.01,
                    delay: 0.0,
                },
                SsaTransition {
                    id: "dissociate".into(),
                    rate: 0.1,
                    delay: 0.0,
                },
            ],
            arcs: vec![
                SsaArc::flow_weighted("A", "dimerise", 2),
                SsaArc::flow("dimerise", "B"),
                SsaArc::flow("B", "dissociate"),
                SsaArc::flow_weighted("dissociate", "A", 2),
            ],
        }
    }

    #[test]
    fn sde_refuses_delayed_transition() {
        use super::super::{SsaArc, SsaModel, SsaPlace, SsaTransition};
        let m = SsaModel {
            places: vec![
                SsaPlace {
                    id: "a".into(),
                    initial: 3,
                    capacity: 0,
                },
                SsaPlace {
                    id: "b".into(),
                    initial: 0,
                    capacity: 0,
                },
            ],
            transitions: vec![SsaTransition {
                id: "t".into(),
                rate: 0.0,
                delay: 1.5,
            }],
            arcs: vec![SsaArc::flow("a", "t"), SsaArc::flow("t", "b")],
        };
        let res = simulate_sde(
            &m,
            &SdeOptions {
                horizon: 1.0,
                samples: 2,
                realizations: 1,
                seed: 1,
            },
        )
        .expect("compiles");
        assert!(res.diverged, "SDE must refuse a delayed transition");
        assert!(
            res.caveats.iter().any(|c| c.contains("delay")),
            "caveats do not name the delay: {:?}",
            res.caveats
        );
    }

    #[test]
    fn sde_refuses_gated_model() {
        use super::super::{ArcKind, SsaArc, SsaModel, SsaPlace, SsaTransition};
        let m = SsaModel {
            places: vec![
                SsaPlace {
                    id: "a".into(),
                    initial: 10,
                    capacity: 10,
                },
                SsaPlace {
                    id: "licence".into(),
                    initial: 1,
                    capacity: 0,
                },
                SsaPlace {
                    id: "b".into(),
                    initial: 0,
                    capacity: 0,
                },
            ],
            transitions: vec![SsaTransition {
                id: "t".into(),
                rate: 1.0,
                delay: 0.0,
            }],
            arcs: vec![
                SsaArc::flow("a", "t"),
                SsaArc {
                    from: "licence".into(),
                    to: "t".into(),
                    weight: 1,
                    kind: ArcKind::Read,
                    kinetic: true,
                },
                SsaArc::flow("t", "b"),
            ],
        };
        let res = simulate_sde(
            &m,
            &SdeOptions {
                horizon: 1.0,
                samples: 2,
                realizations: 1,
                seed: 1,
            },
        )
        .unwrap();
        assert!(res.diverged, "SDE did not refuse a model with a read arc");
        assert!(
            !res.caveats.is_empty(),
            "diverged with no caveats naming why"
        );
    }

    // The SDE mean should track the SSA mean on a linear chain, which is
    // mean-field exact — the same law-of-large-numbers relationship
    // consistency_test.go holds SSA to, checked here against SSA itself
    // rather than a separately-built ODE reference (this crate's ODE
    // Problem/solve API is not wired into this test file).
    #[test]
    fn sde_consistency_linear_chain_tracks_ssa() {
        use super::super::{simulate, SsaOptions};
        let m = chain();
        let opts_common = (400usize, 20260902u64);
        let ssa = simulate(
            &m,
            &SsaOptions {
                horizon: 6.0,
                samples: 61,
                realizations: opts_common.0,
                seed: opts_common.1,
            },
        )
        .unwrap();
        let sde = simulate_sde(
            &m,
            &SdeOptions {
                horizon: 6.0,
                samples: 61,
                realizations: opts_common.0,
                seed: opts_common.1,
            },
        )
        .unwrap();
        assert!(!sde.diverged, "sde diverged: {}", sde.reason);
        for (p, id) in sde.places.iter().enumerate() {
            let ssa_p = ssa.place_index(id).unwrap();
            let mut max_diff = 0.0_f64;
            for i in 0..sde.times.len() {
                max_diff = max_diff.max((sde.values[p][i] - ssa.values[ssa_p][i]).abs());
            }
            assert!(
                max_diff <= 2.0,
                "{id}: max|sde mean - ssa mean| = {max_diff} (limit 2.0)"
            );
        }
        let final_sum: f64 = sde.final_.iter().sum();
        assert!(
            (final_sum - 100.0).abs() <= 2.0,
            "sum of means at horizon = {final_sum}, want ~100"
        );
    }

    // SDE's variance should approximate SSA's at large population — the
    // regime where both approximate the CTMC well. Mirrors
    // consistency_test.go's TestSDEVarianceMatchesSSAAtScale.
    #[test]
    fn sde_variance_matches_ssa_at_scale() {
        use super::super::{simulate, SsaOptions};
        let m = sir(10); // N = 10,000
        let horizon = 40.0;
        let samples = 81;
        let realizations = 100;
        let seed = 20260902;
        let ssa = simulate(
            &m,
            &SsaOptions {
                horizon,
                samples,
                realizations,
                seed,
            },
        )
        .unwrap();
        let sde = simulate_sde(
            &m,
            &SdeOptions {
                horizon,
                samples,
                realizations,
                seed,
            },
        )
        .unwrap();
        assert!(!sde.diverged, "sde diverged: {}", sde.reason);
        let ssa_sd = ssa.stddev.as_ref().unwrap();
        let sde_sd = sde.stddev.as_ref().unwrap();
        for (p, id) in sde.places.iter().enumerate() {
            let ssa_p = ssa.place_index(id).unwrap();
            let mut peak = 0;
            for i in 1..samples {
                if ssa_sd[ssa_p][i] > ssa_sd[ssa_p][peak] {
                    peak = i;
                }
            }
            if ssa_sd[ssa_p][peak] < 1e-9 {
                continue;
            }
            let rel = (sde_sd[p][peak] - ssa_sd[ssa_p][peak]).abs() / ssa_sd[ssa_p][peak];
            assert!(
                rel <= 0.35,
                "{id}: stdev at peak (grid {peak}) sde={} ssa={}, relative diff {rel} (limit 0.35)",
                sde_sd[p][peak],
                ssa_sd[ssa_p][peak]
            );
        }
    }

    // combinationsReal generalizes SSA's exact combinatorics, so an SDE run
    // on a weight-2 dimerisation net should track SSA's mean closely —
    // proving the SDE propensity really is the continuum limit of C(m, w).
    // Mirrors consistency_test.go's TestSDEDimerisationDisagreesFromODELikeSSA
    // minus the ODE half (not wired into this test file).
    #[test]
    fn sde_dimerisation_tracks_ssa() {
        use super::super::{simulate, SsaOptions};
        let m = dimer();
        let horizon = 5.0;
        let samples = 51;
        let realizations = 300;
        let seed = 20260902;
        let ssa = simulate(
            &m,
            &SsaOptions {
                horizon,
                samples,
                realizations,
                seed,
            },
        )
        .unwrap();
        let sde = simulate_sde(
            &m,
            &SdeOptions {
                horizon,
                samples,
                realizations,
                seed,
            },
        )
        .unwrap();
        assert!(!sde.diverged, "sde diverged: {}", sde.reason);
        for (p, id) in sde.places.iter().enumerate() {
            let ssa_p = ssa.place_index(id).unwrap();
            let mut max_diff = 0.0_f64;
            for i in 0..sde.times.len() {
                max_diff = max_diff.max((sde.values[p][i] - ssa.values[ssa_p][i]).abs());
            }
            assert!(
                max_diff <= 3.0,
                "{id}: max|sde - ssa| = {max_diff}, want close (both use C(m,w))"
            );
        }
    }
}
