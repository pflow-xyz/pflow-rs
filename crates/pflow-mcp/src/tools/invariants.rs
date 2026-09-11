//! `petri_invariants`: the model's minimal-support P- and T-invariants
//! (conservation laws and firing-count cycles), via
//! [`pflow_reachability::InvariantAnalyzer`].

use super::convert::parse_any_model;
use pflow_reachability::InvariantAnalyzer;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct InvariantsResult {
    p_invariants: Vec<PInvariantOut>,
    t_invariants: Vec<TInvariantOut>,
    structurally_bounded: bool,
    truncated: bool,
}

#[derive(Serialize)]
struct PInvariantOut {
    expression: String,
    coefficients: HashMap<String, i64>,
    value: i64,
}

#[derive(Serialize)]
struct TInvariantOut {
    firings: String,
    counts: HashMap<String, i64>,
}

pub fn run(model: &str) -> Result<String, String> {
    let model = parse_any_model(model)?;
    let analyzer = InvariantAnalyzer::new(&model);
    let initial = model.initial_marking();

    let p_invariants: Vec<PInvariantOut> = analyzer
        .find_p_invariants(&initial)
        .into_iter()
        .map(|inv| PInvariantOut {
            expression: inv.render(),
            coefficients: inv.coefficients.clone(),
            value: inv.value,
        })
        .collect();

    let t_invariants: Vec<TInvariantOut> = analyzer
        .find_t_invariants()
        .into_iter()
        .map(|inv| TInvariantOut { firings: inv.render(), counts: inv.counts.clone() })
        .collect();

    let p_basis = analyzer.p_invariant_basis();
    let t_basis = analyzer.t_invariant_basis();

    let out = InvariantsResult {
        p_invariants,
        t_invariants,
        structurally_bounded: analyzer.structural_boundedness(),
        truncated: p_basis.truncated || t_basis.truncated,
    };

    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conserved_pair_has_a_p_invariant() {
        // A single token bouncing between two places: p1 + p2 == 1 always.
        let dsl = r#"(schema test
            (states
                (state p1 :kind token :initial 1)
                (state p2 :kind token :initial 0)
            )
            (actions (action t1) (action t2))
            (arcs
                (arc p1 -> t1) (arc t1 -> p2)
                (arc p2 -> t2) (arc t2 -> p1)
            )
        )"#;
        let result = run(dsl).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        let pinvs = v["p_invariants"].as_array().unwrap();
        assert!(!pinvs.is_empty());
        assert_eq!(pinvs[0]["value"], 1);
    }
}
