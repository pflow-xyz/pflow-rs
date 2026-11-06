//! `invariants`: hosted `pflow_invariants` (`mcpInvariants`). Minimal
//! semi-positive P-invariants valued at the net's marking and T-invariants, by
//! Farkas elimination on the color-expanded net's incidence matrix
//! (`pflow_reachability::InvariantAnalyzer`, the port of go-pflow's).
//!
//! Shapes as hosted: `p_invariants` is a list of `{equation, coefficients,
//! value}` (`equation` is go-pflow's `Invariant.String`, `"a + 2*b == 3"`),
//! `t_invariants` a list of `{firing_multiset, counts}` (`"{t1, 2*t2}"`); an
//! empty list is `[]` (hosted answers `null`, Go's nil slice; bug B7).

use pflow_reachability::{InvariantAnalyzer, Marking};
use serde_json::{json, Map, Value};

use crate::net::{expand, int_map, model_of, net_arg, petri_net, start_marking};

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let doc = net_arg(args)?;
    crate::net::whole_weights(&doc)?;
    let kind = match args.get("kind").and_then(Value::as_str).unwrap_or("") {
        "" => "both",
        k @ ("p" | "t" | "both") => k,
        _ => return Err("kind: p, t or both".into()),
    };
    let (expanded, _) = expand(&petri_net(&doc));
    let model = model_of(&expanded);
    let an = InvariantAnalyzer::new(&model);
    let mut out = Map::new();
    if kind != "t" {
        let initial: Marking = start_marking(&expanded).into_iter().collect();
        let list: Vec<Value> = an
            .find_p_invariants(&initial)
            .iter()
            .map(|inv| json!({"equation": inv.render(), "coefficients": int_map(&inv.coefficients), "value": inv.value}))
            .collect();
        out.insert("p_invariants".into(), nil_if_empty(list));
        out.insert(
            "p_truncated".into(),
            json!(an.p_invariant_basis().truncated),
        );
        out.insert("conservative".into(), json!(conservative(&an)));
    }
    if kind != "p" {
        let list: Vec<Value> = an
            .find_t_invariants()
            .iter()
            .map(|inv| json!({"firing_multiset": inv.render(), "counts": int_map(&inv.counts)}))
            .collect();
        out.insert("t_invariants".into(), nil_if_empty(list));
        out.insert(
            "t_truncated".into(),
            json!(an.t_invariant_basis().truncated),
        );
    }
    out.insert(
        "structurally_bounded".into(),
        json!(an.structural_boundedness()),
    );
    Ok(Value::Object(out))
}

/// go-pflow's `CheckConservation`: every column of the incidence matrix sums
/// to zero (the all-ones vector is a P-invariant).
fn conservative(an: &InvariantAnalyzer) -> bool {
    let (matrix, places, transitions) = an.incidence_matrix();
    (0..transitions.len()).all(|t| (0..places.len()).map(|p| matrix[p][t]).sum::<i64>() == 0)
}

/// An empty list stays `[]` (hosted marshals Go's nil slice as `null`: a
/// documented difference, bug B7).
fn nil_if_empty(list: Vec<Value>) -> Value {
    Value::Array(list)
}
