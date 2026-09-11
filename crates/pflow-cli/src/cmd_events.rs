//! `pflow events` — lists notable occurrences recorded on a `Results` value
//! (`results.events`, populated by anything upstream that recorded one —
//! `Builder` itself never does) plus, since that list is empty for a plain
//! `pflow simulate` run, the peaks/troughs/crossings a fresh analysis finds,
//! presented on the same timeline so a bare `simulate` -> `events` pipeline
//! still shows something.

use crate::model_io::write_output;
use crate::Args;
use pflow_results::results::analysis::Analyzer;
use pflow_results::results::io::read_json;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let path = args.positional.first().ok_or("results file required")?;
    let results = read_json(path).map_err(|e| e.to_string())?;

    let mut timeline: Vec<serde_json::Value> = results
        .events
        .iter()
        .map(|e| serde_json::json!({"time": e.time, "type": e.kind, "description": e.description}))
        .collect();

    if timeline.is_empty() {
        let analysis = Analyzer::new(&results).compute_all();
        for p in &analysis.peaks {
            timeline.push(serde_json::json!({"time": p.time, "type": "peak", "description": format!("{} peaks at {}", p.variable, p.value)}));
        }
        for p in &analysis.troughs {
            timeline.push(serde_json::json!({"time": p.time, "type": "trough", "description": format!("{} bottoms at {}", p.variable, p.value)}));
        }
        for c in &analysis.crossings {
            timeline.push(serde_json::json!({"time": c.time, "type": "crossing", "description": format!("{} crosses {} at {}", c.var1, c.var2, c.value)}));
        }
        timeline.sort_by(|a, b| a["time"].as_f64().unwrap_or(0.0).partial_cmp(&b["time"].as_f64().unwrap_or(0.0)).unwrap());
    }

    write_output(&serde_json::json!({"events": timeline}), args.get("output"))
}
