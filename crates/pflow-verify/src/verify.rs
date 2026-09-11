//! [`Verifier`]: turns [`Property`] assertions into [`Verdict`]s with
//! evidence. Ported from go-pflow's `verify/verify.go`.
//!
//! Two proof strategies, in order: a **structural** proof from the
//! incidence matrix (a P-invariant, or a P-invariant dominating a `<=`
//! bound), which holds for every initial marking with no state
//! enumeration; and an **exhaustive** check of the reachability graph,
//! which holds for the analyzed initial marking and is complete only when
//! the state space fits the configured limit.
//!
//! Unlike go-pflow's `Verifier`, this one does not cache the reachability
//! graph or the P-invariant basis across calls (Go's `analyzed`/`pbasis`
//! fields) — each check recomputes what it needs. That trades some
//! repeated work for not needing interior mutability to satisfy the borrow
//! checker; nothing here is on a hot path (correctness parity, not
//! performance, is Phase 2's exit criterion). Revisit if a caller checks
//! many properties against a large net in one call.
//!
//! go-pflow's `Verifier` also expands base color-place names to the sum of
//! their expanded colors (`expandExprColors`, `markingMatches`), since it
//! is built on Shape A's `petri.PetriNet`. This crate builds on
//! `pflow-metamodel`'s already-unfolded Shape B `Model` (colors are
//! resolved upstream, in `pflow-core`/`pflow-parser`), so there is no
//! color map here and no base-name expansion to perform.

use std::collections::HashMap;

use pflow_metamodel::Model;
use pflow_reachability::{Analyzer, Invariant, InvariantAnalyzer, Marking};

use crate::property::{
    parse_expr, Counterexample, Kind, LinearExpr, Method, Property, Relation, Report, Status, Verdict,
};

/// Bounds the exhaustive search. Beyond this, undecided properties come
/// back `Unknown` rather than silently passing on a partial exploration.
pub const DEFAULT_MAX_STATES: usize = 20_000;

/// Checks properties against a model.
pub struct Verifier<'m> {
    model: &'m Model,
    initial: Marking,
    max_states: usize,
}

impl<'m> Verifier<'m> {
    /// Creates a verifier using the model's own initial marking.
    pub fn new(model: &'m Model) -> Self {
        Verifier { model, initial: model.initial_marking(), max_states: DEFAULT_MAX_STATES }
    }

    pub fn with_initial_marking(mut self, marking: Marking) -> Self {
        self.initial = marking;
        self
    }

    pub fn with_max_states(mut self, n: usize) -> Self {
        self.max_states = n;
        self
    }

    fn analyzer(&self) -> Analyzer<'m> {
        Analyzer::new(self.model).with_initial_marking(self.initial.clone()).with_max_states(self.max_states)
    }

    fn invariant_analyzer(&self) -> InvariantAnalyzer<'m> {
        InvariantAnalyzer::new(self.model)
    }

    /// Checks a set of properties and returns a combined report.
    pub fn check(&self, props: &[Property]) -> Report {
        let mut report = Report { verdicts: Vec::with_capacity(props.len()), ..Default::default() };

        for p in props {
            report.verdicts.push(self.check_one(p.clone()));
        }
        for v in &report.verdicts {
            match v.status {
                Status::Proved => report.proved += 1,
                Status::Refuted => report.refuted += 1,
                Status::Unknown => report.unknown += 1,
            }
        }
        report.ok = report.refuted == 0 && report.unknown == 0;

        // Only report exploration stats if an exhaustive check actually ran.
        if report.verdicts.iter().any(|v| matches!(v.method, Method::Exhaustive | Method::Partial)) {
            let r = self.analyzer().analyze();
            report.state_count = r.state_count;
            report.truncated = r.truncated;
        }

        report
    }

    /// Checks a single property.
    pub fn check_one(&self, p: Property) -> Verdict {
        match p.kind {
            Some(Kind::DeadlockFree) => self.check_deadlock_free(p),
            Some(Kind::Bounded) => self.check_bounded(p),
            Some(Kind::Live) => self.check_live(p),
            Some(Kind::Terminating) => self.check_terminating(p),
            Some(Kind::Reachable) => self.check_reachable(p, true),
            Some(Kind::Unreachable) => self.check_reachable(p, false),
            Some(Kind::Invariant) => self.check_invariant(p),
            Some(Kind::MutualExclusion) => self.check_mutual_exclusion(p),
            Some(Kind::Conserves) => self.check_conserves(p),
            None => {
                let mut v = Verdict::new(p, Status::Unknown, Method::None);
                v.detail = "property has no kind".to_string();
                v
            }
        }
    }

    // -----------------------------------------------------------------
    // Behavioral properties
    // -----------------------------------------------------------------

    fn check_deadlock_free(&self, p: Property) -> Verdict {
        let r = self.analyzer().analyze();

        if r.has_deadlock && !r.deadlocks.is_empty() {
            let dead_key = &r.deadlocks[0];
            let dead_marking = r.graph.states[dead_key].marking.clone();
            let mut v = Verdict::new(p, Status::Refuted, method_for(r.truncated));
            v.detail = format!("{} deadlock state(s) reachable", r.deadlocks.len());
            v.counterexample =
                Some(self.counterexample(&dead_marking, "no transition is enabled in this marking"));
            return v;
        }

        if r.truncated {
            let mut v = Verdict::new(p, Status::Unknown, Method::Partial);
            v.detail = format!(
                "no deadlock in the {} states explored, but the state space was truncated",
                r.state_count
            );
            return v;
        }

        let mut v = Verdict::new(p, Status::Proved, Method::Exhaustive);
        v.detail = format!("no deadlock among all {} reachable states", r.state_count);
        v
    }

    fn check_bounded(&self, p: Property) -> Verdict {
        if self.invariant_analyzer().structural_boundedness() {
            let mut v = Verdict::new(p, Status::Proved, Method::Structural);
            v.detail = "every place is covered by a P-invariant, so the net is bounded for any initial marking"
                .to_string();
            v.evidence = self.describe_basis();
            return v;
        }

        if let Some(w) = self.analyzer().find_unbounded_witness() {
            let mut trace = w.prefix.clone();
            trace.extend(w.pump.clone());
            let mut v = Verdict::new(p, Status::Refuted, Method::Witness);
            v.detail = format!("place(s) {} grow without bound", w.places.join(", "));
            v.counterexample = Some(Counterexample {
                trace,
                marking: w.to.clone(),
                explanation: format!(
                    "after the prefix [{}], repeating [{}] returns to a marking that strictly covers {:?}, adding tokens to {} each time — so the count is unbounded",
                    format_trace(&w.prefix),
                    format_trace(&w.pump),
                    w.from,
                    w.places.join(", ")
                ),
            });
            return v;
        }

        let r = self.analyzer().analyze();
        if r.truncated {
            let mut v = Verdict::new(p, Status::Unknown, Method::Partial);
            v.detail = format!(
                "bounded across the {} states explored, but the state space was truncated",
                r.state_count
            );
            return v;
        }

        let mut v = Verdict::new(p, Status::Proved, Method::Exhaustive);
        v.detail =
            format!("bounded across all {} reachable states from this initial marking", r.state_count);
        v
    }

    fn check_live(&self, p: Property) -> Verdict {
        let r = self.analyzer().analyze();

        let dead = if r.confirmed_dead.is_empty() && !r.truncated { &r.dead_trans } else { &r.confirmed_dead };

        if !dead.is_empty() {
            let mut sorted = dead.clone();
            sorted.sort();
            let mut v = Verdict::new(p, Status::Refuted, method_for(r.truncated));
            v.detail = format!("transition(s) can never fire: {}", sorted.join(", "));
            v.counterexample = Some(Counterexample {
                trace: Vec::new(),
                marking: self.initial.clone(),
                explanation: format!(
                    "no firing sequence from the initial marking enables {}",
                    sorted.join(", ")
                ),
            });
            return v;
        }

        if r.truncated {
            let mut detail = format!(
                "all transitions fired within the {} states explored, but the state space was truncated",
                r.state_count
            );
            if !r.potentially_dead.is_empty() {
                let mut sorted = r.potentially_dead.clone();
                sorted.sort();
                detail = format!(
                    "did not observe {} firing in the {} states explored; state space truncated",
                    sorted.join(", "),
                    r.state_count
                );
            }
            let mut v = Verdict::new(p, Status::Unknown, Method::Partial);
            v.detail = detail;
            return v;
        }

        let mut v = Verdict::new(p, Status::Proved, Method::Exhaustive);
        v.detail = format!("all {} transitions can fire from some reachable state", self.model.transitions.len());
        v
    }

    fn check_terminating(&self, p: Property) -> Verdict {
        let r = self.analyzer().analyze();

        if r.has_cycle {
            let explanation = if !r.cycles.is_empty() {
                format!(
                    "the firing sequence {} returns to a previous marking and can repeat forever",
                    r.cycles[0].join(" \u{2192} ")
                )
            } else {
                "a cycle in the reachability graph means some execution never terminates".to_string()
            };
            let mut v = Verdict::new(p, Status::Refuted, method_for(r.truncated));
            v.detail = "the net has a cyclic execution and so does not always terminate".to_string();
            v.counterexample = Some(Counterexample {
                trace: r.cycles.first().cloned().unwrap_or_default(),
                marking: self.initial.clone(),
                explanation,
            });
            return v;
        }

        if r.truncated {
            let mut v = Verdict::new(p, Status::Unknown, Method::Partial);
            v.detail =
                format!("no cycle among the {} states explored, but the state space was truncated", r.state_count);
            return v;
        }

        let mut v = Verdict::new(p, Status::Proved, Method::Exhaustive);
        v.detail = format!(
            "the reachability graph is acyclic across all {} states, so every execution terminates",
            r.state_count
        );
        v
    }

    fn check_reachable(&self, p: Property, want: bool) -> Verdict {
        if p.target.is_empty() {
            let mut v = Verdict::new(p, Status::Unknown, Method::None);
            v.detail = "property requires a target marking".to_string();
            return v;
        }

        let unknown = self.unknown_target_places(&p.target);
        if !unknown.is_empty() {
            let mut v = Verdict::new(p, Status::Unknown, Method::None);
            v.detail = format!("target references places not in the net: {}", unknown.join(", "));
            return v;
        }

        let r = self.analyzer().analyze();

        let mut matched: Option<Marking> = None;
        for state in r.graph.states_list() {
            if marking_matches(&state.marking, &p.target) {
                matched = Some(state.marking.clone());
                break;
            }
        }

        if want {
            if let Some(m) = matched {
                let trace = self.analyzer().path_to(&m).unwrap_or_default();
                let mut v = Verdict::new(p, Status::Proved, method_for(r.truncated));
                v.detail = format!("target reachable in {} firing(s)", trace.len());
                v.evidence = format!("firing sequence: {}", format_trace(&trace));
                return v;
            }
            if r.truncated {
                let mut v = Verdict::new(p, Status::Unknown, Method::Partial);
                v.detail =
                    format!("target not found in the {} states explored; state space truncated", r.state_count);
                return v;
            }
            let mut v = Verdict::new(p, Status::Refuted, Method::Exhaustive);
            v.detail = format!("target is not reachable — searched all {} reachable states", r.state_count);
            v.counterexample = Some(Counterexample {
                trace: Vec::new(),
                marking: self.initial.clone(),
                explanation: "no firing sequence from the initial marking produces the target marking"
                    .to_string(),
            });
            return v;
        }

        // Unreachable
        if let Some(m) = matched {
            let trace = self.analyzer().path_to(&m).unwrap_or_default();
            let mut v = Verdict::new(p, Status::Refuted, method_for(r.truncated));
            v.detail = format!("the marking asserted unreachable is reachable in {} firing(s)", trace.len());
            v.counterexample = Some(Counterexample {
                trace,
                marking: m,
                explanation: "this marking was asserted to be unreachable, but the trace above reaches it"
                    .to_string(),
            });
            return v;
        }
        if r.truncated {
            let mut v = Verdict::new(p, Status::Unknown, Method::Partial);
            v.detail =
                format!("not reached within the {} states explored, but the state space was truncated", r.state_count);
            return v;
        }
        let mut v = Verdict::new(p, Status::Proved, Method::Exhaustive);
        v.detail = format!("marking is unreachable — searched all {} reachable states", r.state_count);
        v
    }

    // -----------------------------------------------------------------
    // Linear-relation properties
    // -----------------------------------------------------------------

    fn check_invariant(&self, p: Property) -> Verdict {
        let expr = match parse_expr(&p.expr) {
            Ok(e) => e,
            Err(e) => {
                let mut v = Verdict::new(p, Status::Unknown, Method::None);
                v.detail = format!("could not parse expression: {e}");
                return v;
            }
        };

        let unknown = self.unknown_places(&expr);
        if !unknown.is_empty() {
            let mut v = Verdict::new(p, Status::Unknown, Method::None);
            v.detail = format!("expression references places not in the net: {}", unknown.join(", "));
            return v;
        }

        if let Some(verdict) = self.prove_structurally(&p, &expr) {
            return verdict;
        }
        self.prove_exhaustively(p, &expr)
    }

    /// Attempts a marking-independent proof from the incidence matrix.
    /// Returns `None` when no structural argument applies.
    fn prove_structurally(&self, p: &Property, expr: &LinearExpr) -> Option<Verdict> {
        match expr.rel {
            Relation::Eq if self.is_invariant_vector(&expr.coeffs) => {
                let actual = expr.eval(&self.initial);
                if actual == expr.constant {
                    let mut v = Verdict::new(p.clone(), Status::Proved, Method::Structural);
                    v.detail =
                        format!("{} is a P-invariant of the net and holds at the initial marking", expr.render_lhs());
                    v.evidence = format!(
                        "y*C = 0 for y = {}; value at initial marking = {}",
                        expr.render_lhs(),
                        actual
                    );
                    return Some(v);
                }
                let mut v = Verdict::new(p.clone(), Status::Refuted, Method::Structural);
                v.detail = format!("{} is invariant but equals {}, not {}", expr.render_lhs(), actual, expr.constant);
                v.counterexample = Some(Counterexample {
                    trace: Vec::new(),
                    marking: self.initial.clone(),
                    explanation: format!(
                        "at the initial marking {} = {actual}, and being invariant it never changes",
                        expr.render_lhs()
                    ),
                });
                return Some(v);
            }
            Relation::Le => {
                if let Some(inv) = self.dominating_invariant(expr) {
                    let mut v = Verdict::new(p.clone(), Status::Proved, Method::Structural);
                    v.detail = format!("{} is bounded by the P-invariant {}", expr.render_lhs(), inv.render());
                    v.evidence = format!(
                        "P-invariant {} dominates the asserted expression, and its constant is {} <= {}",
                        inv.render(),
                        inv.value,
                        expr.constant
                    );
                    return Some(v);
                }
            }
            _ => {}
        }
        None
    }

    /// Whether `y*C == 0` for the given place coefficients — the
    /// definition of a P-invariant.
    fn is_invariant_vector(&self, coeffs: &HashMap<String, i64>) -> bool {
        let (matrix, places, transitions) = self.invariant_analyzer().incidence_matrix();
        if transitions.is_empty() {
            return true;
        }
        // `j` indexes matrix columns (transitions); `matrix` is rows
        // (places) x cols, so this can't become a `.iter()` over either
        // axis alone without transposing.
        #[allow(clippy::needless_range_loop)]
        for j in 0..transitions.len() {
            let sum: i64 = places
                .iter()
                .enumerate()
                .filter_map(|(i, place)| coeffs.get(place).filter(|&&c| c != 0).map(|&c| c * matrix[i][j]))
                .sum();
            if sum != 0 {
                return false;
            }
        }
        true
    }

    /// Looks for a semi-positive P-invariant whose coefficients are `>=`
    /// the expression's everywhere and whose constant satisfies the bound.
    fn dominating_invariant(&self, expr: &LinearExpr) -> Option<Invariant> {
        if expr.coeffs.values().any(|&c| c < 0) {
            return None;
        }
        for inv in self.invariant_analyzer().find_p_invariants(&self.initial) {
            let dominates = expr.coeffs.iter().all(|(place, &c)| {
                inv.coefficients.get(place).map(|&ic| ic >= c).unwrap_or(false)
            });
            if !dominates {
                continue;
            }
            if inv.coefficients.values().any(|&ic| ic < 0) {
                continue;
            }
            if inv.value <= expr.constant {
                return Some(inv);
            }
        }
        None
    }

    /// Checks the relation at every reachable marking.
    fn prove_exhaustively(&self, p: Property, expr: &LinearExpr) -> Verdict {
        let r = self.analyzer().analyze();

        // Deterministic counterexample selection: report the shallowest
        // violating state so the replayable trace is the shortest one
        // available.
        let mut worst: Option<(&str, i64, &Marking)> = None;
        for state in r.graph.states_list() {
            if expr.holds(&state.marking) {
                continue;
            }
            let better = match worst {
                None => true,
                Some((wk, wd, _)) => state.depth < wd || (state.depth == wd && state.key.as_str() < wk),
            };
            if better {
                worst = Some((state.key.as_str(), state.depth, &state.marking));
            }
        }

        if let Some((_, _, marking)) = worst {
            let value = expr.eval(marking);
            let mut v = Verdict::new(p, Status::Refuted, method_for(r.truncated));
            v.detail = format!("{} fails: {} = {} at a reachable marking", expr.render(), expr.render_lhs(), value);
            v.counterexample = Some(self.counterexample(
                marking,
                &format!("{} = {value} here, violating {}", expr.render_lhs(), expr.render()),
            ));
            return v;
        }

        if r.truncated {
            let mut v = Verdict::new(p, Status::Unknown, Method::Partial);
            v.detail = format!(
                "{} holds across the {} states explored, but the state space was truncated",
                expr.render(),
                r.state_count
            );
            return v;
        }

        let mut v = Verdict::new(p, Status::Proved, Method::Exhaustive);
        v.detail = format!("{} holds at all {} reachable states", expr.render(), r.state_count);
        v
    }

    fn check_mutual_exclusion(&self, p: Property) -> Verdict {
        if p.places.is_empty() {
            let mut v = Verdict::new(p, Status::Unknown, Method::None);
            v.detail = "property requires at least one place".to_string();
            return v;
        }
        let bound = if p.bound == 0 { 1 } else { p.bound };

        let quoted: Vec<String> = p.places.iter().map(|s| format!("\"{s}\"")).collect();
        let mut expanded = p.clone();
        expanded.kind = Some(Kind::Invariant);
        expanded.expr = format!("{} <= {bound}", quoted.join(" + "));

        let mut verdict = self.check_invariant(expanded);
        verdict.property = p;
        verdict
    }

    fn check_conserves(&self, p: Property) -> Verdict {
        let mut places: Vec<String> = self.model.places.iter().filter(|pl| pl.is_token()).map(|pl| pl.id.clone()).collect();
        places.sort();

        if places.is_empty() {
            let mut v = Verdict::new(p, Status::Proved, Method::Structural);
            v.detail = "net has no places".to_string();
            return v;
        }

        let quoted: Vec<String> = places.iter().map(|s| format!("\"{s}\"")).collect();
        let total: i64 = self.initial.values().sum();
        let mut expanded = p.clone();
        expanded.kind = Some(Kind::Invariant);
        expanded.expr = format!("{} == {total}", quoted.join(" + "));

        let mut verdict = self.check_invariant(expanded);
        verdict.property = p;
        verdict
    }

    // -----------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------

    fn counterexample(&self, m: &Marking, explanation: &str) -> Counterexample {
        let trace = self.analyzer().path_to(m).unwrap_or_default();
        Counterexample { trace, marking: m.clone(), explanation: explanation.to_string() }
    }

    /// Expression place names that don't exist in the net — almost always a
    /// typo, worth reporting rather than silently treating the place as
    /// holding zero tokens.
    fn unknown_places(&self, expr: &LinearExpr) -> Vec<String> {
        let mut unknown: Vec<String> =
            expr.places().into_iter().filter(|p| self.model.place_by_id(p).is_none()).collect();
        unknown.sort();
        unknown
    }

    fn unknown_target_places(&self, target: &HashMap<String, i64>) -> Vec<String> {
        let mut unknown: Vec<String> =
            target.keys().filter(|p| self.model.place_by_id(p).is_none()).cloned().collect();
        unknown.sort();
        unknown
    }

    fn describe_basis(&self) -> String {
        let invs = self.invariant_analyzer().find_p_invariants(&self.initial);
        if invs.is_empty() {
            return String::new();
        }
        let mut parts: Vec<String> = invs.iter().map(|i| i.render()).collect();
        parts.sort();
        format!("P-invariants: {}", parts.join("; "))
    }
}

/// Whether `m` agrees with `target` on every place `target` names. Places
/// absent from `target` are unconstrained.
fn marking_matches(m: &Marking, target: &HashMap<String, i64>) -> bool {
    target.iter().all(|(place, &want)| m.get(place).copied().unwrap_or(0) == want)
}

fn method_for(truncated: bool) -> Method {
    if truncated {
        Method::Partial
    } else {
        Method::Exhaustive
    }
}

fn format_trace(trace: &[String]) -> String {
    if trace.is_empty() {
        "(already satisfied at the initial marking)".to_string()
    } else {
        trace.join(" \u{2192} ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::schema::{Arc, ArcType as AT, Place, Transition};

    /// mutex.pflow: two processes racing for a one-slot critical section.
    /// p0/p1 idle -> enter -> busy1/busy2 -> exit -> back to idle.
    /// busy1 + busy2 <= 1 always; live; bounded; deadlock-free; not
    /// terminating (cycles back to idle).
    fn mutex_model() -> Model {
        Model {
            name: "mutex".into(),
            places: vec![
                Place { id: "idle1".into(), initial: 1, ..Default::default() },
                Place { id: "idle2".into(), initial: 1, ..Default::default() },
                Place { id: "busy1".into(), ..Default::default() },
                Place { id: "busy2".into(), ..Default::default() },
                Place { id: "lock".into(), initial: 1, ..Default::default() },
            ],
            transitions: vec![
                Transition { id: "enter1".into(), ..Default::default() },
                Transition { id: "exit1".into(), ..Default::default() },
                Transition { id: "enter2".into(), ..Default::default() },
                Transition { id: "exit2".into(), ..Default::default() },
            ],
            arcs: vec![
                Arc { from: "idle1".into(), to: "enter1".into(), ..Default::default() },
                Arc { from: "lock".into(), to: "enter1".into(), ..Default::default() },
                Arc { from: "enter1".into(), to: "busy1".into(), ..Default::default() },
                Arc { from: "busy1".into(), to: "exit1".into(), ..Default::default() },
                Arc { from: "exit1".into(), to: "idle1".into(), ..Default::default() },
                Arc { from: "exit1".into(), to: "lock".into(), ..Default::default() },
                Arc { from: "idle2".into(), to: "enter2".into(), ..Default::default() },
                Arc { from: "lock".into(), to: "enter2".into(), ..Default::default() },
                Arc { from: "enter2".into(), to: "busy2".into(), ..Default::default() },
                Arc { from: "busy2".into(), to: "exit2".into(), ..Default::default() },
                Arc { from: "exit2".into(), to: "idle2".into(), ..Default::default() },
                Arc { from: "exit2".into(), to: "lock".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    /// A linear chain with a terminal, non-empty end state (a deadlock
    /// under the shared heuristic: terminal + non-zero + started non-empty).
    fn chain_model() -> Model {
        Model {
            name: "chain".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn mutual_exclusion_holds_structurally() {
        let model = mutex_model();
        let v = Verifier::new(&model);
        let verdict = v.check_one(Property::mutual_exclusion(
            vec!["busy1".to_string(), "busy2".to_string()],
            1,
        ));
        assert_eq!(verdict.status, Status::Proved);
        assert_eq!(verdict.method, Method::Structural);
    }

    #[test]
    fn deadlock_free_holds_for_the_mutex() {
        let model = mutex_model();
        let v = Verifier::new(&model);
        let verdict = v.check_one(Property::new(Kind::DeadlockFree));
        assert_eq!(verdict.status, Status::Proved);
    }

    #[test]
    fn bounded_and_live_hold_for_the_mutex() {
        let model = mutex_model();
        let v = Verifier::new(&model);
        assert_eq!(v.check_one(Property::new(Kind::Bounded)).status, Status::Proved);
        assert_eq!(v.check_one(Property::new(Kind::Live)).status, Status::Proved);
    }

    #[test]
    fn not_terminating_because_the_mutex_cycles() {
        let model = mutex_model();
        let v = Verifier::new(&model);
        let verdict = v.check_one(Property::new(Kind::Terminating));
        assert_eq!(verdict.status, Status::Refuted);
        assert!(verdict.counterexample.is_some());
    }

    #[test]
    fn conserves_holds_for_a_two_place_cycle() {
        // The mutex model above deliberately does NOT conserve the total
        // token count: entering consumes 2 tokens (idle + lock) but
        // produces 1 (busy), so the total drops from 3 to 2. A simple
        // p0<->p1 cycle is what actually conserves a single token.
        let model = Model {
            name: "cycle".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![
                Transition { id: "t0".into(), ..Default::default() },
                Transition { id: "t1".into(), ..Default::default() },
            ],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
                Arc { from: "p1".into(), to: "t1".into(), ..Default::default() },
                Arc { from: "t1".into(), to: "p0".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let v = Verifier::new(&model);
        let verdict = v.check_one(Property::new(Kind::Conserves));
        assert_eq!(verdict.status, Status::Proved);
        assert_eq!(verdict.method, Method::Structural);
    }

    #[test]
    fn chain_terminates_and_is_a_deadlock() {
        let model = chain_model();
        let v = Verifier::new(&model);
        assert_eq!(v.check_one(Property::new(Kind::Terminating)).status, Status::Proved);
        let dl = v.check_one(Property::new(Kind::DeadlockFree));
        assert_eq!(dl.status, Status::Refuted, "terminal non-empty marking counts as a deadlock");
    }

    #[test]
    fn reachable_and_unreachable_targets() {
        let model = chain_model();
        let v = Verifier::new(&model);

        let mut target = HashMap::new();
        target.insert("p1".to_string(), 1);
        let reach = v.check_one(Property::reachable(target.clone()));
        assert_eq!(reach.status, Status::Proved);

        let mut bogus = HashMap::new();
        bogus.insert("p0".to_string(), 5);
        let unreach = v.check_one(Property::unreachable(bogus));
        assert_eq!(unreach.status, Status::Proved);
    }

    #[test]
    fn unknown_target_place_is_unknown_not_refuted() {
        let model = chain_model();
        let v = Verifier::new(&model);
        let mut target = HashMap::new();
        target.insert("does_not_exist".to_string(), 1);
        let verdict = v.check_one(Property::reachable(target));
        assert_eq!(verdict.status, Status::Unknown);
    }

    #[test]
    fn invariant_refuted_reports_a_counterexample() {
        let model = chain_model();
        let v = Verifier::new(&model);
        let verdict = v.check_one(Property::invariant("p0 == 1"));
        assert_eq!(verdict.status, Status::Refuted);
        assert!(verdict.counterexample.is_some());
    }

    #[test]
    fn report_aggregates_counts_and_ok_flag() {
        let model = mutex_model();
        let v = Verifier::new(&model);
        let report = v.check(&[
            Property::new(Kind::DeadlockFree),
            Property::new(Kind::Bounded),
            Property::new(Kind::Terminating), // refuted: mutex cycles
        ]);
        assert_eq!(report.proved, 2);
        assert_eq!(report.refuted, 1);
        assert!(!report.ok);
    }

    #[test]
    fn inhibitor_arc_gates_the_incidence_matrix_not_the_invariant() {
        // A single guarded transition: p0 -> t0 -> p1, gated by an
        // inhibitor on "guard". The inhibitor moves no tokens, so p0+p1==1
        // is still a structural P-invariant.
        let model = Model {
            name: "guarded".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
                Place { id: "guard".into(), ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "guard".into(), to: "t0".into(), typ: AT::Inhibitor, weight: 1, ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let v = Verifier::new(&model);
        let verdict = v.check_one(Property::invariant("p0 + p1 == 1"));
        assert_eq!(verdict.status, Status::Proved);
        assert_eq!(verdict.method, Method::Structural);
    }
}
