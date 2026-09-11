use crate::model_io::{load_petri_net, model_name_from_path, write_output};
use crate::Args;
use pflow_results::results::builder::Builder;
use pflow_results::results::io::to_json;
use pflow_solver::{methods, solve, Options, Problem};
use std::collections::HashMap;
use std::time::Instant;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &["analyze"]);
    let path = args.positional.first().ok_or("model file required")?;
    let net = load_petri_net(path)?;

    let t_start = args.get_f64("start", 0.0);
    let t_end = args.get_f64("time", 100.0);
    let downsample = args.get_usize("downsample", 150);
    let name = args.get("name").map(String::from).unwrap_or_else(|| model_name_from_path(path));

    let mut state = net.set_state(None);
    if let Some(spec) = args.get("initial") {
        for kv in spec.split(',') {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("bad --initial entry {kv:?}"))?;
            state.insert(k.to_string(), v.parse().map_err(|_| format!("bad value in {kv:?}"))?);
        }
    }

    let mut rates: HashMap<String, f64> = net.transitions.keys().map(|id| (id.clone(), 1.0)).collect();
    if let Some(spec) = args.get("rates") {
        for kv in spec.split(',') {
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("bad --rates entry {kv:?}"))?;
            rates.insert(k.to_string(), v.parse().map_err(|_| format!("bad value in {kv:?}"))?);
        }
    }

    let prob = Problem::new(net.clone(), state.clone(), [t_start, t_end], rates.clone());
    let opts = Options::default_opts();
    let started = Instant::now();
    let sol = solve(&prob, &methods::tsit5(), &opts);
    let compute_time = started.elapsed().as_secs_f64();

    let results = Builder::new()
        .with_model(&net, &name)
        .with_simulation(&state, &rates, [t_start, t_end], Some(&opts))
        .with_solution(&sol, "Tsit5", compute_time, downsample)
        .build();

    let output = args.get("output").ok_or("--output is required")?;
    std::fs::write(output, to_json(&results).map_err(|e| e.to_string())?).map_err(|e| format!("write {output}: {e}"))?;

    write_output(
        &serde_json::json!({"status": "ok", "points": results.data.summary.points, "finalTime": results.data.summary.final_time, "output": output}),
        None,
    )
}
