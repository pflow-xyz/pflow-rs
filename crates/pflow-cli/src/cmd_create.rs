use crate::model_io::{petri_net_to_shape_a_json, write_output};
use crate::Args;
use pflow_compose::templates;

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &[]);
    let name = args.positional.first().ok_or_else(|| {
        format!("template name required. Available: {}", templates::list().join(", "))
    })?;

    let mut params: templates::Params = templates::Params::new();
    if let Some(p) = args.get("params") {
        for kv in p.split(',') {
            let kv = kv.trim();
            if kv.is_empty() {
                continue;
            }
            let (k, v) = kv.split_once('=').ok_or_else(|| format!("bad --params entry {kv:?}, expected key=value"))?;
            let v: f64 = v.parse().map_err(|_| format!("bad numeric value in {kv:?}"))?;
            params.insert(k.to_string(), v);
        }
    }

    let net = templates::generate(name, &params)?;
    let json = petri_net_to_shape_a_json(&net, name);
    write_output(&json, args.get("output"))
}
