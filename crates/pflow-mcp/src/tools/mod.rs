pub mod analyze;
pub mod build;
pub mod canonical;
pub mod conformance;
pub mod convert;
pub mod dataset;
pub mod diff;
pub mod equilibrium;
pub mod extend;
pub mod fire;
pub mod invariants;
pub mod lumping;
pub mod scenario;
pub mod simulate;
pub mod stochastic;
pub mod validate;
pub mod verify;

use pflow_core::{PetriNet, State};
use pflow_dsl::parse_schema;
use pflow_tokenmodel::{ArcType, Schema};
use std::collections::HashMap;

/// Parse a model string into a Schema. Accepts every wire shape
/// [`convert::parse_any_model`] does (DSL, tokenmodel Schema JSON, Shape A
/// editor JSON, Shape B `pflow_metamodel::Model` JSON) — the DSL and native
/// Schema-JSON cases short-circuit straight to a `Schema`; Shape A/B route
/// through [`convert::parse_any_model`] and [`convert::model_to_schema`],
/// which drops what `Schema` cannot express (capacity, rate, delay,
/// stages/schedules, parameters, access — see that function's doc).
pub fn parse_model(model: &str) -> Result<Schema, String> {
    let trimmed = model.trim();
    if trimmed.starts_with('(') {
        return parse_schema(trimmed);
    }
    if let Ok(schema) = serde_json::from_str::<Schema>(trimmed) {
        return Ok(schema);
    }
    let model = convert::parse_any_model(trimmed)?;
    Ok(convert::model_to_schema(&model))
}

/// Convert a Schema into a PetriNet for the ODE and the SSA.
///
/// Token states become places and actions become transitions. Arcs keep their
/// declared weight and type. Contextual arcs use the pflow.xyz convention
/// (`PetriNet::arc_kind`): an inhibitor stays `place -> transition`, a read arc
/// is drawn `transition -> place`. A contextual arc the firing rule would not
/// test, one not drawn from a token place, is left out, as
/// [`convert::model_to_petri_net`] does.
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
        let is_input = schema.state_by_id(&arc.source).is_some_and(|s| s.is_token());
        let is_output = schema.state_by_id(&arc.target).is_some_and(|s| s.is_token());
        let w = arc.effective_weight() as f64;
        match arc.typ {
            ArcType::Normal if is_input || is_output => {
                net.add_arc(&arc.source, &arc.target, vec![w], false);
            }
            ArcType::Inhibitor if is_input => {
                net.add_arc(&arc.source, &arc.target, vec![w], true);
            }
            ArcType::Read if is_input => {
                net.add_arc(&arc.target, &arc.source, vec![w], true);
            }
            ArcType::Normal | ArcType::Inhibitor | ArcType::Read => {}
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
