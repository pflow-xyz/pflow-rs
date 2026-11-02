//! `incidence`: hosted `pflow_incidence` (`mcpIncidence`). The incidence
//! matrix `C = post - pre` of the color-expanded net, rows places and columns
//! transitions, both sorted by id; inhibitor and read arcs move nothing and
//! are left out (`pflow_reachability::InvariantAnalyzer::incidence_matrix`,
//! the port of go-pflow's `InvariantAnalyzer.IncidenceMatrix`).

use pflow_reachability::InvariantAnalyzer;
use serde_json::{json, Value};

use crate::net::{expand, model_of, net_arg, petri_net};

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let doc = net_arg(args)?;
    let (expanded, _) = expand(&petri_net(&doc));
    let model = model_of(&expanded);
    let (matrix, places, transitions) = InvariantAnalyzer::new(&model).incidence_matrix();
    Ok(json!({"places": places, "transitions": transitions, "matrix": matrix}))
}
