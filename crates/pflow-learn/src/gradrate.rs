//! Dispatch between analytic and finite-difference rate-function gradients.

use std::cell::RefCell;
use std::rc::Rc;

use pflow_core::State;

use crate::ratefunc::RateFunc;

/// A shared, mutably-borrowable rate function, installable at more than one transition
/// (the tied-parameter mechanism) and cheap to clone as a handle.
pub type SharedRateFunc = Rc<RefCell<dyn RateFunc>>;

/// Central point that decides analytic vs. finite-difference gradient for a rate
/// function, mirroring Go's per-call type-assertion on `GradRateFunc`.
pub fn rate_grad(rf: &SharedRateFunc, state: &State, t: f64) -> (f64, Vec<f64>, State) {
    if let Some(g) = rf.borrow().eval_grad(state, t) {
        return g;
    }
    fd_rate_grad(rf, state, t)
}

/// Central finite differences of `eval` alone (never of a whole solve): step size
/// `h = 1e-6 * (1 + |value|)` per parameter/state entry. Perturbs a copy of the
/// parameters (restored via `set_params` before returning) and a copy of the state
/// (the caller's map is never mutated).
pub fn fd_rate_grad(rf: &SharedRateFunc, state: &State, t: f64) -> (f64, Vec<f64>, State) {
    let value = rf.borrow().eval(state, t);

    let params = rf.borrow().get_params();
    let mut dparams = vec![0.0; params.len()];
    for i in 0..params.len() {
        let h = 1e-6 * (1.0 + params[i].abs());
        let mut p_plus = params.clone();
        p_plus[i] += h;
        let mut p_minus = params.clone();
        p_minus[i] -= h;

        rf.borrow_mut().set_params(&p_plus);
        let f_plus = rf.borrow().eval(state, t);
        rf.borrow_mut().set_params(&p_minus);
        let f_minus = rf.borrow().eval(state, t);
        rf.borrow_mut().set_params(&params);

        dparams[i] = (f_plus - f_minus) / (2.0 * h);
    }

    let mut dstate = State::new();
    for (place, &v) in state {
        let h = 1e-6 * (1.0 + v.abs());
        let mut s_plus = state.clone();
        s_plus.insert(place.clone(), v + h);
        let mut s_minus = state.clone();
        s_minus.insert(place.clone(), v - h);

        let f_plus = rf.borrow().eval(&s_plus, t);
        let f_minus = rf.borrow().eval(&s_minus, t);
        dstate.insert(place.clone(), (f_plus - f_minus) / (2.0 * h));
    }

    (value, dparams, dstate)
}
