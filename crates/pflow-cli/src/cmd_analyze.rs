use crate::model_io::write_output;
use crate::Args;
use pflow_results::results::analysis::Analyzer;
use pflow_results::results::io::{read_json, write_json};

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let path = args.positional.first().ok_or("results file required")?;
    let mut results = read_json(path).map_err(|e| e.to_string())?;

    let analysis = Analyzer::new(&results).compute_all();
    let out = serde_json::to_value(&analysis).map_err(|e| e.to_string())?;

    if let Some(output) = args.get("output") {
        results.analysis = Some(analysis);
        write_json(&results, output).map_err(|e| e.to_string())?;
    }

    write_output(&out, None)
}
