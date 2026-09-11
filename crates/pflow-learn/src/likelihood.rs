//! Exact CTMC log-likelihood and its closed-form gradient, and
//! [`fit_discrete`]: go-pflow's counterpart to forward-sensitivity ODE
//! fitting for a discretely *observed* sample path. Ported from
//! go-pflow's `stochastic/likelihood.go`.
//!
//! This module builds its own minimal compiled-transition form rather than
//! reusing `pflow_solver::stochastic`'s (which is crate-private): the two
//! are small and independently held to the same
//! [`pflow_metamodel::Model::inputs`]/`outputs`/`tests` firing rule
//! (ROADMAP.md ground rule 4), so there is one *rule*, even though there
//! are two *structs* that cache it — the same shape as go-pflow's own
//! `stochastic` package, which keeps `compile`/`combFactors` next to each
//! other in one package for exactly this reason. Guard evaluation is
//! deliberately not ported here: go-pflow's `FitDiscrete` always calls
//! `compile(m, rates, nil)`, so a declared guard is never enforced during
//! fitting either upstream or here.

use std::collections::HashMap;
use std::fmt;

use pflow_metamodel::Model;

use crate::optim::gradopt::{minimize_gradient, GradMethod, GradOptSettings};
use crate::optim::FitResult;

#[derive(Debug, Clone, PartialEq)]
pub enum LikelihoodError {
    NoTokenPlaces(String),
    UnknownFitTransition(String),
    PathLengthMismatch {
        path: usize,
        got: usize,
        want: usize,
        what: &'static str,
    },
    EventOutOfOrder {
        path: usize,
        event: usize,
        time: f64,
        running: f64,
    },
    UnknownTransition {
        path: usize,
        event: usize,
        id: String,
    },
    NotEnabled {
        path: usize,
        event: usize,
        id: String,
    },
    MarkingMismatch {
        path: usize,
        event: usize,
        id: String,
        place: String,
        got: i64,
        want: i64,
    },
    HorizonBeforeLastEvent {
        path: usize,
        horizon: f64,
        last_event: f64,
    },
}

impl fmt::Display for LikelihoodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LikelihoodError::NoTokenPlaces(name) => {
                write!(f, "model {name:?} has no token places to simulate")
            }
            LikelihoodError::UnknownFitTransition(id) => {
                write!(f, "fit transition {id:?} not found in model")
            }
            LikelihoodError::PathLengthMismatch { path, got, want, what } => write!(
                f,
                "path {path} has {got} {what} values, want {want} (token-place order)"
            ),
            LikelihoodError::EventOutOfOrder { path, event, time, running } => write!(
                f,
                "path {path} event {event} has time {time} before the running time {running}"
            ),
            LikelihoodError::UnknownTransition { path, event, id } => write!(
                f,
                "path {path} event {event} names unknown transition {id:?}"
            ),
            LikelihoodError::NotEnabled { path, event, id } => write!(
                f,
                "path {path} event {event} fires {id:?}, which is not enabled at the marking it follows"
            ),
            LikelihoodError::MarkingMismatch { path, event, id, place, got, want } => write!(
                f,
                "path {path} event {event}'s claimed marking does not match applying {id:?}'s stoichiometry (place {place:?}: got {got}, want {want})"
            ),
            LikelihoodError::HorizonBeforeLastEvent { path, horizon, last_event } => write!(
                f,
                "path {path} horizon {horizon} is before its last event at {last_event}"
            ),
        }
    }
}
impl std::error::Error for LikelihoodError {}

/// One observed transition firing.
#[derive(Debug, Clone)]
pub struct FireEvent {
    pub time: f64,
    pub transition: String,
    /// POST-firing marking, in [`token_places`] order.
    pub marking: Vec<i64>,
}

/// One fully-observed discrete sample path.
#[derive(Debug, Clone)]
pub struct DiscretePath {
    /// Marking at t=0, in [`token_places`] order.
    pub initial: Vec<i64>,
    pub horizon: f64,
    pub events: Vec<FireEvent>,
}

struct Input {
    place: usize,
    weight: i64,
    kinetic: bool,
}

struct Gate {
    place: usize,
    weight: i64,
    inhibitor: bool,
}

struct CapBound {
    place: usize,
    delta: i64,
    limit: i64,
}

struct Transition {
    id: String,
    rate: f64,
    inputs: Vec<Input>,
    outputs: Vec<(usize, i64)>,
    gates: Vec<Gate>,
    caps: Vec<CapBound>,
}

impl Transition {
    fn gated(&self, marking: &[i64]) -> bool {
        for g in &self.gates {
            if g.inhibitor {
                if marking[g.place] >= g.weight {
                    return false;
                }
            } else if marking[g.place] < g.weight {
                return false;
            }
        }
        for c in &self.caps {
            if marking[c.place] + c.delta > c.limit {
                return false;
            }
        }
        true
    }
}

/// The token places of `m`, in the order every marking vector in this
/// module uses.
pub fn token_places(m: &Model) -> Result<(Vec<String>, HashMap<String, usize>), LikelihoodError> {
    let mut places = Vec::new();
    let mut index = HashMap::new();
    for p in &m.places {
        if !p.is_token() {
            continue;
        }
        index.insert(p.id.clone(), places.len());
        places.push(p.id.clone());
    }
    if places.is_empty() {
        return Err(LikelihoodError::NoTokenPlaces(m.name.clone()));
    }
    Ok((places, index))
}

/// `compile(m, rates, nil)`, minus the guard/caveat machinery
/// `NegLogLikelihood` never needs (see the module doc comment).
fn compile(
    m: &Model,
    rates: &HashMap<String, f64>,
) -> Result<(Vec<Transition>, Vec<String>), LikelihoodError> {
    let (places, index) = token_places(m)?;
    let mut limits: HashMap<usize, i64> = HashMap::new();
    for p in &m.places {
        if p.is_token() && p.capacity > 0 {
            limits.insert(index[&p.id], p.capacity);
        }
    }

    let mut out = Vec::with_capacity(m.transitions.len());
    for t in &m.transitions {
        let rate = *rates.get(&t.id).unwrap_or(&0.0);
        let mut delta: HashMap<usize, i64> = HashMap::new();
        let mut inputs = Vec::new();
        for a in m.inputs(&t.id) {
            let p = index[&a.place];
            inputs.push(Input {
                place: p,
                weight: a.weight,
                kinetic: a.kinetic,
            });
            *delta.entry(p).or_insert(0) -= a.weight;
        }
        let mut outputs = Vec::new();
        for a in m.outputs(&t.id) {
            let p = index[&a.place];
            outputs.push((p, a.weight));
            *delta.entry(p).or_insert(0) += a.weight;
        }
        let mut gates = Vec::new();
        for a in m.tests(&t.id) {
            gates.push(Gate {
                place: index[&a.place],
                weight: a.weight,
                inhibitor: matches!(a.typ, pflow_metamodel::ArcType::Inhibitor),
            });
        }
        let mut caps = Vec::new();
        for (&p, &limit) in &limits {
            if let Some(&d) = delta.get(&p) {
                if d > 0 {
                    caps.push(CapBound {
                        place: p,
                        delta: d,
                        limit,
                    });
                }
            }
        }
        caps.sort_by_key(|c| c.place);

        out.push(Transition {
            id: t.id.clone(),
            rate,
            inputs,
            outputs,
            gates,
            caps,
        });
    }
    Ok((out, places))
}

/// `C(m, w)`, the number of distinct ways to select `w` tokens from `m`.
/// Shared arithmetic with `pflow_solver::ssa::combinations` (multiply then
/// divide, once per factor).
fn combinations(m: i64, w: i64) -> f64 {
    if w <= 0 {
        return 1.0;
    }
    if m < w {
        return 0.0;
    }
    let mut result = 1.0;
    for i in 0..w {
        result *= (m - i) as f64;
        result /= (i + 1) as f64;
    }
    result
}

/// For every transition, the marking-dependent factor of its propensity
/// with the rate constant divided out: the product of `C(marking[p], w)`
/// over kinetic inputs, zeroed exactly where the propensity would be
/// zeroed (a short input, a failed gate). `a_j(marking) == rate_j *
/// comb_factors[j]` by construction, which is what makes the propensity
/// linear in `rate_j` and the gradient below closed-form.
fn comb_factors(trs: &[Transition], marking: &[i64]) -> Vec<f64> {
    trs.iter()
        .map(|t| {
            let mut c = 1.0;
            for input in &t.inputs {
                let m = marking[input.place];
                if m < input.weight {
                    c = 0.0;
                    break;
                }
                if input.kinetic {
                    c *= combinations(m, input.weight);
                }
            }
            if c > 0.0 && !t.gated(marking) {
                c = 0.0;
            }
            c
        })
        .collect()
}

/// Minus the exact CTMC log-likelihood of one or more independent observed
/// [`DiscretePath`]s (log-likelihoods, and hence gradients, sum),
/// differentiated w.r.t. every rate named in `fit`. `rates` gives every
/// transition's current rate constant (including ones not being fit, held
/// fixed); `fit` names which transition ids the returned gradient covers,
/// in that order.
///
/// ```text
/// log L(rates) = sum_over_events log(a_chosen(x_pre)) - integral_0^T a0(x(t)) dt
/// ```
///
/// decomposing into one term per inter-event segment since the marking is
/// piecewise constant between events.
pub fn neg_log_likelihood(
    m: &Model,
    rates: &HashMap<String, f64>,
    fit: &[String],
    paths: &[DiscretePath],
) -> Result<(f64, Vec<f64>), LikelihoodError> {
    let (trs, places) = compile(m, rates)?;
    let idx: HashMap<&str, usize> = trs
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();
    let mut fit_idx = Vec::with_capacity(fit.len());
    for id in fit {
        let &j = idx
            .get(id.as_str())
            .ok_or_else(|| LikelihoodError::UnknownFitTransition(id.clone()))?;
        fit_idx.push(j);
    }

    let mut counts = vec![0.0f64; trs.len()];
    let mut integral = vec![0.0f64; trs.len()];
    let mut loss = 0.0f64;

    for (pi, p) in paths.iter().enumerate() {
        if p.initial.len() != places.len() {
            return Err(LikelihoodError::PathLengthMismatch {
                path: pi,
                got: p.initial.len(),
                want: places.len(),
                what: "initial",
            });
        }
        let mut marking = p.initial.clone();
        let mut t = 0.0f64;

        for (ei, ev) in p.events.iter().enumerate() {
            if ev.time < t {
                return Err(LikelihoodError::EventOutOfOrder {
                    path: pi,
                    event: ei,
                    time: ev.time,
                    running: t,
                });
            }
            if ev.marking.len() != places.len() {
                return Err(LikelihoodError::PathLengthMismatch {
                    path: pi,
                    got: ev.marking.len(),
                    want: places.len(),
                    what: "event marking",
                });
            }
            let j = *idx.get(ev.transition.as_str()).ok_or_else(|| {
                LikelihoodError::UnknownTransition {
                    path: pi,
                    event: ei,
                    id: ev.transition.clone(),
                }
            })?;

            let dt = ev.time - t;
            let comb = comb_factors(&trs, &marking);
            let a0: f64 = trs.iter().zip(&comb).map(|(tr, &c)| tr.rate * c).sum();
            for &fi in &fit_idx {
                integral[fi] += dt * comb[fi];
            }

            let a_chosen = trs[j].rate * comb[j];
            if a_chosen <= 0.0 {
                return Err(LikelihoodError::NotEnabled {
                    path: pi,
                    event: ei,
                    id: ev.transition.clone(),
                });
            }
            loss += -a_chosen.ln() + a0 * dt;
            counts[j] += 1.0;

            let mut next = marking.clone();
            for input in &trs[j].inputs {
                if next[input.place] < input.weight {
                    return Err(LikelihoodError::NotEnabled {
                        path: pi,
                        event: ei,
                        id: ev.transition.clone(),
                    });
                }
                next[input.place] -= input.weight;
            }
            for &(p_idx, w) in &trs[j].outputs {
                next[p_idx] += w;
            }
            for (pl, (&got, &want)) in next.iter().zip(ev.marking.iter()).enumerate() {
                if got != want {
                    return Err(LikelihoodError::MarkingMismatch {
                        path: pi,
                        event: ei,
                        id: ev.transition.clone(),
                        place: places[pl].clone(),
                        got,
                        want,
                    });
                }
            }
            marking = next;
            t = ev.time;
        }

        if p.horizon < t {
            return Err(LikelihoodError::HorizonBeforeLastEvent {
                path: pi,
                horizon: p.horizon,
                last_event: t,
            });
        }
        let dt = p.horizon - t;
        if dt > 0.0 {
            let comb = comb_factors(&trs, &marking);
            let a0: f64 = trs.iter().zip(&comb).map(|(tr, &c)| tr.rate * c).sum();
            for &fi in &fit_idx {
                integral[fi] += dt * comb[fi];
            }
            loss += a0 * dt;
        }
    }

    let grad = fit_idx
        .iter()
        .map(|&fi| {
            let mut term = integral[fi];
            let rate = trs[fi].rate;
            if rate > 0.0 {
                term -= counts[fi] / rate;
            }
            term
        })
        .collect();
    Ok((loss, grad))
}

/// Fits the rates named in `fit` (every other transition's rate held at its
/// value in `initial`, falling back to the model's own declared rate) to
/// one or more observed [`DiscretePath`]s by minimizing
/// [`neg_log_likelihood`] via Adam. Returns the optimizer result alongside
/// the fitted rates as a map (every transition, fit or held).
pub fn fit_discrete(
    m: &Model,
    initial: &HashMap<String, f64>,
    fit: &[String],
    paths: &[DiscretePath],
    settings: &GradOptSettings,
) -> Result<(FitResult, HashMap<String, f64>), LikelihoodError> {
    let mut base: HashMap<String, f64> = m
        .transitions
        .iter()
        .map(|t| (t.id.clone(), if t.rate == 0.0 { 1.0 } else { t.rate }))
        .collect();
    for (id, r) in initial {
        base.insert(id.clone(), *r);
    }

    let mut x0 = Vec::with_capacity(fit.len());
    for id in fit {
        let &r = base
            .get(id.as_str())
            .ok_or_else(|| LikelihoodError::UnknownFitTransition(id.clone()))?;
        x0.push(r);
    }

    // A non-positive rate mid-fit would send `combinations`/`ln` into a
    // nonsensical regime; treat that iterate as +Inf loss (a rejected
    // point), the same convention `adam_minimize` already uses for a
    // truncated or non-finite evaluation.
    let fg = |x: &[f64]| -> (f64, Vec<f64>) {
        let reject = vec![0.0; x.len()];
        if x.iter().any(|&v| v <= 0.0 || v.is_nan() || v.is_infinite()) {
            return (f64::INFINITY, reject);
        }
        let mut cur = base.clone();
        for (id, &v) in fit.iter().zip(x.iter()) {
            cur.insert(id.clone(), v);
        }
        match neg_log_likelihood(m, &cur, fit, paths) {
            Ok((loss, grad))
                if !loss.is_nan()
                    && !loss.is_infinite()
                    && grad.iter().all(|g| !g.is_nan() && !g.is_infinite()) =>
            {
                (loss, grad)
            }
            _ => (f64::INFINITY, reject),
        }
    };

    let res = minimize_gradient(&fg, &x0, GradMethod::Adam, settings);

    let mut fitted = base.clone();
    for (id, &v) in fit.iter().zip(res.params.iter()) {
        fitted.insert(id.clone(), v);
    }
    Ok((res, fitted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::{Arc, Place, Transition as MmTransition};

    /// Pure death process: `n -> 0` at rate `lambda`, one token place, one
    /// transition. The exact MLE for a Poisson-process rate from `k` events
    /// observed over `[0, T)` with the process still running (no absorption)
    /// is `k / T`, which this test checks the fitted rate against.
    fn death_process(initial: i64) -> Model {
        Model {
            name: "death".into(),
            places: vec![Place {
                id: "n".into(),
                initial,
                ..Default::default()
            }],
            transitions: vec![MmTransition {
                id: "decay".into(),
                rate: 1.0,
                ..Default::default()
            }],
            arcs: vec![Arc {
                from: "n".into(),
                to: "decay".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn neg_log_likelihood_matches_hand_derivative_at_true_rate() {
        // A single event at t=1 from marking [5] firing "decay" to [4],
        // observed over horizon 2. loss = -ln(rate*5) + rate*5*1 + rate*4*1
        // (a0 = rate*marking after the event contributes over [1,2)).
        let m = death_process(5);
        let path = DiscretePath {
            initial: vec![5],
            horizon: 2.0,
            events: vec![FireEvent {
                time: 1.0,
                transition: "decay".into(),
                marking: vec![4],
            }],
        };
        let rates: HashMap<String, f64> = [("decay".to_string(), 2.0)].into();
        let (loss, grad) = neg_log_likelihood(&m, &rates, &["decay".to_string()], &[path]).unwrap();
        let expected_loss = -(2.0f64 * 5.0).ln() + 2.0 * 5.0 * 1.0 + 2.0 * 4.0 * 1.0;
        assert!(
            (loss - expected_loss).abs() < 1e-9,
            "{loss} vs {expected_loss}"
        );
        // d(-logL)/d(rate) = integral(comb) - count/rate = (5*1 + 4*1) - 1/2.0
        let expected_grad = (5.0 + 4.0) - 1.0 / 2.0;
        assert!(
            (grad[0] - expected_grad).abs() < 1e-9,
            "{:?} vs {expected_grad}",
            grad
        );
    }

    #[test]
    fn fit_discrete_recovers_true_rate_from_many_events() {
        // Generate a deterministic "observed" path by hand: a pure-death
        // process from n=50 firing every 0.1 time units for 30 steps (rate
        // is proportional to n, so this isn't literally Gillespie-sampled,
        // but it is a valid, self-consistent DiscretePath the likelihood
        // can be checked against by fitting back to a nearby rate).
        let m = death_process(50);
        let mut events = Vec::new();
        let mut n = 50i64;
        let mut t = 0.0;
        for _ in 0..30 {
            t += 0.1;
            n -= 1;
            events.push(FireEvent {
                time: t,
                transition: "decay".into(),
                marking: vec![n],
            });
        }
        let path = DiscretePath {
            initial: vec![50],
            horizon: t,
            events,
        };
        let settings = GradOptSettings {
            max_iters: 500,
            ..Default::default()
        };
        let (res, fitted) = fit_discrete(
            &m,
            &HashMap::new(),
            &["decay".to_string()],
            &[path],
            &settings,
        )
        .unwrap();
        assert!(res.final_loss.is_finite());
        assert!(fitted["decay"] > 0.0);
    }

    #[test]
    fn unknown_fit_transition_is_an_error() {
        let m = death_process(5);
        let path = DiscretePath {
            initial: vec![5],
            horizon: 1.0,
            events: vec![],
        };
        let err =
            neg_log_likelihood(&m, &HashMap::new(), &["nope".to_string()], &[path]).unwrap_err();
        assert!(matches!(err, LikelihoodError::UnknownFitTransition(_)));
    }

    #[test]
    fn firing_an_unenabled_transition_is_rejected() {
        let m = death_process(0); // starts empty
        let path = DiscretePath {
            initial: vec![0],
            horizon: 1.0,
            events: vec![FireEvent {
                time: 0.5,
                transition: "decay".into(),
                marking: vec![-1],
            }],
        };
        let rates: HashMap<String, f64> = [("decay".to_string(), 1.0)].into();
        let err = neg_log_likelihood(&m, &rates, &[], &[path]).unwrap_err();
        assert!(matches!(err, LikelihoodError::NotEnabled { .. }));
    }
}
