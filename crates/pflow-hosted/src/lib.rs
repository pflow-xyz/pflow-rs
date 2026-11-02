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

use serde_json::Value;

pub mod edit;

/// The tools [`call`] answers, by their local name (the hosted name without
/// `pflow_`).
pub const TOOLS: &[&str] = &["edit"];

/// Runs one tool. `Err` is the tool's refusal, worded as the hosted tool's.
pub fn call(tool: &str, args: &Value) -> Result<Value, String> {
    match tool {
        "edit" => edit::edit(args),
        _ => Err(format!("unknown tool {tool:?}")),
    }
}
