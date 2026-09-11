//! Run configuration. Ported from go-pflow's `stochastic.Options`
//! (`stochastic/stochastic.go`).

use std::collections::HashMap;

use pflow_metamodel::{Marking, Model, RateSegment};

use super::compile::GuardEval;

/// A transition's firing rate when the model declares none.
pub const DEFAULT_RATE: f64 = 1.0;

/// See [`Options::on_fire`].
pub type OnFireFn<'a> = &'a dyn Fn(usize, f64, &str, &[i64]);

/// Every transition's firing rate, defaulting the unset ones. A model-level
/// `simulation.solver.rates` override wins over a per-transition `rate`,
/// matching how `Forecast`'s ODE path reads the same schema.
pub fn rates(m: &Model) -> HashMap<String, f64> {
    let mut out: HashMap<String, f64> = m
        .transitions
        .iter()
        .map(|t| {
            let r = if t.rate == 0.0 { DEFAULT_RATE } else { t.rate };
            (t.id.clone(), r)
        })
        .collect();
    if let Some(sim) = &m.simulation {
        if let Some(solver) = &sim.solver {
            if let Some(overrides) = &solver.rates {
                for (id, r) in overrides {
                    out.insert(id.clone(), *r);
                }
            }
        }
    }
    out
}

/// Configures a run. `Options::default()` is usable: unit rates, a one-hour
/// horizon and 60 samples.
#[derive(Clone)]
pub struct Options<'a> {
    /// How far forward to run, in the model's time unit.
    pub horizon: f64,
    /// How many points to report along the way.
    pub samples: usize,
    /// Overrides individual transition rates; unset ones come from the model.
    pub rates: HashMap<String, f64>,
    /// Makes a run reproducible. Zero picks a fixed seed (1) rather than a
    /// random one.
    pub seed: u64,
    /// How many independent sample paths to average.
    pub realizations: usize,
    /// Evaluates a transition's guard expression against a marking. `None`
    /// means every guard is caveated rather than enforced. Must return
    /// `Err` (not `Ok(false)`) when the expression references anything the
    /// marking cannot supply — `compile` uses that to classify the guard as
    /// undecidable and caveat it rather than enforce it.
    pub guard: Option<GuardEval<'a>>,
    /// A piecewise-constant rate override per transition, run as consecutive
    /// segments sharing one seed. A transition in both `rates` and
    /// `schedule` takes the schedule.
    pub schedule: HashMap<String, Vec<RateSegment>>,
    /// Called immediately after a transition fires, once per firing:
    /// realization index (0-based), firing time (segment-local under a
    /// schedule), the transition id, and the POST-firing marking in token-
    /// place order. Never called for a dead marking or after the horizon.
    pub on_fire: Option<OnFireFn<'a>>,
}

impl<'a> Default for Options<'a> {
    fn default() -> Self {
        Self {
            horizon: 0.0,
            samples: 0,
            rates: HashMap::new(),
            seed: 0,
            realizations: 0,
            guard: None,
            schedule: HashMap::new(),
            on_fire: None,
        }
    }
}

impl<'a> Options<'a> {
    /// Fills in the zero-value defaults and merges the model's own rates
    /// under the caller's overrides.
    pub fn with_defaults(mut self, m: &Model) -> Self {
        if self.horizon <= 0.0 {
            self.horizon = 1.0;
        }
        if self.samples <= 1 {
            self.samples = 60;
        }
        if self.realizations == 0 {
            self.realizations = 1;
        }
        let mut merged = rates(m);
        for (id, r) in &self.rates {
            merged.insert(id.clone(), *r);
        }
        self.rates = merged;
        self
    }
}

/// Overlays a caller's marking onto the one the model declares. Presence
/// decides, not value: a place the caller names at zero is zero, and a
/// place they omit keeps the model's initial count.
pub fn start_from(m: &Model, marking: &HashMap<String, i64>) -> Marking {
    let mut mk = m.initial_marking();
    for (p, n) in marking {
        if mk.contains_key(p) {
            mk.insert(p.clone(), *n);
        }
    }
    mk
}

/// The sample grid: `times[i] = i * (horizon / (samples - 1))`.
pub fn sample_times(opts: &Options) -> Vec<f64> {
    let step = opts.horizon / (opts.samples - 1) as f64;
    (0..opts.samples).map(|i| i as f64 * step).collect()
}
