pub mod analyze;
pub mod build;
pub mod equilibrium;
pub mod fire;
pub mod simulate;
pub mod stochastic;
pub mod validate;

use pflow_core::{PetriNet, State};
use pflow_dsl::parse_schema;
use pflow_tokenmodel::Schema;
use std::collections::HashMap;

/// Parse a model string (DSL S-expression or JSON) into a Schema.
pub fn parse_model(model: &str) -> Result<Schema, String> {
    let trimmed = model.trim();
    if trimmed.starts_with('(') {
        parse_schema(trimmed)
    } else {
        serde_json::from_str::<Schema>(trimmed).map_err(|e| format!("JSON parse error: {e}"))
    }
}

/// Convert a Schema into a PetriNet suitable for ODE simulation.
/// Token states become places; actions become transitions; arcs map with weight 1.0.
pub fn schema_to_petri_net(schema: &Schema) -> PetriNet {
    let mut net = PetriNet::new();

    for st in &schema.states {
        if st.is_token() {
            net.add_place(
                &st.id,
                vec![st.initial_tokens() as f64],
                vec![],
                0.0,
                0.0,
                None,
            );
        }
    }

    for action in &schema.actions {
        net.add_transition(&action.id, "", 0.0, 0.0, None);
    }

    for arc in &schema.arcs {
        // Only include arcs connecting token states
        let is_input = schema.state_by_id(&arc.source).map_or(false, |s| s.is_token());
        let is_output = schema.state_by_id(&arc.target).map_or(false, |s| s.is_token());
        if is_input || is_output {
            net.add_arc(&arc.source, &arc.target, vec![1.0], false);
        }
    }

    net
}

/// Build default rates (1.0 for every transition).
pub fn default_rates(schema: &Schema) -> HashMap<String, f64> {
    schema
        .actions
        .iter()
        .map(|a| (a.id.clone(), 1.0))
        .collect()
}

/// Get the initial state from a PetriNet.
pub fn initial_state(net: &PetriNet) -> State {
    net.set_state(None)
}
