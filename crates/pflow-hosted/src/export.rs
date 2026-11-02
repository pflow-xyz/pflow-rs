//! `export`: hosted `pflow_export` (`mcpExport`).
//!
//! `format` `json` (the default) answers `{net, cid}`: the net as given (after
//! Go's JSON round trip, so `1.0` reads `1`) and the CID it seals to, which,
//! as hosted, is left out when the net does not seal (here also when the local
//! sealer cannot reproduce pflow.xyz's CID byte for byte; see `cid`).
//!
//! `format` `svg` answers `{format: "svg", svg}` drawn by
//! `pflow_visualization::render_petri_net_svg`, go-pflow's renderer, at the
//! net's own coordinates. The hosted server draws with pflow-xyz's
//! `internal/svg`, a different renderer, so the bytes differ: the same nodes
//! and arcs, not the same markup. `layout` (force-atlas-2, circular,
//! hierarchical) is accepted and not applied: the local drawing always uses
//! the coordinates in the net, which is also what the hosted server falls
//! back to for a layout it does not know.

use serde_json::{json, Value};

use crate::cid::seal;
use crate::net::{net_arg, petri_net};

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let doc = net_arg(args)?;
    match args.get("format").and_then(Value::as_str).unwrap_or("") {
        "" | "json" => {
            let mut out = json!({"net": Value::Object(doc.clone())});
            if let Ok(cid) = seal(&doc) {
                out["cid"] = json!(cid);
            }
            Ok(out)
        }
        "svg" => {
            let svg = pflow_visualization::render_petri_net_svg(&petri_net(&doc));
            Ok(json!({"format": "svg", "svg": svg}))
        }
        _ => Err("format: json or svg".into()),
    }
}
