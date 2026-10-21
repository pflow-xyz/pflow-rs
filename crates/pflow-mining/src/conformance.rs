//! Token-based-replay conformance checking — a port of go-pflow's
//! `mining/conformance.go`.
//!
//! Multi-color nets are unfolded first ([`PetriNet::expand_colors`]), so
//! replay consumes and produces per color: a trace that supplies the wrong
//! color is counted as a missing token rather than satisfied by a summed pool.
//!
//! # Gates: inhibitor and read arcs
//!
//! Replay honours inhibitor and read arcs (go-pflow `84ae2a9`, "mining:
//! honour inhibitor and read arcs in conformance replay"). Before that fix
//! — in go-pflow and in this port alike — replay, `is_enabled` and
//! `fire_transition_silent` each walked `net.arcs` on their own and read
//! every place -> transition arc as a consuming input and every transition
//! -> place arc as an output, so an inhibitor arc *demanded* a token from
//! the place it exists to require empty and a read arc *produced* a token
//! into the place it only tests.
//!
//! go-pflow's fix gives its replay one private classification (`rulesFor`).
//! This port goes one step further, per ROADMAP.md ground rule 4 (the
//! firing rule has one home): the expanded net is converted once into a
//! [`pflow_metamodel::Model`] (`replay_model`), and every question of
//! which arc consumes, produces or only tests — and whether a transition is
//! enabled, and what a proper firing does to the marking — is answered by
//! the shared firing rule ([`Model::inputs`]/[`Model::outputs`]/
//! [`Model::tests`]/[`Model::enabled`]/[`Model::fire`]). What stays here is
//! replay's own bookkeeping, which is not a firing rule: the missing-token
//! tally, and the "force fire" repair a non-enabled event gets.
//!
//! The tally follows go-pflow exactly: a consuming input short of its weight
//! is missing the tokens it is short; a read arc short of its weight
//! likewise; an inhibitor arc whose place is not clear is *one* unmet
//! condition however many tokens sit in it. Gates are checked, never moved,
//! even by a force fire.
//!
//! Activities map to transitions by display label (`label_text`, then
//! `label`, then id), as go-pflow's `buildActivityMapping` does at
//! `84ae2a9` — unchanged by that fix.

use std::collections::{HashMap, HashSet};

use pflow_core::PetriNet;
use pflow_eventlog::{EventLog, Trace};
use pflow_metamodel::{Arc as MmArc, ArcType, Model, Place as MmPlace, Transition as MmTransition};

/// Token counts during replay: place id -> count.
pub type TokenState = HashMap<String, i64>;

/// The result of a full token-based-replay conformance check.
#[derive(Debug, Clone, Default)]
pub struct ConformanceResult {
    /// Overall fitness, `0.0` to `1.0`.
    pub fitness: f64,
    pub produced_tokens: i64,
    pub consumed_tokens: i64,
    pub missing_tokens: i64,
    pub remaining_tokens: i64,
    pub trace_results: Vec<TraceReplayResult>,
    pub fitting_traces: usize,
    pub total_traces: usize,
    pub fitting_percent: f64,
    pub avg_trace_fitness: f64,
}

/// The result of replaying a single trace.
#[derive(Debug, Clone, Default)]
pub struct TraceReplayResult {
    pub case_id: String,
    pub fitness: f64,
    pub fitting: bool,
    pub missing_tokens: i64,
    pub remaining_tokens: i64,
    pub produced_tokens: i64,
    pub consumed_tokens: i64,
    pub fired_transitions: Vec<String>,
    pub missing_activities: Vec<String>,
    pub activities: Vec<String>,
}

/// Performs token-based-replay conformance checking: replays each trace from
/// the event log against the model and computes fitness metrics.
pub fn check_conformance(log: &EventLog, net: &PetriNet) -> ConformanceResult {
    let (net, _) = net.expand_colors();

    let mut result = ConformanceResult {
        total_traces: log.num_cases(),
        ..Default::default()
    };

    let model = replay_model(&net);
    let activity_to_transition = build_activity_mapping(&net);
    let initial_marking = initial_marking(&net);

    for trace in log.traces() {
        let trace_result = replay_trace(trace, &net, &model, &activity_to_transition, &initial_marking);

        result.produced_tokens += trace_result.produced_tokens;
        result.consumed_tokens += trace_result.consumed_tokens;
        result.missing_tokens += trace_result.missing_tokens;
        result.remaining_tokens += trace_result.remaining_tokens;
        if trace_result.fitting {
            result.fitting_traces += 1;
        }
        result.trace_results.push(trace_result);
    }

    if result.consumed_tokens > 0 && result.produced_tokens > 0 {
        let missing_ratio = result.missing_tokens as f64 / result.consumed_tokens as f64;
        let remaining_ratio = result.remaining_tokens as f64 / result.produced_tokens as f64;
        result.fitness = 0.5 * (1.0 - missing_ratio) + 0.5 * (1.0 - remaining_ratio);
    } else if result.total_traces == 0 {
        result.fitness = 1.0;
    }

    if result.total_traces > 0 {
        result.fitting_percent = result.fitting_traces as f64 / result.total_traces as f64 * 100.0;
        let total_fitness: f64 = result.trace_results.iter().map(|t| t.fitness).sum();
        result.avg_trace_fitness = total_fitness / result.total_traces as f64;
    }

    result
}

fn build_activity_mapping(net: &PetriNet) -> HashMap<String, String> {
    let mut ids: Vec<&String> = net.transitions.keys().collect();
    ids.sort();
    let mut mapping = HashMap::new();
    for trans_id in ids {
        let trans = &net.transitions[trans_id];
        let key = match &trans.label_text {
            Some(t) if !t.is_empty() => t.clone(),
            _ if !trans.label.is_empty() => trans.label.clone(),
            _ => trans_id.clone(),
        };
        mapping.insert(key, trans_id.clone());
    }
    mapping
}

fn initial_marking(net: &PetriNet) -> TokenState {
    let mut marking = TokenState::new();
    for (place_id, place) in &net.places {
        if !place.initial.is_empty() {
            let total: i64 = place.initial.iter().map(|&v| v as i64).sum();
            if total > 0 {
                marking.insert(place_id.clone(), total);
            }
        }
    }
    marking
}

fn replay_trace(
    trace: &Trace,
    net: &PetriNet,
    model: &Model,
    activity_to_transition: &HashMap<String, String>,
    initial_marking: &TokenState,
) -> TraceReplayResult {
    let mut result = TraceReplayResult {
        case_id: trace.case_id.clone(),
        activities: trace.activity_variant(),
        ..Default::default()
    };

    let mut marking = initial_marking.clone();
    result.produced_tokens += marking.values().sum::<i64>();

    for activity in &result.activities.clone() {
        let Some(trans_id) = activity_to_transition.get(activity) else {
            result.missing_activities.push(activity.clone());
            result.missing_tokens += 1;
            continue;
        };

        let (missing, consumed, produced) = fire_transition(model, trans_id, &mut marking);
        result.missing_tokens += missing;
        result.consumed_tokens += consumed;
        result.produced_tokens += produced;

        if missing == 0 {
            result.fired_transitions.push(trans_id.clone());
        } else {
            result.missing_activities.push(activity.clone());
        }
    }

    // Final places are places a token can enter but never leave, identified
    // structurally (no non-inhibitor outgoing arc). A token resting there at
    // the end of a trace means the case completed — the opposite of a
    // conformance problem — so it is excluded from "remaining".
    let finals = final_places(net);
    let mut completed = false;
    for (place_id, &count) in &marking {
        if count <= 0 {
            continue;
        }
        if finals.contains(place_id) {
            completed = true;
            continue;
        }
        result.remaining_tokens += count;
    }
    if !finals.is_empty() && !completed {
        result.remaining_tokens += 1;
    }

    if result.consumed_tokens > 0 && result.produced_tokens > 0 {
        let missing_ratio = result.missing_tokens as f64 / result.consumed_tokens as f64;
        let remaining_ratio = result.remaining_tokens as f64 / result.produced_tokens as f64;
        result.fitness = (0.5 * (1.0 - missing_ratio) + 0.5 * (1.0 - remaining_ratio)).max(0.0);
    } else {
        result.fitness = 1.0;
    }

    result.fitting = result.missing_tokens == 0 && result.remaining_tokens == 0;
    result
}

/// The expanded net as the shared firing rule sees it: one
/// [`pflow_metamodel::Model`] built once per check, so replay asks
/// `pflow-metamodel` rather than re-deriving arc roles from `net.arcs`.
///
/// The Shape A -> Shape B arc mapping is the one go-pflow's metapetri and
/// `parser.ModelFromJSON` (here: `pflow_core::colors::unfold`) use: an
/// `inhibit_transition` arc drawn place -> transition is an inhibitor (blocks
/// while the place holds at least the weight); one drawn transition -> place
/// is a read arc (needs the place to hold at least the weight), emitted as
/// the metamodel's explicit read arc from the place to the transition.
/// Neither moves a token. Which end is the transition decides the direction,
/// matching the order of go-pflow's `rulesFor` switch.
///
/// Three choices keep replay's numbers go-pflow's, not just its semantics:
///
///   - **No capacity.** Every place is unbounded here (`capacity: 0`).
///     go-pflow's replay never consulted capacity, before `84ae2a9` or
///     after, so carrying it would make a capacity-bounded net replay
///     differently from go-pflow.
///   - **Last arc wins.** go-pflow keys each role by place
///     (`inputs[place] = weight`), so a second arc in the same role between
///     the same place and transition replaces the first. Duplicates are
///     collapsed the same way, rather than handed to the firing rule as two
///     arcs.
///   - **Dangling endpoints are places.** An arc naming a node the net does
///     not declare still demands (or receives) tokens there in go-pflow, so
///     such a node becomes an empty place rather than being silently
///     dropped by the firing rule's "token place" filter. An arc between two
///     transitions, or two places, is malformed and contributes nothing.
fn replay_model(net: &PetriNet) -> Model {
    let mut model = Model::default();

    let mut place_ids: Vec<&String> = net.places.keys().collect();
    place_ids.sort();
    let mut known: HashSet<&str> = HashSet::new();
    for id in place_ids {
        known.insert(id);
        model.places.push(MmPlace {
            id: id.clone(),
            initial: net.places[id].initial.iter().map(|&v| v as i64).sum(),
            ..Default::default()
        });
    }

    let mut trans_ids: Vec<&String> = net.transitions.keys().collect();
    trans_ids.sort();
    for id in trans_ids {
        model.transitions.push(MmTransition {
            id: id.clone(),
            ..Default::default()
        });
    }

    let is_transition = |id: &str| net.transitions.contains_key(id);
    let mut index: HashMap<(String, String, ArcType), usize> = HashMap::new();
    for arc in &net.arcs {
        // `as i64` truncates toward zero like go-pflow's `int(...)`; an
        // unset (zero) weight reads as 1 in the firing rule, as it does in
        // `rulesFor`.
        let weight = arc.weight_sum() as i64;
        let (place, transition, typ) = if is_transition(&arc.target) && !is_transition(&arc.source) {
            let typ = if arc.inhibit_transition {
                ArcType::Inhibitor
            } else {
                ArcType::Normal
            };
            (&arc.source, &arc.target, typ)
        } else if is_transition(&arc.source) && !is_transition(&arc.target) {
            let typ = if arc.inhibit_transition {
                ArcType::Read
            } else {
                ArcType::Normal
            };
            (&arc.target, &arc.source, typ)
        } else {
            continue;
        };
        if !known.contains(place.as_str()) {
            known.insert(place.as_str());
            model.places.push(MmPlace {
                id: place.clone(),
                ..Default::default()
            });
        }
        // A normal arc keeps its drawn direction (input or output); a gate
        // always points place -> transition, which is how the metamodel
        // spells both inhibitor and read arcs.
        let (from, to) = if typ == ArcType::Normal && is_transition(&arc.source) {
            (transition.clone(), place.clone())
        } else {
            (place.clone(), transition.clone())
        };
        let key = (from.clone(), to.clone(), typ);
        match index.get(&key) {
            Some(&k) => model.arcs[k].weight = weight,
            None => {
                index.insert(key, model.arcs.len());
                model.arcs.push(MmArc {
                    from,
                    to,
                    weight,
                    typ,
                    ..Default::default()
                });
            }
        }
    }

    model
}

/// How far `marking` is from satisfying `trans_id`'s non-consuming gates
/// (go-pflow's `gateShortfall`): the tokens each read arc needs and does not
/// find, plus one per inhibitor arc whose place is not clear. A gate is a
/// condition rather than a token, so a blocked inhibitor counts once however
/// many tokens sit in its place — replay's "missing tokens" is a count of
/// unmet requirements, and that is one requirement.
fn gate_shortfall(model: &Model, trans_id: &str, marking: &TokenState) -> i64 {
    let mut short = 0i64;
    for gate in model.tests(trans_id) {
        let have = *marking.get(&gate.place).unwrap_or(&0);
        match gate.typ {
            ArcType::Inhibitor => {
                if have >= gate.weight {
                    short += 1;
                }
            }
            _ => {
                if have < gate.weight {
                    short += gate.weight - have;
                }
            }
        }
    }
    short
}

/// Attempts to fire a transition, returning `(missing, consumed, produced)`
/// token counts and mutating `marking` in place. Mirrors go-pflow's
/// "force fire": if not fully enabled, still consumes what's available and
/// counts the shortfall as missing, rather than refusing to fire at all.
/// Gates are counted in `missing` when unmet and never moved.
fn fire_transition(model: &Model, trans_id: &str, marking: &mut TokenState) -> (i64, i64, i64) {
    let inputs = model.inputs(trans_id);
    let outputs = model.outputs(trans_id);

    let mut missing = 0i64;
    let mut consumed = 0i64;
    for input in &inputs {
        let available = *marking.get(&input.place).unwrap_or(&0);
        if available < input.weight {
            missing += input.weight - available;
        }
        consumed += input.weight;
    }
    missing += gate_shortfall(model, trans_id, marking);

    let produced: i64 = outputs.iter().map(|o| o.weight).sum();

    if model.enabled(trans_id, marking) {
        *marking = model.fire(trans_id, marking);
    } else {
        // Force fire: consume what's available even when not fully enabled,
        // leaving the shortfall recorded in `missing` rather than refusing
        // to fire (a partial replay tells you more than a stalled one). A
        // short input is drained to zero; gates are left alone.
        for input in &inputs {
            let entry = marking.entry(input.place.clone()).or_insert(0);
            if *entry >= input.weight {
                *entry -= input.weight;
            } else {
                *entry = 0;
            }
        }
        for output in &outputs {
            *marking.entry(output.place.clone()).or_insert(0) += output.weight;
        }
    }

    (missing, consumed, produced)
}

/// Places a token can enter but never leave — where completed cases come to
/// rest. A place with no non-inhibitor outgoing arc qualifies (an inhibitor
/// arc tests without consuming, so a place whose only outgoing arc is an
/// inhibitor is still a sink).
fn final_places(net: &PetriNet) -> HashSet<String> {
    let mut has_outgoing: HashSet<&str> = HashSet::new();
    for arc in &net.arcs {
        if arc.inhibit_transition {
            continue;
        }
        if net.places.contains_key(&arc.source) {
            has_outgoing.insert(&arc.source);
        }
    }
    net.places
        .keys()
        .filter(|id| !has_outgoing.contains(id.as_str()))
        .cloned()
        .collect()
}

// =============================================================================
// Precision metrics (ETC — Escaping-Edges)
// =============================================================================

/// The result of precision analysis via the ETC (escaping-edges) method.
#[derive(Debug, Clone, Default)]
pub struct PrecisionResult {
    /// `1 - escaping_edges/total_enabled`. Higher is better: the model
    /// allows less behavior beyond what the log shows.
    pub precision: f64,
    pub escaping_edges: usize,
    pub total_enabled: usize,
    pub unique_states: usize,
}

/// Computes precision via ETC (escaping edges): the fraction of
/// enabled-but-never-taken transitions across every state visited during
/// replay.
pub fn check_precision(log: &EventLog, net: &PetriNet) -> PrecisionResult {
    let (net, _) = net.expand_colors();
    let model = replay_model(&net);
    let activity_to_transition = build_activity_mapping(&net);
    let initial_marking = initial_marking(&net);

    let mut state_visits: HashMap<String, HashSet<String>> = HashMap::new();
    let mut state_enabled: HashMap<String, HashSet<String>> = HashMap::new();

    for trace in log.traces() {
        let mut marking = initial_marking.clone();
        for activity in &trace.activity_variant() {
            let state_key = marking_to_key(&marking);

            if !state_visits.contains_key(&state_key) {
                state_visits.insert(state_key.clone(), HashSet::new());
                let mut enabled = HashSet::new();
                for trans_id in net.transitions.keys() {
                    if is_enabled(&model, trans_id, &marking) {
                        enabled.insert(trans_id.clone());
                    }
                }
                state_enabled.insert(state_key.clone(), enabled);
            }

            if let Some(trans_id) = activity_to_transition.get(activity) {
                state_visits.get_mut(&state_key).unwrap().insert(trans_id.clone());
                fire_transition_silent(&model, trans_id, &mut marking);
            }
        }
    }

    let mut result = PrecisionResult {
        unique_states: state_enabled.len(),
        ..Default::default()
    };

    for (state_key, enabled) in &state_enabled {
        let taken = state_visits.get(state_key);
        for trans_id in enabled {
            result.total_enabled += 1;
            if !taken.is_some_and(|t| t.contains(trans_id)) {
                result.escaping_edges += 1;
            }
        }
    }

    result.precision = if result.total_enabled > 0 {
        1.0 - result.escaping_edges as f64 / result.total_enabled as f64
    } else {
        1.0
    };

    result
}

fn marking_to_key(marking: &TokenState) -> String {
    let mut places: Vec<&String> = marking.keys().collect();
    places.sort();
    let mut key = String::new();
    for p in places {
        let count = marking[p];
        if count > 0 {
            key.push_str(&format!("{p}:{count},"));
        }
    }
    key
}

/// Whether `trans_id` is enabled at `marking` — the shared firing rule's
/// answer ([`Model::enabled`]): consuming inputs present, read arcs
/// satisfied, inhibitors clear.
fn is_enabled(model: &Model, trans_id: &str, marking: &TokenState) -> bool {
    model.enabled(trans_id, marking)
}

/// Fires a transition without tracking metrics. Enabled, it is exactly
/// [`Model::fire`]. Not enabled (precision replays the log's event
/// regardless), it consumes each input only where enough tokens are present
/// and still produces every output, as go-pflow's `fireTransitionSilent`
/// does. Gates (inhibitor and read arcs) move nothing either way.
fn fire_transition_silent(model: &Model, trans_id: &str, marking: &mut TokenState) {
    if model.enabled(trans_id, marking) {
        *marking = model.fire(trans_id, marking);
        return;
    }
    for input in model.inputs(trans_id) {
        let entry = marking.entry(input.place).or_insert(0);
        if *entry >= input.weight {
            *entry -= input.weight;
        }
    }
    for output in model.outputs(trans_id) {
        *marking.entry(output.place).or_insert(0) += output.weight;
    }
}

// =============================================================================
// Combined conformance
// =============================================================================

/// Fitness + precision together, with their harmonic mean (F-score).
#[derive(Debug, Clone, Default)]
pub struct FullConformanceResult {
    pub fitness: ConformanceResult,
    pub precision: PrecisionResult,
    pub f_score: f64,
}

/// Performs both fitness and precision checking.
pub fn check_full_conformance(log: &EventLog, net: &PetriNet) -> FullConformanceResult {
    let fitness = check_conformance(log, net);
    let precision = check_precision(log, net);

    let f_score = if fitness.fitness + precision.precision > 0.0 {
        2.0 * fitness.fitness * precision.precision / (fitness.fitness + precision.precision)
    } else {
        0.0
    };

    FullConformanceResult {
        fitness,
        precision,
        f_score,
    }
}

impl ConformanceResult {
    /// Non-fitting traces, in original replay order.
    pub fn non_fitting_traces(&self) -> Vec<&TraceReplayResult> {
        self.trace_results.iter().filter(|t| !t.fitting).collect()
    }

    /// Traces sorted by fitness, lowest first.
    pub fn traces_by_fitness(&self) -> Vec<&TraceReplayResult> {
        let mut out: Vec<&TraceReplayResult> = self.trace_results.iter().collect();
        out.sort_by(|a, b| a.fitness.partial_cmp(&b.fitness).unwrap());
        out
    }
}

#[cfg(test)]
#[path = "conformance_gates_tests.rs"]
mod gates_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_eventlog::Event;

    /// The cafe-order.json showcase workflow: new -pay-> paid -start_brew->
    /// brewing -finish_brew-> ready -pick_up-> picked_up
    ///                  \-cancel-> cancelled -refund-> refunded
    fn cafe_order_net() -> PetriNet {
        let mut net = PetriNet::new();
        for p in ["new", "paid", "brewing", "ready", "picked_up", "cancelled", "refunded"] {
            let initial = if p == "new" { vec![1.0] } else { vec![0.0] };
            net.add_place(p, initial, vec![], 0.0, 0.0, None);
        }
        for t in ["pay", "start_brew", "finish_brew", "pick_up", "cancel", "refund"] {
            net.add_transition(t, "default", 0.0, 0.0, Some(t.to_string()));
        }
        let arcs = [
            ("new", "pay"),
            ("pay", "paid"),
            ("paid", "start_brew"),
            ("start_brew", "brewing"),
            ("brewing", "finish_brew"),
            ("finish_brew", "ready"),
            ("ready", "pick_up"),
            ("pick_up", "picked_up"),
            ("paid", "cancel"),
            ("cancel", "cancelled"),
            ("cancelled", "refund"),
            ("refund", "refunded"),
        ];
        for (from, to) in arcs {
            net.add_arc(from, to, vec![1.0], false);
        }
        net
    }

    /// showcase `fixtures/event-log.json`, in relative-second timestamps.
    fn showcase_event_log() -> EventLog {
        let mut log = EventLog::new();
        let mut push = |case: &str, activity: &str, t: i64| {
            log.add_event(Event::new(case, activity, t));
        };

        push("order-1", "pay", 0);
        push("order-1", "start_brew", 20);
        push("order-1", "finish_brew", 200);
        push("order-1", "pick_up", 230);

        push("order-2", "pay", 60);
        push("order-2", "start_brew", 240);
        push("order-2", "finish_brew", 450);
        push("order-2", "pick_up", 480);

        push("order-3", "pay", 120);
        push("order-3", "cancel", 150);
        push("order-3", "refund", 300);

        push("order-4", "pay", 180);
        push("order-4", "start_brew", 490);
        push("order-4", "pick_up", 540);

        push("order-5", "start_brew", 250);
        push("order-5", "finish_brew", 460);
        push("order-5", "pick_up", 480);

        log
    }

    #[test]
    fn showcase_fitness_and_precision_match_the_pilot_golden() {
        // Same golden as `tests/conformance_golden.rs`, which replays the
        // byte-copied showcase fixtures directly; this unit test hand-builds
        // the same net/log inline (in relative-millisecond timestamps) as a
        // fast, dependency-free regression that doesn't need the fixture
        // files. Values captured 2026-09-10 from the live petri-pilot
        // `petri_conformance` MCP tool; the showcase README rounds this to
        // "fitness 0.90, precision 0.80, three of five traces fit".
        let net = cafe_order_net();
        let log = showcase_event_log();

        let result = check_full_conformance(&log, &net);

        assert!(
            (result.fitness.fitness - 0.895_721_925_133_689_9).abs() < 1e-12,
            "fitness = {}",
            result.fitness.fitness
        );
        assert_eq!(result.precision.precision, 0.8);
        assert!(
            (result.f_score - 0.845_159_255_755_282_3).abs() < 1e-12,
            "f_score = {}",
            result.f_score
        );
        assert!(
            (result.fitness.avg_trace_fitness - 0.883_333_333_333_333_4).abs() < 1e-12,
            "avg_trace_fitness = {}",
            result.fitness.avg_trace_fitness
        );
        assert_eq!(result.fitness.fitting_traces, 3);
        assert_eq!(result.fitness.total_traces, 5);

        let order4 = result
            .fitness
            .trace_results
            .iter()
            .find(|t| t.case_id == "order-4")
            .unwrap();
        assert!(!order4.fitting);
        assert!((order4.fitness - 0.708_333_333_333_333_4).abs() < 1e-12);
        assert_eq!(order4.missing_activities, vec!["pick_up".to_string()]);

        let order5 = result
            .fitness
            .trace_results
            .iter()
            .find(|t| t.case_id == "order-5")
            .unwrap();
        assert!(!order5.fitting);
        assert!((order5.fitness - 0.708_333_333_333_333_4).abs() < 1e-12);
        assert_eq!(order5.missing_activities, vec!["start_brew".to_string()]);

        for case in ["order-1", "order-2", "order-3"] {
            let tr = result.fitness.trace_results.iter().find(|t| t.case_id == case).unwrap();
            assert!(tr.fitting, "{case} should fit");
            assert_eq!(tr.fitness, 1.0);
        }
    }

    #[test]
    fn empty_log_is_trivially_conformant() {
        let net = cafe_order_net();
        let log = EventLog::new();
        let result = check_conformance(&log, &net);
        assert_eq!(result.fitness, 1.0);
        assert_eq!(result.total_traces, 0);
    }

    #[test]
    fn unknown_activity_counts_as_missing() {
        let net = cafe_order_net();
        let mut log = EventLog::new();
        log.add_event(Event::new("c1", "pay", 0));
        log.add_event(Event::new("c1", "teleport", 1));
        let result = check_conformance(&log, &net);
        let tr = &result.trace_results[0];
        assert!(tr.missing_activities.contains(&"teleport".to_string()));
        assert!(!tr.fitting);
    }

    #[test]
    fn a_completed_trace_fits() {
        let net = cafe_order_net();
        let mut log = EventLog::new();
        for (i, a) in ["pay", "start_brew", "finish_brew", "pick_up"].iter().enumerate() {
            log.add_event(Event::new("c1", *a, i as i64));
        }
        let result = check_conformance(&log, &net);
        assert_eq!(result.fitting_traces, 1);
        assert_eq!(result.fitness, 1.0);
    }

    #[test]
    fn final_places_excludes_places_with_a_non_inhibitor_outgoing_arc() {
        let net = cafe_order_net();
        let finals = final_places(&net);
        assert!(finals.contains("picked_up"));
        assert!(finals.contains("refunded"));
        assert!(!finals.contains("new"));
        assert!(!finals.contains("paid"));
    }

    #[test]
    fn precision_is_perfect_when_only_one_path_is_ever_enabled() {
        // Strict linear net: a -> b, each state has exactly one enabled
        // transition, and the log always takes it.
        let mut net = PetriNet::new();
        net.add_place("p0", vec![1.0], vec![], 0.0, 0.0, None);
        net.add_place("p1", vec![0.0], vec![], 0.0, 0.0, None);
        net.add_transition("a", "default", 0.0, 0.0, Some("a".into()));
        net.add_arc("p0", "a", vec![1.0], false);
        net.add_arc("a", "p1", vec![1.0], false);

        let mut log = EventLog::new();
        log.add_event(Event::new("c1", "a", 0));

        let precision = check_precision(&log, &net);
        assert_eq!(precision.precision, 1.0);
        assert_eq!(precision.escaping_edges, 0);
    }
}
