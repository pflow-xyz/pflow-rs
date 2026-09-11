//! Token-based-replay conformance checking — a port of go-pflow's
//! `mining/conformance.go`.
//!
//! Multi-color nets are unfolded first ([`PetriNet::expand_colors`]), so
//! replay consumes and produces per color: a trace that supplies the wrong
//! color is counted as a missing token rather than satisfied by a summed pool.

use std::collections::{HashMap, HashSet};

use pflow_core::PetriNet;
use pflow_eventlog::{EventLog, Trace};

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

    let activity_to_transition = build_activity_mapping(&net);
    let initial_marking = initial_marking(&net);

    for trace in log.traces() {
        let trace_result = replay_trace(trace, &net, &activity_to_transition, &initial_marking);

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

        let (missing, consumed, produced) = fire_transition(net, trans_id, &mut marking);
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

/// Attempts to fire a transition, returning `(missing, consumed, produced)`
/// token counts and mutating `marking` in place. Mirrors go-pflow's
/// "force fire": if not fully enabled, still consumes what's available and
/// counts the shortfall as missing, rather than refusing to fire at all.
fn fire_transition(net: &PetriNet, trans_id: &str, marking: &mut TokenState) -> (i64, i64, i64) {
    let mut input_places: HashMap<String, i64> = HashMap::new();
    for arc in &net.arcs {
        if arc.target == trans_id {
            let mut weight = arc.weight_sum() as i64;
            if weight == 0 {
                weight = 1;
            }
            input_places.insert(arc.source.clone(), weight);
        }
    }

    let mut missing = 0i64;
    let mut consumed = 0i64;
    let mut enabled = true;
    for (place_id, &required) in &input_places {
        let available = *marking.get(place_id).unwrap_or(&0);
        if available < required {
            missing += required - available;
            enabled = false;
        }
        consumed += required;
    }

    // Force fire: consume what's available even when not fully enabled,
    // leaving the shortfall recorded in `missing` rather than refusing to
    // fire (a partial replay tells you more than a stalled one).
    for (place_id, &required) in &input_places {
        let available = *marking.get(place_id).unwrap_or(&0);
        if enabled || available >= required {
            *marking.entry(place_id.clone()).or_insert(0) -= required;
        } else {
            marking.insert(place_id.clone(), 0);
        }
    }

    let mut produced = 0i64;
    for arc in &net.arcs {
        if arc.source == trans_id {
            let mut weight = arc.weight_sum() as i64;
            if weight == 0 {
                weight = 1;
            }
            *marking.entry(arc.target.clone()).or_insert(0) += weight;
            produced += weight;
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
                    if is_enabled(&net, trans_id, &marking) {
                        enabled.insert(trans_id.clone());
                    }
                }
                state_enabled.insert(state_key.clone(), enabled);
            }

            if let Some(trans_id) = activity_to_transition.get(activity) {
                state_visits.get_mut(&state_key).unwrap().insert(trans_id.clone());
                fire_transition_silent(&net, trans_id, &mut marking);
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

fn is_enabled(net: &PetriNet, trans_id: &str, marking: &TokenState) -> bool {
    for arc in &net.arcs {
        if arc.target == trans_id {
            let mut weight = arc.weight_sum() as i64;
            if weight == 0 {
                weight = 1;
            }
            if *marking.get(&arc.source).unwrap_or(&0) < weight {
                return false;
            }
        }
    }
    true
}

fn fire_transition_silent(net: &PetriNet, trans_id: &str, marking: &mut TokenState) {
    for arc in &net.arcs {
        if arc.target == trans_id {
            let mut weight = arc.weight_sum() as i64;
            if weight == 0 {
                weight = 1;
            }
            let entry = marking.entry(arc.source.clone()).or_insert(0);
            if *entry >= weight {
                *entry -= weight;
            }
        }
    }
    for arc in &net.arcs {
        if arc.source == trans_id {
            let mut weight = arc.weight_sum() as i64;
            if weight == 0 {
                weight = 1;
            }
            *marking.entry(arc.target.clone()).or_insert(0) += weight;
        }
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
