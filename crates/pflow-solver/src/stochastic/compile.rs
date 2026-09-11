//! Model -> index-addressed transitions, and the marking-based firing
//! tests. Ported from `compile`/`decidableFromMarking`/`allows`/`gated`/
//! `enabled` in go-pflow's `stochastic/stochastic.go`.
//!
//! The classification — what is an input, what merely tests the marking,
//! what weight an unset arc carries — comes from
//! [`pflow_metamodel::Model::inputs`]/`outputs`/`tests`, the shared firing
//! rule, rather than being re-derived here (ROADMAP.md ground rule 4).

use std::collections::HashMap;
use std::fmt;

use pflow_metamodel::{ArcType, Marking, Model};

/// A guard evaluator: decides a transition's guard expression against a
/// marking. See `Options::guard`.
pub type GuardEval<'a> = &'a dyn Fn(&str, &Marking) -> Result<bool, String>;

/// One input, output or test arc, resolved to a place index.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CArc {
    pub place: usize,
    pub weight: i64,
    /// Meaningful only on an input: whether this place's count enters the
    /// propensity product (see `propensities_at`).
    pub kinetic: bool,
}

/// A post-firing capacity bound: firing this transition raises `place` by
/// `delta`, and `place` may not end above `limit`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CapBound {
    pub place: usize,
    pub delta: i64,
    pub limit: i64,
}

/// A compiled transition.
#[derive(Debug, Clone)]
pub(crate) struct CTransition {
    pub id: String,
    pub rate: f64,
    pub inputs: Vec<CArc>,
    pub outputs: Vec<CArc>,
    pub reads: Vec<CArc>,
    pub inhibits: Vec<CArc>,
    pub caps: Vec<CapBound>,
    /// The guard expression, when it is decidable from the marking alone
    /// (evaluated every step against whatever [`GuardEval`] the caller
    /// supplies).
    pub guard: Option<String>,
    /// `> 0`: a timed transition; it has no rate, starts the instant it is
    /// enabled, and completes exactly `delay` later.
    pub delay: f64,
}

impl CTransition {
    /// Every constraint that lets this fire at `marking`, minus the
    /// consuming-input test (the exponential path folds that into the
    /// propensity; a timed transition has none and asks here).
    pub fn enabled(&self, places: &[String], marking: &[i64], guard: Option<GuardEval>) -> bool {
        for a in &self.inputs {
            if marking[a.place] < a.weight {
                return false;
            }
        }
        self.gated(marking) && self.allows(places, marking, guard)
    }

    /// The non-consuming constraints: reads, inhibitors, capacity.
    pub fn gated(&self, marking: &[i64]) -> bool {
        for a in &self.reads {
            if marking[a.place] < a.weight {
                return false;
            }
        }
        for a in &self.inhibits {
            if marking[a.place] >= a.weight {
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

    /// The index of the only input place this transition is short of, or
    /// `None` when it is short of none or of several — deliberately not
    /// reported when several, since with two things missing at once neither
    /// is the reason it did not fire.
    pub fn sole_short_input(&self, marking: &[i64]) -> Option<usize> {
        let mut short = None;
        for a in &self.inputs {
            if marking[a.place] >= a.weight {
                continue;
            }
            if short.is_some() {
                return None;
            }
            short = Some(a.place);
        }
        short
    }

    /// Evaluates the guard against the current marking, when set.
    pub fn allows(&self, places: &[String], marking: &[i64], guard: Option<GuardEval>) -> bool {
        let Some(expr) = &self.guard else {
            return true;
        };
        let Some(eval) = guard else {
            return true;
        };
        let mut mk = Marking::with_capacity(places.len());
        for (i, p) in places.iter().enumerate() {
            mk.insert(p.clone(), marking[i]);
        }
        // A guard that fails to evaluate mid-run was classified as
        // decidable at compile time, so this would be a bug rather than a
        // modelling choice; refuse the firing (over-reporting throughput is
        // the more damaging error for a capacity question).
        matches!(eval(expr, &mk), Ok(true))
    }
}

pub(crate) struct Compiled {
    pub transitions: Vec<CTransition>,
    pub places: Vec<String>,
    pub caveats: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StochasticError {
    NoTokenPlaces(String),
    UnknownFitTransition(String),
    /// Anything else: a mismatched carry count, a stage-expansion refusal
    /// (`pflow_metamodel::Error`'s message), and other conditions go-pflow
    /// reports as a plain `fmt.Errorf("stochastic: ...")`.
    Other(String),
}

impl fmt::Display for StochasticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StochasticError::NoTokenPlaces(name) => {
                write!(f, "model {name:?} has no token places to simulate")
            }
            StochasticError::UnknownFitTransition(id) => {
                write!(f, "fit transition {id:?} not found in model")
            }
            StochasticError::Other(msg) => write!(f, "stochastic: {msg}"),
        }
    }
}
impl std::error::Error for StochasticError {}

/// The token places of `m`, in the order every marking vector and
/// [`super::result::Series`] use.
pub fn token_places(m: &Model) -> Result<(Vec<String>, HashMap<String, usize>), StochasticError> {
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
        return Err(StochasticError::NoTokenPlaces(m.name.clone()));
    }
    Ok((places, index))
}

/// Reports whether a guard can be settled by token counts alone: probed by
/// evaluating it against the model's own initial marking with no bindings
/// in scope. Anything that fails to resolve — an action parameter, an
/// ambient value — references something beyond the marking.
fn decidable_from_marking(expr: &str, m: &Model, guard: GuardEval) -> bool {
    let mut probe = Marking::new();
    for p in &m.places {
        if p.is_token() {
            probe.insert(p.id.clone(), p.initial);
        }
    }
    guard(expr, &probe).is_ok()
}

/// Turns the model into index-addressed transitions. Returns any caveats:
/// constraints the model expresses that this engine cannot enforce. They
/// are reported, never silently dropped.
pub(crate) fn compile(
    m: &Model,
    rates: &HashMap<String, f64>,
    guard: Option<GuardEval>,
) -> Result<Compiled, StochasticError> {
    let (places, index) = token_places(m)?;

    let mut limits: HashMap<usize, i64> = HashMap::new();
    for p in &m.places {
        if p.is_token() && p.capacity > 0 {
            limits.insert(index[&p.id], p.capacity);
        }
    }

    let mut caveats = Vec::new();
    let mut out = Vec::with_capacity(m.transitions.len());
    for t in &m.transitions {
        let mut rate = *rates.get(&t.id).unwrap_or(&0.0);
        let mut delay = t.delay;
        if delay > 0.0 {
            // A timer, not a race: the rate has nothing to say.
            if t.rate > 0.0 {
                caveats.push(format!(
                    "{} declares both a delay and a rate; the delay is the firing rule and the rate is ignored",
                    t.id
                ));
            }
            rate = 0.0;
        } else if delay < 0.0 {
            delay = 0.0;
        }

        let mut delta: HashMap<usize, i64> = HashMap::new();
        let mut inputs = Vec::new();
        for a in m.inputs(&t.id) {
            let p = index[&a.place];
            inputs.push(CArc {
                place: p,
                weight: a.weight,
                kinetic: a.kinetic,
            });
            *delta.entry(p).or_insert(0) -= a.weight;
        }
        let mut outputs = Vec::new();
        for a in m.outputs(&t.id) {
            let p = index[&a.place];
            outputs.push(CArc {
                place: p,
                weight: a.weight,
                kinetic: a.kinetic,
            });
            *delta.entry(p).or_insert(0) += a.weight;
        }
        let mut reads = Vec::new();
        let mut inhibits = Vec::new();
        for a in m.tests(&t.id) {
            let carc = CArc {
                place: index[&a.place],
                weight: a.weight,
                kinetic: a.kinetic,
            };
            match a.typ {
                ArcType::Inhibitor => inhibits.push(carc),
                _ => reads.push(carc),
            }
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
        // Deterministic order (HashMap iteration above is not): capacity
        // bounds are consumed by `gated`, which short-circuits on the first
        // breach, so order does not change the answer, but a stable order
        // keeps `Compiled` reproducible for tests and debugging.
        caps.sort_by_key(|c| c.place);

        let mut guard_expr = None;
        if !t.guard.is_empty() {
            match guard {
                None => caveats.push(format!(
                    "no guard evaluator was supplied (Options.guard is None), so the guard on {} is not enforced; \
                     this run may fire it where the application would refuse",
                    t.id
                )),
                Some(g) if decidable_from_marking(&t.guard, m, g) => {
                    guard_expr = Some(t.guard.clone());
                }
                Some(_) => caveats.push(format!(
                    "the guard on {} needs action parameters, so it is not enforced here; \
                     this run may fire it where the application would refuse",
                    t.id
                )),
            }
        }

        out.push(CTransition {
            id: t.id.clone(),
            rate,
            inputs,
            outputs,
            reads,
            inhibits,
            caps,
            guard: guard_expr,
            delay,
        });
    }

    Ok(Compiled {
        transitions: out,
        places,
        caveats,
    })
}
