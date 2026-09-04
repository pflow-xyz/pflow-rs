//! Tied (shared) parameters: install the SAME [`crate::gradrate::SharedRateFunc`]
//! handle at more than one transition and its parameter block packs once. Mirrors
//! go-pflow's `learn/tied.go::SharedScalar` (`TiedScalar` is this crate's name for it —
//! see the API design doc, §3.5).
//!
//! [`sensitivity::build_rhs_index`](crate::sensitivity) already accumulates each
//! transition's `∂flux/∂θ` into `tr.param_range`, so once [`pack_params`] gives two tied
//! transitions the *same* range, their contributions to that shared column sum
//! automatically — no change needed there. The only gap tying closes is packing: without
//! dedup, `LearnableProblem::get_all_params` would count and pack the same handle's
//! parameters twice.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::gradrate::SharedRateFunc;
use crate::ratefunc::{RateFunc, ScalarRateFunc};

/// A learnable constant rate whose single parameter is SHARED: install the same
/// [`TiedScalar::handle`] at every transition it should drive.
///
/// `Rc<RefCell<_>>` (not `Arc`) is deliberate: this is single-threaded fitting code,
/// matching `pflow-solver`'s own non-threaded design.
pub struct TiedScalar(Rc<RefCell<ScalarRateFunc>>);

impl TiedScalar {
    pub fn new(rate: f64) -> Self {
        Self(Rc::new(RefCell::new(ScalarRateFunc::new(rate))))
    }

    /// The current shared rate.
    pub fn value(&self) -> f64 {
        self.0.borrow().get_params()[0]
    }

    /// Updates the shared rate. `&self`, not `&mut self` — interior mutability through
    /// the shared `RefCell`, so a `TiedScalar` can be updated directly even while its
    /// `handle()` is installed at several transitions.
    pub fn set(&self, v: f64) {
        self.0.borrow_mut().set_params(&[v]);
    }

    /// A handle installable at a transition via
    /// [`crate::problem::LearnableProblem::set_rate_func`]. Installing this SAME handle
    /// (an `Rc` clone, cheap) at two transitions is the whole tying mechanism —
    /// [`pack_params`] dedups by `Rc::ptr_eq` to pack its one parameter once.
    pub fn handle(&self) -> SharedRateFunc {
        self.0.clone()
    }
}

impl Clone for TiedScalar {
    /// Clones the handle, not the value: the clone shares the same underlying
    /// parameter, exactly like calling [`Self::handle`] and holding onto it.
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Dedups `rate_funcs` by `Rc::ptr_eq` (identity), packing each DISTINCT rate
/// function's parameters once, in the first-transition-wins order sorted-name
/// iteration gives — mirrors Go's sorted-name + interface-equality dedup and the JS
/// port's object-identity dedup. `Rc::ptr_eq` is strictly comparable (unlike Go's
/// `reflect.Type.Comparable()`, which a non-comparable custom rate type could defeat),
/// so there is no "silently never dedupes" failure mode to replicate here.
///
/// Returns the flat packed parameter vector and each transition's `[start, end)` range
/// into it — tied transitions share the same range.
pub fn pack_params(
    rate_funcs: &BTreeMap<String, SharedRateFunc>,
) -> (Vec<f64>, HashMap<String, (usize, usize)>) {
    let mut params = Vec::new();
    let mut ranges = HashMap::new();
    let mut seen: Vec<(SharedRateFunc, (usize, usize))> = Vec::new();

    for (name, rf) in rate_funcs {
        if let Some((_, range)) = seen.iter().find(|(s, _)| Rc::ptr_eq(s, rf)) {
            ranges.insert(name.clone(), *range);
            continue;
        }
        let start = params.len();
        params.extend(rf.borrow().get_params());
        let range = (start, params.len());
        ranges.insert(name.clone(), range);
        seen.push((rf.clone(), range));
    }

    (params, ranges)
}

/// Total learnable parameter count, deduped the same way [`pack_params`] is — a tied
/// rate function counts once no matter how many transitions install its handle.
pub fn num_params(rate_funcs: &BTreeMap<String, SharedRateFunc>) -> usize {
    let mut seen: Vec<SharedRateFunc> = Vec::new();
    let mut total = 0;
    for rf in rate_funcs.values() {
        if seen.iter().any(|s| Rc::ptr_eq(s, rf)) {
            continue;
        }
        total += rf.borrow().num_params();
        seen.push(rf.clone());
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tied_scalar_handle_shares_value() {
        let tied = TiedScalar::new(0.5);
        let h1 = tied.handle();
        let h2 = tied.handle();
        assert!(Rc::ptr_eq(&h1, &h2));

        h1.borrow_mut().set_params(&[1.25]);
        assert_eq!(tied.value(), 1.25);
        assert_eq!(h2.borrow().get_params(), vec![1.25]);

        tied.set(2.0);
        assert_eq!(h1.borrow().get_params(), vec![2.0]);
    }

    #[test]
    fn pack_params_dedups_tied_handles() {
        let tied = TiedScalar::new(0.42);
        let mut untied_a = crate::ratefunc::ScalarRateFunc::new(1.0);
        untied_a.set_params(&[1.0]); // no-op, just exercising the trait
        let untied: SharedRateFunc = Rc::new(RefCell::new(untied_a));

        let mut rate_funcs: BTreeMap<String, SharedRateFunc> = BTreeMap::new();
        rate_funcs.insert("t1".to_string(), tied.handle());
        rate_funcs.insert("t2".to_string(), tied.handle());
        rate_funcs.insert("t3".to_string(), untied.clone());

        let (params, ranges) = pack_params(&rate_funcs);
        // One packed slot for the tied pair, one for the untied transition: 2, not 3.
        assert_eq!(params.len(), 2);
        assert_eq!(ranges["t1"], ranges["t2"]);
        assert_ne!(ranges["t1"], ranges["t3"]);
        assert_eq!(num_params(&rate_funcs), 2);
    }
}
