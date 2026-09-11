//! `petri_dataset`: generates a synthetic event log from a model by
//! playing out the default stochastic engine ([`pflow_solver::stochastic`])
//! for `cases` independent realizations and recording every firing.
//! Deterministic: same model, same seed, same cases -> same bytes. Returns
//! CSV (`case_id,activity,timestamp`), the shape `petri_conformance`'s
//! companion event-log format and `pflow-eventlog`'s CSV reader both
//! understand.
//!
//! **Approximation, not a port.** The private `sim`/petri-pilot fork's
//! `sim_dataset` walks a case-per-arrival playout keyed to a declared
//! arrival transition (`eventgen.Playout`), which has no counterpart
//! ported into this workspace. This tool instead treats each of the
//! `cases` realizations as one case and records its own firing sequence in
//! order — a real, seeded, reproducible synthetic log, just not a
//! byte-identical one.

use super::convert::parse_any_model;
use pflow_metamodel::Model;
use pflow_solver::stochastic::{simulate, start_from, Options};
use std::cell::RefCell;
use std::collections::HashMap;

pub fn run(model: &str, cases: usize, hours: f64, seed: u64) -> Result<String, String> {
    let model: Model = parse_any_model(model)?;
    let marking = start_from(&model, &HashMap::new());

    // Collected as (case, time, transition) — one realization is one case.
    let events: RefCell<Vec<(usize, f64, String)>> = RefCell::new(Vec::new());
    let on_fire = |realization: usize, t: f64, transition: &str, _marking: &[i64]| {
        events.borrow_mut().push((realization, t, transition.to_string()));
    };

    let opts = Options {
        horizon: hours,
        samples: 2,
        rates: HashMap::new(),
        seed,
        realizations: cases,
        guard: None,
        schedule: HashMap::new(),
        on_fire: Some(&on_fire),
    }
    .with_defaults(&model);

    simulate(&model, &marking, &opts).map_err(|e| e.to_string())?;

    let mut rows = events.into_inner();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));

    let mut out = String::from("case_id,activity,timestamp\n");
    for (case, t, transition) in rows {
        out.push_str(&format!("case-{case},{transition},{t:.6}\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsl() -> &'static str {
        r#"(schema chain
            (states
                (state a :kind token :initial 5)
                (state b :kind token :initial 0)
            )
            (actions (action ab))
            (arcs (arc a -> ab) (arc ab -> b))
        )"#
    }

    #[test]
    fn deterministic_for_same_seed() {
        let a = run(dsl(), 3, 1.0, 42).unwrap();
        let b = run(dsl(), 3, 1.0, 42).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn emits_a_header_and_rows() {
        let out = run(dsl(), 2, 1.0, 1).unwrap();
        assert!(out.starts_with("case_id,activity,timestamp\n"));
        assert!(out.lines().count() > 1, "expected at least one event row");
    }
}
