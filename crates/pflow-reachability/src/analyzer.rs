//! Reachability analysis: BFS graph construction, cycle detection and
//! liveness, ported from go-pflow's `reachability/analyzer.go`.

use std::collections::{HashMap, HashSet, VecDeque};

use pflow_metamodel::Model;

use crate::graph::Graph;
use crate::marking::{self, Marking};

/// Metrics about a BFS exploration.
#[derive(Debug, Clone, Default)]
pub struct ExplorationStats {
    pub states_explored: usize,
    pub states_limit: usize,
    pub tokens_limit: i64,
    pub queue_max_size: usize,
    /// Average enabled transitions per state that had at least one.
    pub branching_factor: f64,
    /// Rough estimate of the explored fraction of the full state space.
    pub exploration_ratio: f64,
}

/// The result of building (and optionally fully analyzing) a reachability
/// graph. Owns the [`Graph`] so callers can look states up by key after the
/// fact — mirroring Go's `Result.Graph`, but by key rather than pointer
/// (see the `graph` module doc).
pub struct AnalysisResult<'m> {
    pub graph: Graph<'m>,
    pub state_count: usize,
    pub edge_count: usize,
    pub bounded: bool,
    pub max_tokens: HashMap<String, i64>,
    pub max_depth: i64,
    pub has_deadlock: bool,
    /// Keys (see [`marking::key`]) of states flagged as deadlocks.
    pub deadlocks: Vec<String>,
    pub has_cycle: bool,
    /// Firing sequences that form cycles.
    pub cycles: Vec<Vec<String>>,
    /// All transitions can eventually fire (only meaningful when
    /// `is_complete`).
    pub live: bool,
    /// Transitions proven never to fire (populated only when `is_complete`).
    pub dead_trans: Vec<String>,
    pub truncated: bool,
    pub truncate_msg: String,
    /// True if the full state space was explored (no truncation).
    pub is_complete: bool,
    /// Transitions that did not fire in the explored space but may fire in
    /// unexplored regions (only populated when `truncated`).
    pub potentially_dead: Vec<String>,
    /// Transitions proven unreachable via a targeted search.
    pub confirmed_dead: Vec<String>,
    pub fired_transitions: Vec<String>,
    pub exploration_stats: ExplorationStats,
}

/// Where a transition stands under L4 liveness. See [`Analyzer::l4_liveness`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L4Status {
    /// From every reachable marking, some firing sequence enables it.
    Live,
    /// Some reachable marking can never enable it again; the witness goes there.
    NotLive,
    /// The state space was truncated and no explored region rules it out.
    Unknown,
}

/// A replayable path to where a transition can never fire again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L4Witness {
    /// The shortest explored firing sequence from the initial marking to
    /// `marking`. The transition may well fire along the way.
    pub trace: Vec<String>,
    /// The first marking of the closed region the trace enters.
    pub marking: Marking,
    /// How many markings the region has. `1` with no enabled transition is a
    /// dead marking; more than `1` is a livelock the rest of the net is shut
    /// out of, which no deadlock check sees.
    pub region_size: usize,
}

/// The L4 result for one transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionL4 {
    pub transition: String,
    pub status: L4Status,
    /// Present exactly when `status` is [`L4Status::NotLive`].
    pub witness: Option<L4Witness>,
}

/// The result of [`Analyzer::l4_liveness`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L4Report {
    /// One entry per transition, in the model's declaration order.
    pub transitions: Vec<TransitionL4>,
    /// Terminal strongly connected components found. When truncated, only those
    /// known to be closed in the full state space count.
    pub terminal_components: usize,
    pub state_count: usize,
    pub truncated: bool,
}

impl L4Report {
    /// The transitions shown not to be L4-live, in declaration order.
    pub fn not_live(&self) -> impl Iterator<Item = &TransitionL4> {
        self.transitions.iter().filter(|t| t.status == L4Status::NotLive)
    }
}

/// Performs reachability analysis on a model.
pub struct Analyzer<'m> {
    model: &'m Model,
    initial: Marking,
    max_states: usize,
    max_tokens: i64,
}

impl<'m> Analyzer<'m> {
    /// The default state and token limits mirror go-pflow's (10,000 states,
    /// 1,000 tokens per place).
    pub fn new(model: &'m Model) -> Self {
        Analyzer {
            model,
            initial: model.initial_marking(),
            max_states: 10_000,
            max_tokens: 1_000,
        }
    }

    pub fn with_initial_marking(mut self, marking: Marking) -> Self {
        self.initial = marking;
        self
    }

    pub fn with_max_states(mut self, max: usize) -> Self {
        self.max_states = max;
        self
    }

    pub fn with_max_tokens(mut self, max: i64) -> Self {
        self.max_tokens = max;
        self
    }

    pub fn model(&self) -> &'m Model {
        self.model
    }

    pub fn initial(&self) -> &Marking {
        &self.initial
    }

    pub fn max_states(&self) -> usize {
        self.max_states
    }

    /// Constructs the reachability graph via breadth-first search, without
    /// cycle detection or liveness analysis. Go's `Analyzer.BuildGraph`.
    pub fn build_graph(&self) -> AnalysisResult<'m> {
        let mut graph = Graph::new(self.model, self.initial.clone());
        let mut bounded = true;
        let mut truncated = false;
        let mut truncate_msg = String::new();

        let mut queue: VecDeque<Marking> = VecDeque::new();
        queue.push_back(self.initial.clone());
        graph.add_state(&self.initial);

        let mut max_queue_size = 1usize;
        let mut total_enabled = 0usize;
        let mut states_with_enabled = 0usize;

        'bfs: while let Some(current) = queue.pop_front() {
            if graph.state_count() >= self.max_states {
                break;
            }
            let current_key = marking::key(&current);
            let enabled = match graph.get_state_by_key(&current_key) {
                Some(s) => s.enabled.clone(),
                None => continue,
            };

            if !enabled.is_empty() {
                total_enabled += enabled.len();
                states_with_enabled += 1;
            }

            for trans in &enabled {
                let Some(new_marking) = graph.fire(&current, trans) else {
                    continue;
                };

                if marking::max(&new_marking) > self.max_tokens {
                    bounded = false;
                    truncated = true;
                    truncate_msg = "unbounded: token count exceeded limit".to_string();
                    break 'bfs;
                }

                let new_key = marking::key(&new_marking);
                if graph.get_state_by_key(&new_key).is_none() {
                    graph.add_state(&new_marking);
                    queue.push_back(new_marking);
                    if queue.len() > max_queue_size {
                        max_queue_size = queue.len();
                    }
                }
                graph.add_edge(&current_key, &new_key, trans);
            }
        }

        if graph.state_count() >= self.max_states && !truncated {
            truncated = true;
            truncate_msg = "state limit reached".to_string();
        }

        let is_complete = !truncated;
        let state_count = graph.state_count();
        let edge_count = graph.edge_count();
        let max_depth = graph.max_depth();
        let max_tokens = graph.max_tokens();

        let mut stats = ExplorationStats {
            states_explored: state_count,
            states_limit: self.max_states,
            tokens_limit: self.max_tokens,
            queue_max_size: max_queue_size,
            ..Default::default()
        };
        if states_with_enabled > 0 {
            stats.branching_factor = total_enabled as f64 / states_with_enabled as f64;
        }
        if truncated && max_queue_size > 0 {
            let estimated_total =
                state_count as f64 * (1.0 + max_queue_size as f64 / state_count as f64);
            stats.exploration_ratio = state_count as f64 / estimated_total;
        } else {
            stats.exploration_ratio = 1.0;
        }

        // A deadlock is a terminal state reached with a non-empty marking,
        // given the run started with tokens (an empty net has nothing to
        // deadlock on).
        let initial_total = marking::total(&self.initial);
        let mut deadlocks = Vec::new();
        let mut has_deadlock = false;
        let terminal_keys: Vec<String> =
            graph.terminal_states().into_iter().map(|s| s.key.clone()).collect();
        for key in terminal_keys {
            let is_dl = {
                let state = &graph.states[&key];
                let mut dl = false;
                if initial_total > 0 && !marking::is_zero(&state.marking) {
                    dl = true;
                }
                if state.is_initial && state.enabled.is_empty() && initial_total > 0 {
                    dl = true;
                }
                dl
            };
            if is_dl {
                graph.states.get_mut(&key).unwrap().is_deadlock = true;
                has_deadlock = true;
                deadlocks.push(key);
            }
        }

        AnalysisResult {
            graph,
            state_count,
            edge_count,
            bounded,
            max_tokens,
            max_depth,
            has_deadlock,
            deadlocks,
            has_cycle: false,
            cycles: Vec::new(),
            live: false,
            dead_trans: Vec::new(),
            truncated,
            truncate_msg,
            is_complete,
            potentially_dead: Vec::new(),
            confirmed_dead: Vec::new(),
            fired_transitions: Vec::new(),
            exploration_stats: stats,
        }
    }

    /// Full analysis: [`Self::build_graph`] plus cycle detection and
    /// liveness. Go's `Analyzer.Analyze`.
    pub fn analyze(&self) -> AnalysisResult<'m> {
        let mut result = self.build_graph();
        let (has_cycle, cycles) = detect_cycles(&result.graph);
        result.has_cycle = has_cycle;
        result.cycles = cycles;
        self.analyze_liveness(&mut result);
        result
    }

    fn analyze_liveness(&self, result: &mut AnalysisResult<'m>) {
        let mut fired: HashSet<String> = HashSet::new();
        for edge in &result.graph.edges {
            fired.insert(edge.transition.clone());
        }
        result.fired_transitions = fired.iter().cloned().collect();

        let mut unfired: Vec<String> = self
            .model
            .transitions
            .iter()
            .map(|t| t.id.clone())
            .filter(|id| !fired.contains(id))
            .collect();
        unfired.sort();

        if result.is_complete {
            result.dead_trans = unfired.clone();
            result.confirmed_dead = unfired;
            result.live = result.dead_trans.is_empty();
        } else {
            result.potentially_dead = unfired;
            result.dead_trans.clear();
            result.live = false;
        }
    }

    /// L4 liveness per transition, from the terminal strongly connected
    /// components (SCCs) of the reachability graph.
    ///
    /// A transition is L4-live when, from every reachable marking, some firing
    /// sequence enables it. In a finite graph every marking reaches a terminal
    /// SCC and can never leave it, so that holds exactly when the transition
    /// fires inside every terminal SCC. A dead marking is a terminal SCC of one
    /// with no firings, so it makes every transition not L4-live, including
    /// when that marking is empty, which the deadlock heuristic accepts as an
    /// end state.
    ///
    /// Sound under truncation. A region only counts if every one of its states
    /// was fully expanded, because then it is closed in the full state space as
    /// well, and a refutation from it stands. Anything else comes back
    /// [`L4Status::Unknown`], never [`L4Status::Live`].
    ///
    /// go-pflow has no counterpart: its `live` is L1, as is [`Self::analyze`]'s.
    pub fn l4_liveness(&self) -> L4Report {
        let result = self.build_graph();
        let graph = &result.graph;
        let states = graph.states_list();
        let id = |key: &str| graph.states[key].id;
        let successors: Vec<Vec<usize>> = states
            .iter()
            .map(|s| s.successors.iter().map(|&e| id(&graph.edges[e].to)).collect())
            .collect();

        // Breadth-first over the explored edges: a shortest trace to each state.
        let root = states.first().map(|s| s.id).unwrap_or(0);
        let mut parent: Vec<Option<usize>> = vec![None; states.len()];
        let mut seen = vec![false; states.len()];
        let mut order = Vec::with_capacity(states.len());
        let mut queue = VecDeque::from([root]);
        seen[root] = true;
        while let Some(s) = queue.pop_front() {
            order.push(s);
            for &e in &states[s].successors {
                let to = id(&graph.edges[e].to);
                if !seen[to] {
                    seen[to] = true;
                    parent[to] = Some(e);
                    queue.push_back(to);
                }
            }
        }
        let trace_to = |mut s: usize| {
            let mut trace = Vec::new();
            while let Some(e) = parent[s] {
                trace.push(graph.edges[e].transition.clone());
                s = id(&graph.edges[e].from);
            }
            trace.reverse();
            trace
        };

        let component = strongly_connected_components(&successors);
        let count = component.iter().copied().max().map_or(0, |c| c + 1);
        let mut closed = vec![true; count];
        for (s, tos) in successors.iter().enumerate() {
            // Fewer edges than enabled transitions: not fully expanded, so
            // what lies beyond is unknown.
            if tos.iter().any(|&to| component[to] != component[s])
                || tos.len() < states[s].enabled.len()
            {
                closed[component[s]] = false;
            }
        }

        // Regions in the order the search first enters them, so each witness
        // is the nearest one, chosen the same way on every run.
        struct Region {
            entry: usize,
            size: usize,
            fires: HashSet<String>,
        }
        let mut regions: Vec<Region> = Vec::new();
        let mut region_of: HashMap<usize, usize> = HashMap::new();
        for &s in &order {
            let c = component[s];
            if !closed[c] {
                continue;
            }
            let r = *region_of.entry(c).or_insert_with(|| {
                regions.push(Region { entry: s, size: 0, fires: HashSet::new() });
                regions.len() - 1
            });
            regions[r].size += 1;
            regions[r].fires.extend(states[s].enabled.iter().cloned());
        }

        let transitions = self
            .model
            .transitions
            .iter()
            .map(|t| match regions.iter().find(|r| !r.fires.contains(&t.id)) {
                Some(r) => TransitionL4 {
                    transition: t.id.clone(),
                    status: L4Status::NotLive,
                    witness: Some(L4Witness {
                        trace: trace_to(r.entry),
                        marking: states[r.entry].marking.clone(),
                        region_size: r.size,
                    }),
                },
                None => TransitionL4 {
                    transition: t.id.clone(),
                    status: if result.truncated { L4Status::Unknown } else { L4Status::Live },
                    witness: None,
                },
            })
            .collect();

        L4Report {
            transitions,
            terminal_components: regions.len(),
            state_count: result.state_count,
            truncated: result.truncated,
        }
    }

    /// Whether `target` is reachable from the initial marking. Builds a
    /// fresh graph up to the configured limit. Go's `Analyzer.IsReachable`.
    pub fn is_reachable(&self, target: &Marking) -> bool {
        let result = self.build_graph();
        result.graph.get_state(target).is_some()
    }

    /// Whether the given transition sequence can fire in order from the
    /// initial marking, and the marking it reaches (partially, on failure).
    /// Go's `Analyzer.CanFire`.
    pub fn can_fire(&self, transitions: &[String]) -> (bool, Marking) {
        let mut current = self.initial.clone();
        for trans in transitions {
            if !self.model.enabled(trans, &current) {
                return (false, current);
            }
            current = self.model.fire(trans, &current);
        }
        (true, current)
    }

    /// Finds a firing sequence from the initial marking to `target`, or
    /// `None` if it is not reachable within the state limit. Go's
    /// `Analyzer.PathTo`.
    pub fn path_to(&self, target: &Marking) -> Option<Vec<String>> {
        let mut graph = Graph::new(self.model, self.initial.clone());
        let target_key = marking::key(target);

        let mut queue: VecDeque<(Marking, Vec<String>)> = VecDeque::new();
        queue.push_back((self.initial.clone(), Vec::new()));
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(marking::key(&self.initial));

        while let Some((cur_marking, path)) = queue.pop_front() {
            if visited.len() >= self.max_states {
                break;
            }
            if marking::key(&cur_marking) == target_key {
                return Some(path);
            }

            let key = graph.add_state(&cur_marking);
            let enabled = graph.states[&key].enabled.clone();
            for trans in enabled {
                let Some(next) = graph.fire(&cur_marking, &trans) else {
                    continue;
                };
                let next_key = marking::key(&next);
                if visited.insert(next_key) {
                    let mut new_path = path.clone();
                    new_path.push(trans);
                    queue.push_back((next, new_path));
                }
            }
        }
        None
    }

    /// A targeted search proving whether `transition` can ever fire,
    /// prioritizing states closer to enabling it. Go's
    /// `Analyzer.CanTransitionFire`.
    pub fn can_transition_fire(&self, transition: &str) -> (bool, Option<Vec<String>>) {
        if self.model.transition_by_id(transition).is_none() {
            return (false, None);
        }

        let mut graph = Graph::new(self.model, self.initial.clone());
        let input_reqs = self.transition_inputs(transition);

        struct Item {
            marking: Marking,
            path: Vec<String>,
            distance: i64,
        }

        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(marking::key(&self.initial));
        let mut queue = vec![Item {
            distance: distance_to_enable(&self.initial, &input_reqs),
            marking: self.initial.clone(),
            path: Vec::new(),
        }];

        let targeted_limit = self.max_states * 2;

        while !queue.is_empty() && visited.len() < targeted_limit {
            let mut min_idx = 0;
            for i in 1..queue.len() {
                if queue[i].distance < queue[min_idx].distance {
                    min_idx = i;
                }
            }
            let item = queue.remove(min_idx);

            let key = graph.add_state(&item.marking);
            let enabled = graph.states[&key].enabled.clone();
            if enabled.iter().any(|t| t == transition) {
                let mut path = item.path.clone();
                path.push(transition.to_string());
                return (true, Some(path));
            }

            for trans in enabled {
                let Some(next) = graph.fire(&item.marking, &trans) else {
                    continue;
                };
                if marking::max(&next) > self.max_tokens {
                    continue;
                }
                let next_key = marking::key(&next);
                if visited.insert(next_key) {
                    let mut new_path = item.path.clone();
                    new_path.push(trans);
                    let distance = distance_to_enable(&next, &input_reqs);
                    queue.push(Item { marking: next, path: new_path, distance });
                }
            }
        }

        (false, None)
    }

    fn transition_inputs(&self, transition: &str) -> HashMap<String, i64> {
        self.model
            .inputs(transition)
            .into_iter()
            .map(|a| (a.place, a.weight))
            .collect()
    }

    /// Runs [`Self::can_transition_fire`] on each candidate, splitting into
    /// confirmed-dead and confirmed-reachable. Go's
    /// `Analyzer.VerifyPotentiallyDead`.
    pub fn verify_potentially_dead(&self, potentially_dead: &[String]) -> (Vec<String>, Vec<String>) {
        let mut confirmed_dead = Vec::new();
        let mut confirmed_reachable = Vec::new();
        for trans in potentially_dead {
            let (can_fire, _) = self.can_transition_fire(trans);
            if can_fire {
                confirmed_reachable.push(trans.clone());
            } else {
                confirmed_dead.push(trans.clone());
            }
        }
        (confirmed_dead, confirmed_reachable)
    }

    /// [`Self::analyze`] followed by targeted verification of any
    /// potentially-dead transitions left by a truncated search. Go's
    /// `Analyzer.AnalyzeWithVerification`.
    pub fn analyze_with_verification(&self) -> AnalysisResult<'m> {
        let mut result = self.analyze();

        if result.truncated && !result.potentially_dead.is_empty() {
            let (confirmed, reachable) = self.verify_potentially_dead(&result.potentially_dead);
            result.confirmed_dead = confirmed.clone();
            result.dead_trans = confirmed;

            if !reachable.is_empty() {
                result.fired_transitions.extend(reachable.iter().cloned());
                let reachable_set: HashSet<&String> = reachable.iter().collect();
                result.potentially_dead.retain(|t| !reachable_set.contains(t));
            }

            result.live = result.confirmed_dead.is_empty() && result.potentially_dead.is_empty();
        }

        result
    }
}

fn distance_to_enable(marking: &Marking, inputs: &HashMap<String, i64>) -> i64 {
    let mut distance = 0;
    for (place, &required) in inputs {
        let have = marking.get(place).copied().unwrap_or(0);
        if have < required {
            distance += required - have;
        }
    }
    distance
}

/// Tarjan's algorithm, iteratively so a long chain of states cannot overflow
/// the stack. Maps each node to its component's number.
fn strongly_connected_components(successors: &[Vec<usize>]) -> Vec<usize> {
    const UNSEEN: usize = usize::MAX;
    let n = successors.len();
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut component = vec![0usize; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index = 0;
    let mut next_component = 0;

    for start in 0..n {
        if index[start] != UNSEEN {
            continue;
        }
        // (node, next successor to look at)
        let mut work = vec![(start, 0usize)];
        while let Some(&mut (v, ref mut next)) = work.last_mut() {
            if *next == 0 {
                index[v] = next_index;
                low[v] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            if let Some(&w) = successors[v].get(*next) {
                *next += 1;
                if index[w] == UNSEEN {
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(u, _)) = work.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component[w] = next_component;
                    if w == v {
                        break;
                    }
                }
                next_component += 1;
            }
        }
    }
    component
}

/// DFS cycle detection from the graph's root. Go's (private)
/// `Analyzer.detectCycles`.
fn detect_cycles(graph: &Graph) -> (bool, Vec<Vec<String>>) {
    let Some(root_key) = graph.root_key.clone() else {
        return (false, Vec::new());
    };

    let mut cycles = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut in_stack: HashSet<String> = HashSet::new();
    let mut path: Vec<String> = Vec::new();
    let mut state_path: Vec<String> = Vec::new();

    dfs_cycles(graph, &root_key, &mut visited, &mut in_stack, &mut path, &mut state_path, &mut cycles);

    (!cycles.is_empty(), cycles)
}

#[allow(clippy::too_many_arguments)]
fn dfs_cycles(
    graph: &Graph,
    key: &str,
    visited: &mut HashSet<String>,
    in_stack: &mut HashSet<String>,
    path: &mut Vec<String>,
    state_path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    visited.insert(key.to_string());
    in_stack.insert(key.to_string());
    state_path.push(key.to_string());

    let state = &graph.states[key];
    let successors: Vec<(String, String)> = state
        .successors
        .iter()
        .map(|&i| (graph.edges[i].to.clone(), graph.edges[i].transition.clone()))
        .collect();

    for (next_key, transition) in successors {
        path.push(transition);

        if !visited.contains(&next_key) {
            dfs_cycles(graph, &next_key, visited, in_stack, path, state_path, cycles);
        } else if in_stack.contains(&next_key) {
            if let Some(cycle_start) = state_path.iter().position(|h| h == &next_key) {
                cycles.push(path[cycle_start..].to_vec());
            }
        }

        path.pop();
    }

    in_stack.remove(key);
    state_path.pop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pflow_metamodel::schema::{Arc, Place, Transition};

    fn cycle_model() -> Model {
        Model {
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
        }
    }

    /// A linear chain that terminates: p0 -> t0 -> p1 (no way back).
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
    fn cycle_model_is_live_and_has_cycle_no_deadlock() {
        let model = cycle_model();
        let analyzer = Analyzer::new(&model);
        let result = analyzer.analyze();
        assert_eq!(result.state_count, 2);
        assert!(result.has_cycle);
        assert!(!result.has_deadlock);
        assert!(result.is_complete);
        assert!(result.live);
        assert!(result.bounded);
    }

    #[test]
    fn chain_model_terminates_and_deadlocks() {
        let model = chain_model();
        let analyzer = Analyzer::new(&model);
        let result = analyzer.analyze();
        assert_eq!(result.state_count, 2);
        assert!(!result.has_cycle);
        assert!(result.has_deadlock, "terminal non-empty marking is a deadlock");
        assert!(result.live);
    }

    #[test]
    fn path_to_finds_a_firing_sequence() {
        let model = chain_model();
        let analyzer = Analyzer::new(&model);
        let mut target = Marking::new();
        target.insert("p0".to_string(), 0);
        target.insert("p1".to_string(), 1);
        let path = analyzer.path_to(&target).expect("reachable");
        assert_eq!(path, vec!["t0".to_string()]);
    }

    #[test]
    fn can_fire_replays_a_sequence() {
        let model = chain_model();
        let analyzer = Analyzer::new(&model);
        let (ok, marking) = analyzer.can_fire(&["t0".to_string()]);
        assert!(ok);
        assert_eq!(marking.get("p1"), Some(&1));

        let (ok2, _) = analyzer.can_fire(&["t0".to_string(), "t0".to_string()]);
        assert!(!ok2, "t0 cannot fire twice: p0 is empty the second time");
    }

    #[test]
    fn unbounded_place_is_reported_and_truncated() {
        // p0 has one token and a self-loop that also produces into p1
        // unconditionally: p1 grows without bound.
        let model = Model {
            name: "unbounded".into(),
            places: vec![
                Place { id: "p0".into(), initial: 1, ..Default::default() },
                Place { id: "p1".into(), ..Default::default() },
            ],
            transitions: vec![Transition { id: "t0".into(), ..Default::default() }],
            arcs: vec![
                Arc { from: "p0".into(), to: "t0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p0".into(), ..Default::default() },
                Arc { from: "t0".into(), to: "p1".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let analyzer = Analyzer::new(&model).with_max_tokens(5);
        let result = analyzer.build_graph();
        assert!(!result.bounded);
        assert!(result.truncated);
    }
}
