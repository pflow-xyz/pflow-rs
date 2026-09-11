//! `petri_lumping`: ordinary lumpability of the CTMC the stochastic engine
//! samples — is there a coarser partition of the (enumerated) reachable
//! state space such that the block-aggregated process is itself a valid
//! CTMC producing identical answers to any question phrased over blocks?
//! (Buchholz, P. (1994). "Exact and Ordinary Lumpability in Finite Markov
//! Chains." *Zeitschrift für Operations Research* 34, 59-75, Definition 2:
//! a partition is ordinarily lumpable iff, for every pair of blocks, the
//! total rate from any state in one block into the other is the same for
//! every member.)
//!
//! **Not a go-pflow package** (ROADMAP.md's Phase 6 note: no `lumping`
//! algorithm exists on the go-pflow side, only a mention in a comment).
//! This is relational coarsest-partition refinement by naive fixed-point
//! iteration ("repeatedly split every block by its rate-vector into the
//! current blocks, until nothing splits further" — Kanellakis, P.C. and
//! Smolka, S.A. (1990). "CCS expressions, finite state processes, and three
//! problems of equivalence." *Information and Computation* 86(1), 43-68),
//! not the Paige-Tarjan splitter-queue algorithm a from-scratch, from-first-
//! principles implementation of the harder-to-get-right performance variant
//! would need — same fixed point, worse asymptotics on a large state space.
//! Bounded by `max_states`, the same exhaustive-exploration cap
//! `petri_verify`'s exhaustive checks use.

use super::convert::parse_any_model;
use pflow_reachability::Analyzer;
use serde::Serialize;
use std::collections::HashMap;

const DEFAULT_MAX_STATES: usize = 5_000;

#[derive(Serialize)]
struct LumpingResult {
    state_count: usize,
    block_count: usize,
    lumpable: bool,
    truncated: bool,
    blocks: Vec<Vec<String>>,
}

pub fn run(model: &str, max_states: Option<usize>) -> Result<String, String> {
    let model = parse_any_model(model)?;
    let rates = pflow_solver::stochastic::rates(&model);

    let analyzer = Analyzer::new(&model).with_max_states(max_states.unwrap_or(DEFAULT_MAX_STATES));
    let result = analyzer.build_graph();

    let states: Vec<String> = result.graph.states_list().iter().map(|s| s.key.clone()).collect();
    let idx: HashMap<&str, usize> = states.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();

    // out_rates[i][j] = total rate of transitions taking state i to state j.
    let mut out_rates: Vec<HashMap<usize, f64>> = vec![HashMap::new(); states.len()];
    for edge in &result.graph.edges {
        let (Some(&from), Some(&to)) = (idx.get(edge.from.as_str()), idx.get(edge.to.as_str())) else {
            continue;
        };
        let r = rates.get(&edge.transition).copied().unwrap_or(1.0);
        *out_rates[from].entry(to).or_insert(0.0) += r;
    }

    let partition = refine(&states, &out_rates);

    let mut blocks_map: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, &block) in partition.iter().enumerate() {
        blocks_map.entry(block).or_default().push(states[i].clone());
    }
    let mut blocks: Vec<Vec<String>> = blocks_map.into_values().collect();
    for b in &mut blocks {
        b.sort();
    }
    blocks.sort_by(|a, b| a[0].cmp(&b[0]));

    let out = LumpingResult {
        state_count: states.len(),
        block_count: blocks.len(),
        lumpable: blocks.len() < states.len(),
        truncated: result.truncated,
        blocks,
    };

    serde_json::to_string_pretty(&out).map_err(|e| format!("Serialization error: {e}"))
}

/// Coarsest ordinarily-lumpable partition, by repeated splitting until a
/// full pass changes nothing.
fn refine(states: &[String], out_rates: &[HashMap<usize, f64>]) -> Vec<usize> {
    let n = states.len();
    if n == 0 {
        return Vec::new();
    }
    let mut partition = vec![0usize; n];

    loop {
        // Signature: (current block, rate into each current block), in a
        // canonical bit-for-bit comparable form.
        let mut signatures: Vec<Vec<(usize, u64)>> = Vec::with_capacity(n);
        for rates_i in out_rates.iter().take(n) {
            let mut sig: Vec<(usize, u64)> = rates_i
                .iter()
                .map(|(&to, &r)| (partition[to], r.to_bits()))
                .collect();
            sig.sort();
            sig.dedup_by(|a, b| {
                if a.0 == b.0 {
                    // Two edges into the same target block: rates add.
                    let merged = f64::from_bits(a.1) + f64::from_bits(b.1);
                    b.1 = merged.to_bits();
                    true
                } else {
                    false
                }
            });
            signatures.push(sig);
        }

        let mut new_ids: HashMap<(usize, Vec<(usize, u64)>), usize> = HashMap::new();
        let mut new_partition = vec![0usize; n];
        for i in 0..n {
            let key = (partition[i], signatures[i].clone());
            let next_id = new_ids.len();
            let id = *new_ids.entry(key).or_insert(next_id);
            new_partition[i] = id;
        }

        if new_partition == partition {
            return partition;
        }
        partition = new_partition;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_fork_lumps() {
        // start -> {a, b} (a race with equal rate), a -> done, b -> done.
        // The two mid-states are structurally interchangeable, so they
        // should end up in one block.
        let dsl = r#"(schema race
            (states
                (state start :kind token :initial 1)
                (state mid_a :kind token :initial 0)
                (state mid_b :kind token :initial 0)
                (state done :kind token :initial 0)
            )
            (actions (action go_a) (action go_b) (action finish_a) (action finish_b))
            (arcs
                (arc start -> go_a) (arc go_a -> mid_a)
                (arc start -> go_b) (arc go_b -> mid_b)
                (arc mid_a -> finish_a) (arc finish_a -> done)
                (arc mid_b -> finish_b) (arc finish_b -> done)
            )
        )"#;
        let result = run(dsl, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["state_count"].as_u64().unwrap() > v["block_count"].as_u64().unwrap());
        assert_eq!(v["lumpable"], true);
    }

    #[test]
    fn chain_is_already_minimal() {
        // Every state in a plain chain is its own block: no two states have
        // the same outgoing-rate signature into the same set of targets.
        let dsl = r#"(schema chain
            (states
                (state a :kind token :initial 1)
                (state b :kind token :initial 0)
                (state c :kind token :initial 0)
            )
            (actions (action ab) (action bc))
            (arcs (arc a -> ab) (arc ab -> b) (arc b -> bc) (arc bc -> c))
        )"#;
        let result = run(dsl, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["state_count"], v["block_count"]);
        assert_eq!(v["lumpable"], false);
    }
}
