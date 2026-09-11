//! `pflow expand` — unfolds a colored Shape A net into an equivalent
//! single-color net, via `pflow_core::colors::unfold`.

use crate::model_io::{model_name_from_path, petri_net_to_shape_a_json, read_file, write_output};
use crate::Args;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let path = args.positional.first().ok_or("model file required")?;
    let text = read_file(path)?;
    let net = pflow_core::from_json(text.as_bytes()).map_err(|e| format!("expand only reads Shape A JSON: {e}"))?;
    let unfolded = pflow_core::unfold(&net);
    let json = petri_net_to_shape_a_json(&unfolded.net, &model_name_from_path(path));
    write_output(&json, args.get("output"))
}
