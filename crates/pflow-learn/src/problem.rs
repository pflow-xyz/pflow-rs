//! [`LearnableProblem`]: a Petri net ODE problem whose per-transition rates are
//! [`RateFunc`] objects with learnable parameters, rather than fixed constants.

use std::collections::{BTreeMap, HashMap};

use pflow_core::net::PetriNet;
use pflow_core::State;
use pflow_solver::methods::Solver;
use pflow_solver::ode::{integrate_vec, Options, Solution};

use crate::error::LearnError;
use crate::gradrate::SharedRateFunc;

/// A Petri net ODE problem parameterized by per-transition [`crate::ratefunc::RateFunc`]s.
///
/// `state_labels` is always **sorted** — a deliberate deviation from go-pflow, whose own
/// map-order iteration for this vector is genuinely nondeterministic run-to-run. Rust has
/// no such excuse, and the augmented sensitivity vector's row order must be stable.
pub struct LearnableProblem {
    pub net: PetriNet,
    pub u0: State,
    pub tspan: [f64; 2],
    rate_funcs: BTreeMap<String, SharedRateFunc>,
    state_labels: Vec<String>,
    state_index: HashMap<String, usize>,
}

impl LearnableProblem {
    /// Builds a problem over `net`'s places (in sorted order) with no rate functions
    /// installed yet — every transition is inert (rate 0) until [`Self::set_rate_func`]
    /// is called for it.
    pub fn new(net: PetriNet, u0: State, tspan: [f64; 2]) -> Self {
        let mut state_labels: Vec<String> = net.places.keys().cloned().collect();
        state_labels.sort();
        let state_index: HashMap<String, usize> = state_labels
            .iter()
            .enumerate()
            .map(|(i, l)| (l.clone(), i))
            .collect();
        Self {
            net,
            u0,
            tspan,
            rate_funcs: BTreeMap::new(),
            state_labels,
            state_index,
        }
    }

    /// Installs (or replaces) the rate function for one transition. Installing the same
    /// `SharedRateFunc` handle at two transitions is the tied-parameter mechanism (see
    /// [`crate::tied::TiedScalar`]) — [`Self::get_all_params`]/[`Self::num_params`]
    /// dedup by `Rc::ptr_eq` via [`crate::tied::pack_params`], so a tied handle's
    /// parameters are packed once no matter how many transitions install it.
    pub fn set_rate_func(&mut self, transition: &str, rf: SharedRateFunc) {
        self.rate_funcs.insert(transition.to_string(), rf);
    }

    pub fn state_labels(&self) -> &[String] {
        &self.state_labels
    }

    pub fn state_index(&self) -> &HashMap<String, usize> {
        &self.state_index
    }

    pub(crate) fn rate_funcs(&self) -> &BTreeMap<String, SharedRateFunc> {
        &self.rate_funcs
    }

    /// Dense initial state vector in `state_labels()` order. Errors if any state label
    /// (i.e. any net place) has no entry in `u0`.
    pub fn dense_u0(&self) -> Result<Vec<f64>, LearnError> {
        self.state_labels
            .iter()
            .map(|l| {
                self.u0
                    .get(l)
                    .copied()
                    .ok_or_else(|| LearnError::PlaceMissingFromU0(l.clone()))
            })
            .collect()
    }

    /// Total number of learnable parameters across every DISTINCT installed rate
    /// function, in sorted-transition-name order (transitions with no rate function
    /// installed contribute none; a tied handle installed at several transitions
    /// counts once — see [`crate::tied::pack_params`]).
    pub fn num_params(&self) -> usize {
        crate::tied::num_params(&self.rate_funcs)
    }

    /// Flat parameter vector and the `[start, end)` range each transition occupies in
    /// it, both in sorted-transition-name order. Transitions sharing a tied
    /// [`crate::gradrate::SharedRateFunc`] handle share the same range — see
    /// [`crate::tied::pack_params`].
    pub fn get_all_params(&self) -> (Vec<f64>, HashMap<String, (usize, usize)>) {
        crate::tied::pack_params(&self.rate_funcs)
    }

    /// Overwrites every installed rate function's parameters from a flat vector packed
    /// in the same order [`Self::get_all_params`] returns.
    ///
    /// Takes `&self`, not `&mut self`: the mutation happens through each rate function's
    /// `RefCell`, not through `self.rate_funcs` itself, which lets an optimizer's
    /// objective closure hold only a shared borrow of the problem — exactly what
    /// [`crate::fit::fit`] needs to build several closures over the same `prob` at once.
    ///
    /// Looks up each transition's `[start, end)` range via [`crate::tied::pack_params`]
    /// rather than walking `params` sequentially, so a tied handle installed at several
    /// transitions is written from the same slice each time (idempotently) instead of
    /// being fed successive, wrong slices.
    pub fn set_all_params(&self, params: &[f64]) {
        let (_, ranges) = crate::tied::pack_params(&self.rate_funcs);
        for (name, rf) in &self.rate_funcs {
            let (start, end) = ranges[name];
            rf.borrow_mut().set_params(&params[start..end]);
        }
    }

    /// Builds the dense vector of the current state, evaluated as a `State` map for
    /// [`crate::ratefunc::RateFunc::eval`] calls.
    pub(crate) fn vec_to_state(&self, v: &[f64]) -> State {
        self.state_labels
            .iter()
            .zip(v.iter())
            .map(|(l, &x)| (l.clone(), x))
            .collect()
    }

    /// Solves the plain (unaugmented) ODE: mass-action kinetics with each transition's
    /// rate evaluated via its installed [`crate::ratefunc::RateFunc`] (rate 0 for any
    /// transition with none installed).
    pub fn solve(&self, solver: &Solver, opts: &Options) -> Result<Solution, LearnError> {
        let u0 = self.dense_u0()?;
        let n = u0.len();
        let index = crate::sensitivity::build_rhs_index(self)?;

        let f = move |t: f64, u: &[f64]| -> Vec<f64> {
            let mut du = vec![0.0; n];
            let state = index.vec_to_state(u);
            for tr in &index.transitions {
                if !tr.inputs.iter().all(|&(idx, _)| u[idx] > 0.0) {
                    continue;
                }
                let k = tr.rate_fn.borrow().eval(&state, t);
                let g: f64 = tr.inputs.iter().map(|&(idx, _)| u[idx]).product();
                let flux = k * g;
                if flux < 0.0 {
                    continue;
                }
                for &(idx, w) in &tr.inputs {
                    du[idx] -= flux * w;
                }
                for &(idx, w) in &tr.outputs {
                    du[idx] += flux * w;
                }
            }
            du
        };

        let (t_out, u_out) = integrate_vec(&f, &u0, self.tspan, solver, opts);
        let u = u_out.iter().map(|v| self.vec_to_state(v)).collect();
        Ok(Solution {
            t: t_out,
            u,
            state_labels: self.state_labels.clone(),
        })
    }
}
