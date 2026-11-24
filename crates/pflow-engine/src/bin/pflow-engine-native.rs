//! The engine run natively, for the verdict check and for native-vs-WASM
//! comparisons (`scripts/check-wasm-repro.sh`). Not part of the module.
//!
//!   pflow-engine-native cid <model.jsonld>
//!   pflow-engine-native run <model.jsonld> <cid> <op> [args-json]
//!   pflow-engine-native cases <cases.json>      # every case, as check-verdicts.mjs prints them
use std::process::ExitCode;

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let read = |p: &str| std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let out = match a.first().map(String::as_str) {
        Some("cid") if a.len() == 2 => pflow_engine::cid_of(&read(&a[1])),
        Some("run") if a.len() >= 4 => pflow_engine::Model::load(&read(&a[1]), &a[2])
            .and_then(|m| m.run(&a[3], a.get(4).map(String::as_str).unwrap_or(""))),
        Some("cases") if a.len() == 2 => {
            let root = std::path::Path::new(&a[1]).parent().unwrap().to_path_buf();
            Ok(pflow_engine::cases::run_all(&read(&a[1]), &|p| std::fs::read(root.join(p)).map_err(|e| e.to_string())))
        }
        _ => Err("usage: pflow-engine-native cid <model> | run <model> <cid> <op> [args] | cases <cases.json>".into()),
    };
    match out {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
