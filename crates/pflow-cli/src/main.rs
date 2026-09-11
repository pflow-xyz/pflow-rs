//! `pflow` — command-line parity with go-pflow's `cmd/pflow`
//! (ROADMAP.md Phase 6). Subcommands and flags mirror the Go CLI one for
//! one where a Rust library counterpart exists; `sweep` is not implemented
//! (no `pflow-sweep`-shaped optimizer is ported — `pflow-results::sweep`
//! covers post-hoc analysis of an already-run parameter sweep, not driving
//! one, and building that driver is out of this phase's scope) and
//! `plot`/`visualize` are real, backed by `pflow-visualization`, which
//! landed alongside this CLI.

mod model_io;

use std::env;
use std::process::ExitCode;

mod cmd_analyze;
mod cmd_compare;
mod cmd_create;
mod cmd_events;
mod cmd_expand;
mod cmd_plot;
mod cmd_simulate;
mod cmd_summary;
mod cmd_validate;
mod cmd_verify;
mod cmd_visualize;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::FAILURE;
    }

    let (command, rest) = (args[0].as_str(), &args[1..]);
    let result = match command {
        "create" => cmd_create::run(rest),
        "validate" => cmd_validate::run(rest),
        "verify" => cmd_verify::run(rest),
        "expand" => cmd_expand::run(rest),
        "simulate" => cmd_simulate::run(rest),
        "analyze" => cmd_analyze::run(rest),
        "summary" => cmd_summary::run(rest),
        "compare" => cmd_compare::run(rest),
        "events" => cmd_events::run(rest),
        "visualize" => cmd_visualize::run(rest),
        "plot" => cmd_plot::run(rest),
        "help" | "-h" | "--help" => {
            print_usage();
            Ok(())
        }
        "version" | "-v" | "--version" => {
            println!("pflow-rs version {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => {
            eprintln!("Unknown command: {other}\n");
            print_usage();
            Err(String::new())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("Error: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!(
        r#"pflow - Petri net modeling and simulation tool (Rust port; parity with go-pflow's cmd/pflow)

Usage:
  pflow <command> [options]

Commands:
  create     Create a model from a template (sir, seir, queue, producer-consumer, workflow)
  validate   Validate model structure (structure, connectivity, deadlock, unbounded, conservation)
  verify     Check declarative properties (proved/refuted/unknown + counterexample)
  expand     Unfold a colored Shape A net into an equivalent single-color net
  simulate   Run ODE simulation from a Petri net model
  analyze    Compute peaks/troughs/crossings/steady-state/conservation from simulation results
  summary    Display a quick summary of simulation results
  compare    Compare two simulation results (final-state deltas)
  events     List notable occurrences recorded in simulation results
  visualize  Generate SVG visualization of Petri net structure
  plot       Generate SVG plot from simulation results
  help       Show this help message
  version    Show version information

Examples:
  pflow create sir --output sir.json
  pflow validate model.json --reachability
  pflow simulate model.json --time 100 --output results.json
  pflow analyze results.json
  pflow visualize model.json --output structure.svg
  pflow plot results.json --output plot.svg
  pflow compare baseline.json variant.json
"#
    );
}

/// Pulls `--flag value` and `--flag=value` pairs plus positional args out of
/// a flat arg list — go's `flag` package does the split-on-`=` form
/// automatically; this reproduces just enough of that for this CLI's flags.
pub struct Args {
    pub positional: Vec<String>,
    flags: std::collections::HashMap<String, String>,
    bool_flags: std::collections::HashSet<String>,
}

impl Args {
    pub fn parse(raw: &[String], known_bools: &[&str]) -> Self {
        let mut positional = Vec::new();
        let mut flags = std::collections::HashMap::new();
        let mut bool_flags = std::collections::HashSet::new();
        let mut i = 0;
        while i < raw.len() {
            let a = &raw[i];
            if let Some(rest) = a.strip_prefix("--") {
                if let Some((k, v)) = rest.split_once('=') {
                    flags.insert(k.to_string(), v.to_string());
                } else if known_bools.contains(&rest) {
                    bool_flags.insert(rest.to_string());
                } else if i + 1 < raw.len() {
                    flags.insert(rest.to_string(), raw[i + 1].clone());
                    i += 1;
                } else {
                    bool_flags.insert(rest.to_string());
                }
            } else {
                positional.push(a.clone());
            }
            i += 1;
        }
        Args { positional, flags, bool_flags }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(|s| s.as_str())
    }

    pub fn get_f64(&self, name: &str, default: f64) -> f64 {
        self.get(name).and_then(|s| s.parse().ok()).unwrap_or(default)
    }

    pub fn get_usize(&self, name: &str, default: usize) -> usize {
        self.get(name).and_then(|s| s.parse().ok()).unwrap_or(default)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.bool_flags.contains(name) || self.flags.contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_positional_and_space_separated_flags() {
        let raw: Vec<String> = ["model.json", "--time", "50", "--reachability"].iter().map(|s| s.to_string()).collect();
        let args = Args::parse(&raw, &["reachability"]);
        assert_eq!(args.positional, vec!["model.json".to_string()]);
        assert_eq!(args.get_f64("time", 0.0), 50.0);
        assert!(args.flag("reachability"));
        assert!(!args.flag("json"));
    }

    #[test]
    fn parses_equals_form() {
        let raw: Vec<String> = ["model.json", "--output=out.json", "--max-states=500"].iter().map(|s| s.to_string()).collect();
        let args = Args::parse(&raw, &[]);
        assert_eq!(args.get("output"), Some("out.json"));
        assert_eq!(args.get_usize("max-states", 0), 500);
    }

    #[test]
    fn unknown_trailing_flag_without_value_is_a_bool() {
        let raw: Vec<String> = ["model.json", "--verbose"].iter().map(|s| s.to_string()).collect();
        let args = Args::parse(&raw, &[]);
        assert!(args.flag("verbose"));
    }
}
