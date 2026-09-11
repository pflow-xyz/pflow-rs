use crate::model_io::write_output;
use crate::Args;
use pflow_results::results::io::read_json;
use std::collections::HashSet;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let a_path = args.positional.first().ok_or("two results files required")?;
    let b_path = args.positional.get(1).ok_or("two results files required")?;
    let a = read_json(a_path).map_err(|e| e.to_string())?;
    let b = read_json(b_path).map_err(|e| e.to_string())?;

    let places: HashSet<&String> = a.data.summary.final_state.keys().chain(b.data.summary.final_state.keys()).collect();
    let mut deltas = serde_json::Map::new();
    for p in places {
        let av = a.data.summary.final_state.get(p).copied().unwrap_or(0.0);
        let bv = b.data.summary.final_state.get(p).copied().unwrap_or(0.0);
        let percent_change: Option<f64> = if av != 0.0 { Some((bv - av) / av * 100.0) } else { None };
        deltas.insert(p.clone(), serde_json::json!({"a": av, "b": bv, "delta": bv - av, "percentChange": percent_change}));
    }

    let out = serde_json::json!({
        "a": {"name": a.model.name, "finalTime": a.data.summary.final_time},
        "b": {"name": b.model.name, "finalTime": b.data.summary.final_time},
        "finalStateDeltas": deltas,
    });
    write_output(&out, args.get("output"))
}
