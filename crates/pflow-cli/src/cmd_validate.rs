use crate::model_io::{load_model, write_output};
use crate::Args;
use pflow_validation::{Issue, Severity, Validator};

fn issue_json(i: &Issue) -> serde_json::Value {
    serde_json::json!({
        "severity": match i.severity { Severity::Error => "error", Severity::Warning => "warning", Severity::Info => "info" },
        "category": i.category,
        "message": i.message,
        "location": i.location,
        "suggestion": i.suggestion,
    })
}

pub fn run(raw: &[String]) -> Result<(), String> {
    let args = Args::parse(raw, &["reachability", "json"]);
    let path = args.positional.first().ok_or("model file required")?;
    let model = load_model(path)?;

    let max_states = args.get_usize("max-states", 10_000);
    let result = if args.flag("reachability") {
        Validator::new(&model).validate_with_reachability(max_states)
    } else {
        Validator::new(&model).validate()
    };

    let out = serde_json::json!({
        "valid": result.valid,
        "summary": {
            "places": result.summary.places,
            "transitions": result.summary.transitions,
            "arcs": result.summary.arcs,
            "errors": result.summary.errors,
            "warnings": result.summary.warnings,
            "conserved": result.summary.conserved,
        },
        "errors": result.errors.iter().map(issue_json).collect::<Vec<_>>(),
        "warnings": result.warnings.iter().map(issue_json).collect::<Vec<_>>(),
        "info": result.info.iter().map(issue_json).collect::<Vec<_>>(),
        "invariants": result.invariants,
        "reachability": result.reachability.map(|r| serde_json::json!({
            "reachable": r.reachable,
            "bounded": r.bounded,
            "maxTokens": r.max_tokens,
            "terminalStates": r.terminal_states,
            "deadlockStates": r.deadlock_states,
            "hasCycles": r.has_cycles,
            "maxDepth": r.max_depth,
            "truncated": r.truncated,
            "truncatedReason": r.truncated_reason,
        })),
    });

    write_output(&out, args.get("output"))?;
    if !result.valid {
        return Err(String::new());
    }
    Ok(())
}
