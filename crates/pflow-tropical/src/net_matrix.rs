//! Generic incidence matrix type with Petri net firing semantics.
//!
//! `NetMatrix` owns the dense incidence data, input weights, initial marking,
//! and labels. It provides firing, enabledness, and tropical adjacency without
//! depending on any pflow crate.

use std::fmt;

use crate::semiring::Matrix;

/// A place that breaks the event-graph property: it does not have exactly one
/// producer and one consumer. Counts are in tokens, so an arc of weight 2 counts
/// twice, as it does in pflow-jl's stoichiometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventGraphViolation {
    pub place: String,
    pub producers: i64,
    pub consumers: i64,
}

/// `rho(t) = |pre(t)| / |post(t)|` for one transition, counted in tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionRho {
    pub transition: String,
    pub pre: i64,
    pub post: i64,
    /// `pre / post`, or `None` for a transition with no post-set. Above 1 is
    /// fan-in, the algebraic boundary between a net's core and an observer.
    pub rho: Option<f64>,
}

/// Whether a net is an event graph, and if not, why. The event graph is the
/// domain on which the tropical eigenvalue is defined. Lists follow the matrix's
/// place and transition order. See [`NetMatrix::classify_event_graph`].
#[derive(Debug, Clone, PartialEq)]
pub struct EventGraphReport {
    pub violations: Vec<EventGraphViolation>,
    pub rho: Vec<TransitionRho>,
}

impl EventGraphReport {
    /// True when every place has exactly one producer and one consumer.
    pub fn is_event_graph(&self) -> bool {
        self.violations.is_empty()
    }
}

impl fmt::Display for EventGraphReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_event_graph() {
            return write!(f, "the net is an event graph");
        }
        write!(f, "not an event graph: place(s) without exactly one producer and one consumer: ")?;
        for (i, v) in self.violations.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{} ({} in, {} out)", v.place, v.producers, v.consumers)?;
        }
        Ok(())
    }
}

impl std::error::Error for EventGraphReport {}

/// A standalone Petri net representation as dense matrices.
///
/// - `incidence[t][p]` = net token change when transition `t` fires (output - input)
/// - `input[t][p]` = tokens consumed (needed for enabledness with catalytic arcs)
/// - `initial[p]` = initial marking
#[derive(Debug, Clone)]
pub struct NetMatrix {
    pub incidence: Vec<Vec<i64>>,
    pub input: Vec<Vec<i64>>,
    pub initial: Vec<i64>,
    pub place_labels: Vec<String>,
    pub transition_labels: Vec<String>,
}

impl NetMatrix {
    /// Create a `NetMatrix` with explicit input weights (supports catalytic arcs).
    pub fn new(
        incidence: Vec<Vec<i64>>,
        input: Vec<Vec<i64>>,
        initial: Vec<i64>,
        place_labels: Vec<String>,
        transition_labels: Vec<String>,
    ) -> Self {
        let nt = incidence.len();
        let np = if nt > 0 { incidence[0].len() } else { 0 };
        assert_eq!(input.len(), nt, "input rows must match transitions");
        assert_eq!(initial.len(), np, "initial must match places");
        for (t, row) in incidence.iter().enumerate() {
            assert_eq!(row.len(), np, "incidence row {t} length mismatch");
        }
        for (t, row) in input.iter().enumerate() {
            assert_eq!(row.len(), np, "input row {t} length mismatch");
        }
        Self { incidence, input, initial, place_labels, transition_labels }
    }

    /// Convenience: derive input weights from negative incidence entries.
    ///
    /// Correct for non-catalytic nets where consuming `k` tokens means
    /// `incidence[t][p] = -k` (no simultaneous produce to same place).
    pub fn from_incidence(
        incidence: Vec<Vec<i64>>,
        initial: Vec<i64>,
        place_labels: Vec<String>,
        transition_labels: Vec<String>,
    ) -> Self {
        let input: Vec<Vec<i64>> = incidence
            .iter()
            .map(|row| row.iter().map(|&v| if v < 0 { -v } else { 0 }).collect())
            .collect();
        Self::new(incidence, input, initial, place_labels, transition_labels)
    }

    pub fn num_places(&self) -> usize {
        self.initial.len()
    }

    pub fn num_transitions(&self) -> usize {
        self.incidence.len()
    }

    /// Delta vector for transition `t`: the net token change per place.
    pub fn delta(&self, t: usize) -> &[i64] {
        &self.incidence[t]
    }

    /// Check if transition `t` is enabled at the given marking.
    pub fn is_enabled(&self, marking: &[i64], t: usize) -> bool {
        self.input[t].iter().enumerate().all(|(p, &w)| marking[p] >= w)
    }

    /// Fire transition `t` at the given marking. Returns `None` if not enabled.
    pub fn fire(&self, marking: &[i64], t: usize) -> Option<Vec<i64>> {
        if !self.is_enabled(marking, t) {
            return None;
        }
        let next: Vec<i64> = marking
            .iter()
            .enumerate()
            .map(|(p, &m)| m + self.incidence[t][p])
            .collect();
        // Check non-negative (sanity)
        if next.iter().any(|&v| v < 0) {
            return None;
        }
        Some(next)
    }

    /// Classifies the net against the event-graph domain. The structured form
    /// of pflow-jl's `is_event_graph`, plus each transition's rho.
    pub fn classify_event_graph(&self) -> EventGraphReport {
        let output = |t: usize, p: usize| self.incidence[t][p] + self.input[t][p];
        let (np, nt) = (self.num_places(), self.num_transitions());
        let violations = (0..np)
            .map(|p| EventGraphViolation {
                place: self.place_labels[p].clone(),
                producers: (0..nt).map(|t| output(t, p)).sum(),
                consumers: (0..nt).map(|t| self.input[t][p]).sum(),
            })
            .filter(|v| v.producers != 1 || v.consumers != 1)
            .collect();
        let rho = (0..nt)
            .map(|t| {
                let pre: i64 = (0..np).map(|p| self.input[t][p]).sum();
                let post: i64 = (0..np).map(|p| output(t, p)).sum();
                TransitionRho {
                    transition: self.transition_labels[t].clone(),
                    pre,
                    post,
                    rho: (post != 0).then(|| pre as f64 / post as f64),
                }
            })
            .collect();
        EventGraphReport { violations, rho }
    }

    /// [`Self::tropical_adjacency`], refusing a net outside the event-graph
    /// domain, with the report saying why.
    pub fn checked_tropical_adjacency(&self) -> Result<Matrix, EventGraphReport> {
        let report = self.classify_event_graph();
        if report.is_event_graph() {
            Ok(self.adjacency())
        } else {
            Err(report)
        }
    }

    /// Build the tropical adjacency matrix (transition-to-transition).
    ///
    /// `A[i][j]` = weight of the path from transition `j` through an
    /// intermediate place to transition `i`. NEG_INF means no connection.
    ///
    /// The eigenvalue of this matrix means a cycle time only on an event graph.
    /// Off that domain this still returns the matrix and writes one warning to
    /// stderr: the matrix is well defined and useful as a structural summary,
    /// and existing callers use it on nets like tic-tac-toe. Use
    /// [`Self::checked_tropical_adjacency`] to refuse such a net instead.
    pub fn tropical_adjacency(&self) -> Matrix {
        if let Some(warning) = self.domain_warning() {
            eprintln!("{warning}");
        }
        self.adjacency()
    }

    fn domain_warning(&self) -> Option<String> {
        let report = self.classify_event_graph();
        (!report.is_event_graph())
            .then(|| format!("warning: pflow-tropical: tropical_adjacency: {report}"))
    }

    fn adjacency(&self) -> Matrix {
        let nt = self.num_transitions();
        let np = self.num_places();
        let mut a = Matrix::new(nt, nt);

        // Compute output weights: output[t][p] = incidence[t][p] + input[t][p]
        // (since incidence = output - input)
        for p in 0..np {
            // Find producers: transitions with positive output to place p
            let mut producers: Vec<(usize, i64)> = Vec::new();
            for t in 0..nt {
                let out_w = self.incidence[t][p] + self.input[t][p];
                if out_w > 0 {
                    producers.push((t, out_w));
                }
            }
            // Find consumers: transitions that consume from place p
            let mut consumers: Vec<(usize, i64)> = Vec::new();
            for t in 0..nt {
                if self.input[t][p] > 0 {
                    consumers.push((t, self.input[t][p]));
                }
            }
            for &(producer, _) in &producers {
                for &(consumer, cw) in &consumers {
                    let candidate = cw as f64;
                    if candidate > a.at(consumer, producer) {
                        a.set(consumer, producer, candidate);
                    }
                }
            }
        }
        a
    }

    /// P-invariants (convenience delegation).
    pub fn p_invariants(&self) -> Vec<Vec<i64>> {
        crate::invariants::p_invariants_from_dense(&self.incidence)
    }

    /// T-invariants (convenience delegation).
    pub fn t_invariants(&self) -> Vec<Vec<i64>> {
        crate::invariants::t_invariants_from_dense(&self.incidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::NEG_INF;

    #[test]
    fn test_simple_loop() {
        let net = NetMatrix::from_incidence(
            vec![vec![-1, 1], vec![1, -1]],
            vec![1, 0],
            vec!["P0".into(), "P1".into()],
            vec!["T0".into(), "T1".into()],
        );
        assert!(net.is_enabled(&net.initial, 0));
        assert!(!net.is_enabled(&net.initial, 1));
        let m1 = net.fire(&net.initial, 0).unwrap();
        assert_eq!(m1, vec![0, 1]);
        let m2 = net.fire(&m1, 1).unwrap();
        assert_eq!(m2, vec![1, 0]);
    }

    #[test]
    fn test_catalytic_arc() {
        // Transition reads from p0 (consumes+produces) and produces to p1
        // incidence: [0, 1] but input: [1, 0]
        let net = NetMatrix::new(
            vec![vec![0, 1]],
            vec![vec![1, 0]],
            vec![1, 0],
            vec!["p0".into(), "p1".into()],
            vec!["t0".into()],
        );
        assert!(net.is_enabled(&[1, 0], 0));
        assert!(!net.is_enabled(&[0, 0], 0)); // needs token in p0
        let m1 = net.fire(&[1, 0], 0).unwrap();
        assert_eq!(m1, vec![1, 1]); // p0 unchanged (catalytic), p1 gains
    }

    #[test]
    fn test_tropical_adjacency() {
        let net = NetMatrix::from_incidence(
            vec![vec![-1, 1], vec![1, -1]],
            vec![1, 0],
            vec!["P0".into(), "P1".into()],
            vec!["T0".into(), "T1".into()],
        );
        let a = net.tropical_adjacency();
        assert_eq!(a.at(1, 0), 1.0); // T1 depends on T0 via P1
        assert_eq!(a.at(0, 1), 1.0); // T0 depends on T1 via P0
        assert_eq!(a.at(0, 0), NEG_INF);
        assert_eq!(a.at(1, 1), NEG_INF);
    }

    /// Builds a `NetMatrix` from each transition's pre- and post-sets.
    fn net(places: &[&str], flows: &[(&str, &[&str], &[&str])]) -> NetMatrix {
        let index = |p: &str| places.iter().position(|x| *x == p).unwrap();
        let mut incidence = vec![vec![0; places.len()]; flows.len()];
        let mut input = vec![vec![0; places.len()]; flows.len()];
        for (t, (_, pre, post)) in flows.iter().enumerate() {
            for p in *pre {
                incidence[t][index(p)] -= 1;
                input[t][index(p)] += 1;
            }
            for p in *post {
                incidence[t][index(p)] += 1;
            }
        }
        NetMatrix::new(
            incidence,
            input,
            vec![0; places.len()],
            places.iter().map(|s| s.to_string()).collect(),
            flows.iter().map(|f| f.0.to_string()).collect(),
        )
    }

    const CYCLE: &[(&str, &[&str], &[&str])] = &[
        ("send_ab", &["a"], &["pending_ab"]),
        ("settle_ab", &["pending_ab"], &["b"]),
        ("send_bc", &["b"], &["pending_bc"]),
        ("settle_bc", &["pending_bc"], &["c"]),
        ("send_ca", &["c"], &["pending_ca"]),
        ("settle_ca", &["pending_ca"], &["a"]),
    ];

    fn settle_cycle() -> NetMatrix {
        net(&["a", "b", "c", "pending_ab", "pending_bc", "pending_ca"], CYCLE)
    }

    fn netting() -> NetMatrix {
        let mut flows = CYCLE.to_vec();
        flows.push(("net_abc", &["pending_ab", "pending_bc", "pending_ca"], &["cleared"]));
        net(&["a", "b", "c", "pending_ab", "pending_bc", "pending_ca", "cleared"], &flows)
    }

    #[test]
    fn the_settle_cycle_is_an_event_graph() {
        let r = settle_cycle().classify_event_graph();
        assert!(r.is_event_graph());
        assert_eq!(r.violations, vec![]);
        assert!(r.rho.iter().all(|t| (t.pre, t.post, t.rho) == (1, 1, Some(1.0))));
        assert_eq!(r.to_string(), "the net is an event graph");
    }

    /// pflow-jl's testset "event graph breaks": exactly the three pending places
    /// (two consumers each) and `cleared` (a sink). The balances are untouched.
    #[test]
    fn netting_breaks_the_event_graph_at_exactly_these_places() {
        let r = netting().classify_event_graph();
        assert!(!r.is_event_graph());
        let v = |place: &str, producers, consumers| EventGraphViolation {
            place: place.to_string(),
            producers,
            consumers,
        };
        assert_eq!(
            r.violations,
            vec![
                v("pending_ab", 1, 2),
                v("pending_bc", 1, 2),
                v("pending_ca", 1, 2),
                v("cleared", 1, 0),
            ]
        );
        let net_abc = r.rho.iter().find(|t| t.transition == "net_abc").unwrap();
        assert_eq!((net_abc.pre, net_abc.post, net_abc.rho), (3, 1, Some(3.0)));
        assert_eq!(r.rho.iter().filter(|t| t.rho == Some(1.0)).count(), 6);
        assert_eq!(
            r.to_string(),
            "not an event graph: place(s) without exactly one producer and one consumer: \
             pending_ab (1 in, 2 out), pending_bc (1 in, 2 out), pending_ca (1 in, 2 out), \
             cleared (1 in, 0 out)"
        );
    }

    #[test]
    fn rho_counts_tokens_and_has_no_value_without_a_post_set() {
        // p --2--> t --1--> q: a weight-2 arc is two consumers and rho 2.
        let m = NetMatrix::from_incidence(
            vec![vec![-2, 1], vec![1, -1], vec![0, 1], vec![-1, 0]],
            vec![0, 0],
            vec!["p".into(), "q".into()],
            vec!["t".into(), "u".into(), "source".into(), "sink".into()],
        );
        let r = m.classify_event_graph();
        let of = |name: &str| r.rho.iter().find(|t| t.transition == name).unwrap();
        assert_eq!(of("t").rho, Some(2.0));
        assert_eq!(of("u").rho, Some(1.0));
        assert_eq!(of("source").rho, Some(0.0));
        assert_eq!((of("sink").pre, of("sink").post, of("sink").rho), (1, 0, None));
        assert_eq!(r.violations[0].consumers, 3);
    }

    /// A catalytic arc both consumes and produces, so it counts on both sides.
    #[test]
    fn a_catalytic_arc_counts_as_a_producer_and_a_consumer() {
        let m = NetMatrix::new(
            vec![vec![0]],
            vec![vec![1]],
            vec![1],
            vec!["p".into()],
            vec!["t".into()],
        );
        let r = m.classify_event_graph();
        assert!(r.is_event_graph());
        assert_eq!(r.rho[0].rho, Some(1.0));
    }

    #[test]
    fn checked_adjacency_refuses_outside_the_domain_with_the_report() {
        let cycle = settle_cycle();
        let a = cycle.checked_tropical_adjacency().unwrap();
        assert_eq!(a.at(1, 0), 1.0);

        let m = netting();
        let e = m.checked_tropical_adjacency().unwrap_err();
        assert_eq!(e, m.classify_event_graph());
    }

    /// Least breaking: same signature and same matrix, plus a warning.
    #[test]
    fn plain_adjacency_warns_and_still_returns_the_matrix() {
        let m = netting();
        assert_eq!(
            m.domain_warning().unwrap(),
            format!(
                "warning: pflow-tropical: tropical_adjacency: {}",
                m.classify_event_graph()
            )
        );
        assert!(settle_cycle().domain_warning().is_none());

        let a = m.tropical_adjacency();
        let b = m.adjacency();
        for i in 0..m.num_transitions() {
            for j in 0..m.num_transitions() {
                assert_eq!(a.at(i, j), b.at(i, j));
            }
        }
        // net_abc (6) consumes what send_ab (0) puts on pending_ab.
        assert_eq!(a.at(6, 0), 1.0);
    }
}
