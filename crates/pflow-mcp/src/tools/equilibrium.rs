use super::{default_rates, initial_state, parse_model, schema_to_petri_net};
use pflow_solver::{methods, solve_until_equilibrium, EquilibriumOptions, Options, Problem};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct EquilibriumResult {
    reached: bool,
    time: f64,
    state: HashMap<String, f64>,
    max_change: f64,
    steps: usize,
    reason: String,
}

pub fn run(model: &str, t_max: f64) -> Result<String, String> {
    let schema = parse_model(model)?;
    let net = schema_to_petri_net(&schema);
    let state = initial_state(&net);
    let rates = default_rates(&schema);

    let prob = Problem::new(net, state, [0.0, t_max], rates);
    let opts = Options::default_opts();
    let eq_opts = EquilibriumOptions::default_opts();

    let (_sol, eq) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);

    let result = EquilibriumResult {
        reached: eq.reached,
        time: eq.time,
        state: eq.state,
        max_change: eq.max_change,
        steps: eq.steps,
        reason: eq.reason,
    };

    serde_json::to_string_pretty(&result).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equilibrium_sir() {
        // SIR model reaches equilibrium
        let dsl = r#"(schema sir
            (states
                (state S :kind token :initial 999)
                (state I :kind token :initial 1)
                (state R :kind token :initial 0)
            )
            (actions
                (action infect)
                (action recover)
            )
            (arcs
                (arc S -> infect)
                (arc I -> infect)
                (arc infect -> I)
                (arc infect -> I)
                (arc I -> recover)
                (arc recover -> R)
            )
        )"#;
        let result = run(dsl, 100.0).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["reached"].as_bool().unwrap());
    }
}
