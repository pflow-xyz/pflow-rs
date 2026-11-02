//! `validate`: hosted `pflow_validate` (`mcpValidate` over go-pflow's
//! `validation.Validator`), answering in `validation.ValidationResult`'s JSON
//! shape key for key.
//!
//! go-pflow's checks read the petri net itself (arc and capacity vectors,
//! inhibitor arcs counted as arcs), which `pflow-validation`'s port over a
//! metamodel `Model` cannot see, and its messages differ in small ways (`→`,
//! `%.2f`, `%v` lists). So the checks are ported here over the expanded
//! `pflow_core::PetriNet`, and the analyses behind them (P-invariant basis,
//! pump witness, reachability graph) run on `pflow-reachability`, the port of
//! the analyzer go-pflow uses.
//!
//! Issues come out in sorted-id order. go-pflow ranges over Go maps, so the
//! hosted server's order of issues within one check varies from call to call;
//! the set is the same.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use pflow_core::PetriNet;
use pflow_reachability::{marking, Analyzer, InvariantAnalyzer, Marking};
use serde_json::{json, Map, Value};

use crate::net::{
    expand, model_of, net_arg, num_arg, petri_net, sorted, start_marking, MAX_STATES,
};

/// go-pflow's `unboundedSearchLimit`.
const UNBOUNDED_SEARCH_LIMIT: usize = 2000;

pub(crate) fn run(args: &Value) -> Result<Value, String> {
    let mut doc = net_arg(args)?;
    drop_unbounded_capacity(&mut doc);
    let raw = petri_net(&doc);
    let mut v = Validator::new(&raw);
    if args
        .get("reachability")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let max = num_arg(args, "max_states", 2000.0, 1.0, MAX_STATES)? as usize;
        v.validate_with_reachability(max);
    } else {
        v.validate();
    }
    Ok(v.into_json())
}

/// `dropUnboundedCapacity`: a capacity list with no non-zero number in it is
/// left out, so the structure check sees the editor's "unbounded".
fn drop_unbounded_capacity(doc: &mut Map<String, Value>) {
    let Some(Value::Object(places)) = doc.get_mut("places") else {
        return;
    };
    for p in places.values_mut() {
        let Value::Object(p) = p else { continue };
        let unbounded = match p.get("capacity") {
            Some(Value::Array(caps)) => !caps.iter().any(|c| c.as_f64().is_some_and(|f| f != 0.0)),
            _ => false,
        };
        if unbounded {
            p.remove("capacity");
        }
    }
}

struct Issue {
    category: &'static str,
    message: String,
    location: Vec<String>,
    suggestion: &'static str,
}

impl Issue {
    fn json(&self, severity: &str) -> Value {
        let mut m = Map::new();
        m.insert("severity".into(), json!(severity));
        m.insert("category".into(), json!(self.category));
        m.insert("message".into(), json!(self.message));
        if !self.location.is_empty() {
            m.insert("location".into(), json!(self.location));
        }
        if !self.suggestion.is_empty() {
            m.insert("suggestion".into(), json!(self.suggestion));
        }
        Value::Object(m)
    }
}

struct Validator<'n> {
    raw: &'n PetriNet,
    net: PetriNet,
    errors: Vec<Issue>,
    warnings: Vec<Issue>,
    info: Vec<Issue>,
    conserved: bool,
    invariants: Vec<String>,
    reachability: Option<Value>,
    /// `Valid` and the summary's counts, fixed when `Validate` returns: the
    /// reachability findings added after it change neither (as in go-pflow).
    counted: (bool, usize, usize),
}

/// `fmt.Sprintf("%v", []string{...})`.
fn go_list(items: &[String]) -> String {
    format!("[{}]", items.join(" "))
}

impl<'n> Validator<'n> {
    fn new(raw: &'n PetriNet) -> Self {
        let (net, _) = expand(raw);
        Validator {
            raw,
            net,
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
            conserved: false,
            invariants: Vec::new(),
            reachability: None,
            counted: (true, 0, 0),
        }
    }

    fn error(
        &mut self,
        category: &'static str,
        message: String,
        location: Vec<String>,
        suggestion: &'static str,
    ) {
        self.errors.push(Issue {
            category,
            message,
            location,
            suggestion,
        });
    }

    fn warning(
        &mut self,
        category: &'static str,
        message: String,
        location: Vec<String>,
        suggestion: &'static str,
    ) {
        self.warnings.push(Issue {
            category,
            message,
            location,
            suggestion,
        });
    }

    fn note(&mut self, category: &'static str, message: String, location: Vec<String>) {
        self.info.push(Issue {
            category,
            message,
            location,
            suggestion: "",
        });
    }

    fn validate(&mut self) {
        self.check_structure();
        self.check_connectivity();
        self.check_deadlocks();
        self.check_unbounded();
        self.check_conservation();
        self.counted = (
            self.errors.is_empty(),
            self.errors.len(),
            self.warnings.len(),
        );
    }

    fn validate_with_reachability(&mut self, max_states: usize) {
        self.validate();
        let model = model_of(&self.net);
        let res = Analyzer::new(&model).with_max_states(max_states).analyze();
        let mut bounded = res.bounded;
        let mut reason = String::new();
        if bounded {
            if let Some(w) = Analyzer::new(&model)
                .with_max_states(max_states)
                .find_unbounded_witness()
            {
                bounded = false;
                reason = format!("unbounded: {} grow without bound", w.places.join(", "));
            }
        }
        if res.truncated && reason.is_empty() {
            reason = if res.truncate_msg.is_empty() {
                format!("state limit of {max_states} reached")
            } else {
                res.truncate_msg.clone()
            };
        }
        let mut terminal = Vec::new();
        let mut deadlock = Vec::new();
        for state in res.graph.states.values() {
            if state.is_terminal {
                terminal.push(render(&state.marking));
            }
            if state.is_deadlock {
                deadlock.push(render(&state.marking));
            }
        }
        terminal.sort();
        deadlock.sort();
        // go-pflow's `Graph.MaxTokens` only records a place once it beats the
        // map's zero value, so a place that never holds a token is absent;
        // pflow-reachability lists it with 0.
        let max_tokens: BTreeMap<&String, &i64> =
            res.max_tokens.iter().filter(|(_, v)| **v > 0).collect();
        let mut r = Map::new();
        r.insert("reachable".into(), json!(res.state_count));
        r.insert("bounded".into(), json!(bounded));
        r.insert("maxTokens".into(), json!(max_tokens));
        r.insert("terminalStates".into(), nil_if_empty(&terminal));
        r.insert("deadlockStates".into(), nil_if_empty(&deadlock));
        r.insert("hasCycles".into(), json!(res.has_cycle));
        r.insert("maxDepth".into(), json!(res.max_depth));
        r.insert("truncated".into(), json!(res.truncated));
        if !reason.is_empty() {
            r.insert("truncatedReason".into(), json!(reason));
        }
        self.reachability = Some(Value::Object(r));

        if !deadlock.is_empty() {
            self.warning(
                "reachability",
                format!(
                    "Found {} deadlock states (terminal states that are not goal states)",
                    deadlock.len()
                ),
                Vec::new(),
                "Review model structure to ensure all terminal states are valid end states",
            );
        }
        if !bounded {
            self.error(
                "reachability",
                "Model is unbounded (some places can accumulate tokens indefinitely)".into(),
                Vec::new(),
                "Add capacity constraints or fix structural issues causing unbounded growth",
            );
        }
        if res.truncated {
            self.warning(
                "reachability",
                format!("Reachability analysis truncated: {reason}"),
                Vec::new(),
                "Consider simplifying model or increasing state limit",
            );
        }
    }

    fn check_structure(&mut self) {
        if self.net.places.is_empty() {
            self.error(
                "structure",
                "Net has no places".into(),
                Vec::new(),
                "Add at least one place",
            );
            return;
        }
        if self.net.transitions.is_empty() {
            self.warning(
                "structure",
                "Net has no transitions".into(),
                Vec::new(),
                "Add transitions to enable dynamics",
            );
        }
        if self.net.arcs.is_empty() {
            self.warning(
                "structure",
                "Net has no arcs".into(),
                Vec::new(),
                "Add arcs to connect places and transitions",
            );
        }
        for name in sorted(self.net.places.keys()) {
            let place = &self.net.places[&name];
            let tokens = place.token_count();
            let capacity: Option<f64> =
                (!place.capacity.is_empty()).then(|| place.capacity.iter().sum());
            if tokens < 0.0 {
                self.error(
                    "structure",
                    format!("Place '{name}' has negative initial tokens"),
                    vec![name.clone()],
                    "Set initial tokens to non-negative value",
                );
            }
            if let Some(capacity) = capacity {
                if tokens > capacity {
                    self.error(
                        "structure",
                        format!("Place '{name}' initial tokens ({tokens:.2}) exceed capacity ({capacity:.2})"),
                        vec![name.clone()],
                        "Reduce initial tokens or increase capacity",
                    );
                }
            }
        }
        for (i, arc) in self.raw.arcs.iter().enumerate() {
            if arc.weight_sum() <= 0.0 {
                self.error(
                    "structure",
                    format!(
                        "Arc {i} ({} → {}) has non-positive weight",
                        arc.source, arc.target
                    ),
                    vec![arc.source.clone(), arc.target.clone()],
                    "Set arc weight to positive value",
                );
            }
        }
    }

    fn check_connectivity(&mut self) {
        let mut connected = BTreeSet::new();
        for arc in &self.net.arcs {
            connected.insert(arc.source.clone());
            connected.insert(arc.target.clone());
        }
        for name in sorted(self.net.places.keys()) {
            if !connected.contains(&name) {
                self.warning(
                    "connectivity",
                    format!("Place '{name}' is not connected to any transition"),
                    vec![name],
                    "Add arcs to connect this place",
                );
            }
        }
        let transitions = sorted(self.net.transitions.keys());
        for name in &transitions {
            if !connected.contains(name) {
                self.warning(
                    "connectivity",
                    format!("Transition '{name}' is not connected"),
                    vec![name.clone()],
                    "Add input and output arcs",
                );
            }
        }
        let mut inputs: HashMap<&str, usize> = HashMap::new();
        let mut outputs: HashMap<&str, usize> = HashMap::new();
        for arc in &self.net.arcs {
            if self.net.places.contains_key(&arc.source) {
                *inputs.entry(arc.target.as_str()).or_default() += 1;
            }
            if self.net.places.contains_key(&arc.target) {
                *outputs.entry(arc.source.as_str()).or_default() += 1;
            }
        }
        let (no_in, no_out): (Vec<String>, Vec<String>) = (
            transitions
                .iter()
                .filter(|t| !inputs.contains_key(t.as_str()))
                .cloned()
                .collect(),
            transitions
                .iter()
                .filter(|t| !outputs.contains_key(t.as_str()))
                .cloned()
                .collect(),
        );
        for name in transitions {
            if no_in.contains(&name) {
                self.warning(
                    "connectivity",
                    format!("Transition '{name}' has no input places"),
                    vec![name.clone()],
                    "Add input arcs from places",
                );
            }
            if no_out.contains(&name) {
                self.warning(
                    "connectivity",
                    format!("Transition '{name}' has no output places"),
                    vec![name.clone()],
                    "Add output arcs to places",
                );
            }
        }
    }

    fn check_deadlocks(&mut self) {
        let mut by_transition: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
        for arc in &self.net.arcs {
            if self.net.places.contains_key(&arc.source) {
                by_transition
                    .entry(arc.target.clone())
                    .or_default()
                    .push((arc.source.clone(), arc.weight_sum()));
            }
        }
        for (trans, inputs) in by_transition {
            let blocked: Vec<String> = inputs
                .iter()
                .filter(|(p, w)| self.net.places[p].token_count() < *w)
                .map(|(p, _)| p.clone())
                .collect();
            if !blocked.is_empty() {
                let mut location = vec![trans.clone()];
                location.extend(blocked.iter().cloned());
                self.warning(
                    "deadlock",
                    format!(
                        "Transition '{trans}' cannot fire with initial marking (insufficient tokens in: {})",
                        go_list(&blocked)
                    ),
                    location,
                    "Increase initial tokens in input places or adjust arc weights",
                );
            }
        }
    }

    fn check_unbounded(&mut self) {
        let model = model_of(&self.net);
        let basis = InvariantAnalyzer::new(&model).p_invariant_basis();
        let mut covered = BTreeSet::new();
        for vec in &basis.basis {
            for (i, &c) in vec.iter().enumerate() {
                if c > 0 {
                    covered.insert(basis.labels[i].clone());
                }
            }
        }
        if let Some(w) = Analyzer::new(&model)
            .with_max_states(UNBOUNDED_SEARCH_LIMIT)
            .find_unbounded_witness()
        {
            for name in &w.places {
                if self
                    .net
                    .places
                    .get(name)
                    .is_some_and(|p| !p.capacity.is_empty())
                {
                    continue;
                }
                self.error(
                    "unbounded",
                    format!(
                        "Place '{name}' is unbounded: repeating [{}] adds tokens indefinitely",
                        w.pump.join(" → ")
                    ),
                    vec![name.clone()],
                    "Add a capacity, or consume the accumulated tokens on the cycle",
                );
            }
        }
        let mut place_inputs: HashMap<&str, usize> = HashMap::new();
        let mut place_outputs: HashMap<&str, usize> = HashMap::new();
        for arc in &self.net.arcs {
            if self.net.places.contains_key(&arc.target) {
                *place_inputs.entry(arc.target.as_str()).or_default() += 1;
            }
            if self.net.places.contains_key(&arc.source) {
                *place_outputs.entry(arc.source.as_str()).or_default() += 1;
            }
        }
        let mut found = Vec::new();
        for name in sorted(self.net.places.keys()) {
            if !self.net.places[&name].capacity.is_empty() {
                continue;
            }
            let inputs = place_inputs.get(name.as_str()).copied().unwrap_or(0);
            let outputs = place_outputs.get(name.as_str()).copied().unwrap_or(0);
            found.push((name, inputs, outputs));
        }
        for (name, inputs, outputs) in found {
            if inputs > 0 && outputs == 0 {
                self.note(
                    "unbounded",
                    format!("Place '{name}' is a sink (only inputs, no outputs)"),
                    vec![name.clone()],
                );
            }
            if outputs > 0 && inputs == 0 {
                self.note(
                    "unbounded",
                    format!("Place '{name}' is a source (only outputs, no inputs)"),
                    vec![name.clone()],
                );
            }
            if !covered.contains(&name) && inputs > 0 {
                self.warning(
                    "unbounded",
                    format!("Place '{name}' is not covered by any P-invariant, so nothing structurally bounds it"),
                    vec![name],
                    "Add a capacity, or balance the flow so the place participates in a conservation law",
                );
            }
        }
    }

    fn check_conservation(&mut self) {
        let mut non_conserving = Vec::new();
        for name in sorted(self.net.transitions.keys()) {
            let input: f64 = self
                .net
                .arcs
                .iter()
                .filter(|a| a.target == name)
                .map(|a| a.weight_sum())
                .sum();
            let output: f64 = self
                .net
                .arcs
                .iter()
                .filter(|a| a.source == name)
                .map(|a| a.weight_sum())
                .sum();
            if input != output {
                non_conserving.push(name);
            }
        }
        self.conserved = non_conserving.is_empty();
        if !self.conserved {
            self.note(
                "conservation",
                format!(
                    "Net does not conserve total tokens (transitions with unbalanced flow: {})",
                    go_list(&non_conserving)
                ),
                non_conserving,
            );
        } else {
            self.note(
                "conservation",
                "Net conserves tokens (all transitions have balanced input/output)".into(),
                Vec::new(),
            );
        }
        let model = model_of(&self.net);
        let initial: Marking = start_marking(&self.net).into_iter().collect();
        let mut rendered: Vec<String> = InvariantAnalyzer::new(&model)
            .find_p_invariants(&initial)
            .iter()
            .map(|i| i.render())
            .collect();
        if rendered.is_empty() {
            return;
        }
        rendered.sort();
        self.note(
            "conservation",
            format!(
                "Found {} conservation law(s): {}",
                rendered.len(),
                rendered.join("; ")
            ),
            Vec::new(),
        );
        self.invariants = rendered;
    }

    fn into_json(self) -> Value {
        let mut out = Map::new();
        let (valid, errors, warnings) = self.counted;
        out.insert("valid".into(), json!(valid));
        for (key, list, severity) in [
            ("errors", &self.errors, "error"),
            ("warnings", &self.warnings, "warning"),
            ("info", &self.info, "info"),
        ] {
            if !list.is_empty() {
                out.insert(
                    key.into(),
                    Value::Array(list.iter().map(|i| i.json(severity)).collect()),
                );
            }
        }
        out.insert(
            "summary".into(),
            json!({
                "places": self.net.places.len(),
                "transitions": self.net.transitions.len(),
                "arcs": self.net.arcs.len(),
                "errors": errors,
                "warnings": warnings,
                "conserved": self.conserved,
            }),
        );
        if let Some(r) = self.reachability {
            out.insert("reachability".into(), r);
        }
        if !self.invariants.is_empty() {
            out.insert("invariants".into(), json!(self.invariants));
        }
        Value::Object(out)
    }
}

/// go-pflow's `Marking.String`: positive places only, `(empty)` for none.
fn render(m: &Marking) -> String {
    let positive: Marking = m
        .iter()
        .filter(|(_, &v)| v > 0)
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    marking::render(&positive)
}

fn nil_if_empty(list: &[String]) -> Value {
    if list.is_empty() {
        Value::Null
    } else {
        json!(list)
    }
}
