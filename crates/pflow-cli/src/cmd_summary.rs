use crate::model_io::write_output;
use crate::Args;
use pflow_results::results::io::read_json;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let path = args.positional.first().ok_or("results file required")?;
    let results = read_json(path).map_err(|e| e.to_string())?;

    let out = serde_json::json!({
        "name": results.model.name,
        "places": results.model.places.len(),
        "transitions": results.model.transitions.len(),
        "solver": results.metadata.solver,
        "status": results.metadata.status,
        "computeTime": results.metadata.compute_time,
        "points": results.data.summary.points,
        "finalTime": results.data.summary.final_time,
        "finalState": results.data.summary.final_state,
    });
    write_output(&out, args.get("output"))
}
