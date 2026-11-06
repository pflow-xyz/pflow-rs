//! The pflow.xyz/mcp hosted tools, run locally on pflow-rs.
//!
//! pflow.xyz/mcp (pflow-xyz `cmd/webserver/mcp*.go`) answers its tools on
//! go-pflow, and its workbench and composition tools are ports of
//! pflow-native, which runs on pflow-rs. This crate gives the same tools the
//! same names (without the `pflow_` prefix), arguments, refusals and answer
//! shapes over pflow-rs's own engines, so `@pflow-xyz/pflow-mcp` can answer
//! them offline. Each tool is stateless: the net (or composition) travels in
//! the arguments, the answer travels back.
//!
//! [`call`] is the whole surface: plain Rust over `serde_json::Value`, tested
//! natively (`cargo test -p pflow-hosted`, including pflow-xyz's native
//! goldens under `tests/fixtures/`); pflow-wasm's `hosted` feature exposes it
//! to JavaScript as one function.

mod bundle;

use serde_json::Value;

pub mod edit;
pub mod sde;
mod wbnet;
pub mod what_if;
// Single-net tools (the hosted `mcp.go`/`mcp_registry.go` ones); `net`
// reads their shared `net` argument.
mod check;
mod cid;
mod convert;
mod export;
mod incidence;
mod info;
mod invariants;
mod net;
mod validate;
mod verify;

/// The tools [`call`] answers, by their local name (the hosted name without
/// `pflow_`).
pub const TOOLS: &[&str] = &[
    "edit",
    "sde",
    "what_if",
    "bundle_example",
    "bundle_validate",
    "bundle_add_link",
    "bundle_candidates",
    "bundle_flatten",
    "bundle_run",
    "bundle_incidence",
    "bundle_net",
    "bundle_parts",
    "bundle_prove",
    "bundle_check_proof",
    "check",
    "cid",
    "incidence",
    "invariants",
    "validate",
    "verify",
    "export",
    "info",
];

/// Runs one tool. `Err` is the tool's refusal, worded as the hosted tool's.
pub fn call(tool: &str, args: &Value) -> Result<Value, String> {
    match tool {
        "edit" => edit::edit(args),
        "sde" => sde::run(args),
        "what_if" => what_if::run(args),
        "bundle_example" | "bundle_validate" | "bundle_add_link" | "bundle_candidates"
        | "bundle_flatten" | "bundle_run" | "bundle_incidence" | "bundle_net" | "bundle_parts" | "bundle_prove" | "bundle_check_proof" => bundle::call(tool, args),
        "check" => check::run(args),
        "cid" => cid::run(args),
        "incidence" => incidence::run(args),
        "invariants" => invariants::run(args),
        "validate" => validate::run(args),
        "verify" => verify::run(args),
        "export" => export::run(args),
        "info" => info::run(args),
        // Not a pflow.xyz tool, so not in TOOLS: load_net's converter (convert.rs).
        "net_to_jsonld" => convert::run(args),
        _ => Err(format!("unknown tool {tool:?}")),
    }
}
