//! [`Validator`], [`ValidationResult`] and the reachability summary embedded
//! in it. Ported from go-pflow's `validation/types.go` and
//! `validation/reachability.go`.

use pflow_metamodel::Model;
use pflow_reachability::{marking, Analyzer};
use std::collections::HashMap;

/// The severity of a validation [`Issue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// One structural finding.
#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    /// "structure", "connectivity", "deadlock", "unbounded", "conservation",
    /// "reachability".
    pub category: String,
    pub message: String,
    /// Affected places/transitions.
    pub location: Vec<String>,
    pub suggestion: String,
}

/// Net-size and pass/fail counts.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub places: usize,
    pub transitions: usize,
    pub arcs: usize,
    pub errors: usize,
    pub warnings: usize,
    pub conserved: bool,
}

/// Reachability analysis embedded in a [`ValidationResult`]. Go's
/// `validation.ReachabilityResult`.
#[derive(Debug, Clone, Default)]
pub struct ReachabilityResult {
    pub reachable: usize,
    pub bounded: bool,
    pub max_tokens: HashMap<String, i64>,
    /// Rendered markings ([`marking::render`]), sorted.
    pub terminal_states: Vec<String>,
    pub deadlock_states: Vec<String>,
    pub has_cycles: bool,
    pub max_depth: i64,
    pub truncated: bool,
    pub truncated_reason: String,
}

/// The outcome of running a [`Validator`].
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
    pub info: Vec<Issue>,
    pub summary: Summary,
    pub reachability: Option<ReachabilityResult>,
    /// The net's minimal-support P-invariants, rendered
    /// (`"3*boxes + widgets == 6"`), sorted.
    pub invariants: Vec<String>,
}

/// Runs structural validation checks against a model.
pub struct Validator<'m> {
    pub(crate) model: &'m Model,
    pub(crate) result: ValidationResult,
}

impl<'m> Validator<'m> {
    pub fn new(model: &'m Model) -> Self {
        let summary = Summary {
            places: model.places.len(),
            transitions: model.transitions.len(),
            arcs: model.arcs.len(),
            ..Default::default()
        };
        Validator {
            model,
            result: ValidationResult { valid: true, summary, ..Default::default() },
        }
    }

    pub fn model(&self) -> &'m Model {
        self.model
    }

    /// Runs every check and returns the accumulated result.
    pub fn validate(mut self) -> ValidationResult {
        self.check_structure();
        self.check_connectivity();
        self.check_deadlocks();
        self.check_unbounded();
        self.check_conservation();

        self.result.summary.errors = self.result.errors.len();
        self.result.summary.warnings = self.result.warnings.len();
        self.result.valid = self.result.errors.is_empty();

        self.result
    }

    /// [`Self::validate`] plus a full reachability analysis. Go's
    /// `Validator.ValidateWithReachability`.
    pub fn validate_with_reachability(mut self, max_states: usize) -> ValidationResult {
        self.check_structure();
        self.check_connectivity();
        self.check_deadlocks();
        self.check_unbounded();
        self.check_conservation();
        self.result.summary.errors = self.result.errors.len();
        self.result.summary.warnings = self.result.warnings.len();
        self.result.valid = self.result.errors.is_empty();

        let reach = self.analyze_reachability(max_states);

        if !reach.deadlock_states.is_empty() {
            self.add_warning(
                "reachability",
                format!(
                    "Found {} deadlock states (terminal states that are not goal states)",
                    reach.deadlock_states.len()
                ),
                Vec::new(),
                "Review model structure to ensure all terminal states are valid end states",
            );
        }
        if !reach.bounded {
            self.add_error(
                "reachability",
                "Model is unbounded (some places can accumulate tokens indefinitely)",
                Vec::new(),
                "Add capacity constraints or fix structural issues causing unbounded growth",
            );
        }
        if reach.truncated {
            self.add_warning(
                "reachability",
                format!("Reachability analysis truncated: {}", reach.truncated_reason),
                Vec::new(),
                "Consider simplifying model or increasing state limit",
            );
        }

        self.result.summary.errors = self.result.errors.len();
        self.result.summary.warnings = self.result.warnings.len();
        self.result.valid = self.result.errors.is_empty();
        self.result.reachability = Some(reach);
        self.result
    }

    /// Runs reachability analysis against the model, in the presentation
    /// shape `ValidationResult` carries. Delegates entirely to
    /// `pflow-reachability`, which is the one place BFS exploration, cycle
    /// detection and the coverability witness live — go-pflow's note on
    /// `AnalyzeReachability` (a float-based duplicate used to report "has
    /// cycles" for nearly every net) is the reason this crate never grows a
    /// second copy.
    pub fn analyze_reachability(&self, max_states: usize) -> ReachabilityResult {
        let analyzer = Analyzer::new(self.model).with_max_states(max_states);
        let res = analyzer.analyze();

        let mut result = ReachabilityResult {
            reachable: res.state_count,
            bounded: res.bounded,
            max_tokens: res.max_tokens.clone(),
            has_cycles: res.has_cycle,
            max_depth: res.max_depth,
            truncated: res.truncated,
            ..Default::default()
        };

        // A covering witness is a definite proof of unboundedness and does
        // not require exhausting the state limit.
        if result.bounded {
            let analyzer2 = Analyzer::new(self.model).with_max_states(max_states);
            if let Some(w) = analyzer2.find_unbounded_witness() {
                result.bounded = false;
                result.truncated_reason =
                    format!("unbounded: {} grow without bound", w.places.join(", "));
            }
        }

        if res.truncated && result.truncated_reason.is_empty() {
            result.truncated_reason = if res.truncate_msg.is_empty() {
                format!("state limit of {max_states} reached")
            } else {
                res.truncate_msg.clone()
            };
        }

        for state in res.graph.states.values() {
            if state.is_terminal {
                result.terminal_states.push(marking::render(&state.marking));
            }
            if state.is_deadlock {
                result.deadlock_states.push(marking::render(&state.marking));
            }
        }
        result.terminal_states.sort();
        result.deadlock_states.sort();

        result
    }

    pub fn add_error(
        &mut self,
        category: impl Into<String>,
        message: impl Into<String>,
        location: Vec<String>,
        suggestion: impl Into<String>,
    ) {
        self.result.errors.push(Issue {
            severity: Severity::Error,
            category: category.into(),
            message: message.into(),
            location,
            suggestion: suggestion.into(),
        });
    }

    pub fn add_warning(
        &mut self,
        category: impl Into<String>,
        message: impl Into<String>,
        location: Vec<String>,
        suggestion: impl Into<String>,
    ) {
        self.result.warnings.push(Issue {
            severity: Severity::Warning,
            category: category.into(),
            message: message.into(),
            location,
            suggestion: suggestion.into(),
        });
    }

    pub fn add_info(&mut self, category: impl Into<String>, message: impl Into<String>, location: Vec<String>) {
        self.result.info.push(Issue {
            severity: Severity::Info,
            category: category.into(),
            message: message.into(),
            location,
            suggestion: String::new(),
        });
    }
}
