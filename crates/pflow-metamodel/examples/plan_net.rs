//! Prints a search plan as a Petri net (`SearchPlan::net`), the plan's JSON
//! given as the one argument:
//!
//! ```text
//! cargo run -p pflow-metamodel --example plan_net -- '{"order": []}' > net.json
//! ```
//!
//! Draw it with pflow-native's `view.string_diagram`; see
//! docs/search-composition-laws.md.

use pflow_metamodel::SearchPlan;

fn main() {
    let json = std::env::args().nth(1).unwrap_or_else(|| "{}".into());
    match SearchPlan::from_json(&json).and_then(|p| p.net()) {
        Ok(net) => print!("{net}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
