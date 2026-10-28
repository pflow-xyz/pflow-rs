//! The counts engine's transposition key is kept by XORing in the places
//! each firing changes (Zobrist hashing). The gate: it equals a full rehash
//! of the marking at every node of random walks on the three fixture nets,
//! and a rehash tells different markings apart.
mod common;

use std::collections::HashMap;

use common::Lcg;

#[test]
fn the_incremental_key_equals_a_full_rehash_at_every_node() {
    for name in common::NETS {
        let (g, m0) = common::game(name);
        let mut r = Lcg(0xC0FFEE);
        let (mut nodes, mut seen) = (0, HashMap::new());
        for _ in 0..300 {
            // A random walk to the end of the game, checked at each node.
            let (mut mk, mut line) = (m0.clone(), vec![]);
            loop {
                let moves = g.moves(&mk);
                if moves.is_empty() {
                    break;
                }
                let t = moves[r.below(moves.len())];
                line.push(t);
                mk = g.play(&mk, t).unwrap();
            }
            let keys = g.counts_keys(&m0, &line).unwrap();
            assert_eq!(keys.len(), line.len() + 1, "{name}");
            let mut mk = m0.clone();
            for (i, (inc, full)) in keys.iter().enumerate() {
                assert_eq!(inc, full, "{name} at ply {i} of {line:?}");
                if i > 0 {
                    mk = g.play(&mk, line[i - 1]).unwrap();
                }
                // Equal keys mean equal markings: no two seen so far collide.
                let was = seen.insert(*full, mk.clone());
                assert!(
                    was.is_none_or(|w| w == mk),
                    "{name}: a collision at ply {i}"
                );
                nodes += 1;
            }
        }
        assert!(nodes > 1000, "{name}: {nodes} nodes");
    }
}

#[test]
fn every_move_from_every_node_of_tic_tac_toe_keeps_the_key() {
    // Not a sample: all 5,478 positions, every move out of each.
    let (g, m0) = common::game("tic-tac-toe");
    let mut checked = 0;
    for mk in common::reachable(&g, &m0) {
        for t in g.moves(&mk) {
            let keys = g.counts_keys(&mk, &[t]).unwrap();
            assert!(keys.iter().all(|(a, b)| a == b), "{mk:?} then {t}");
            checked += 1;
        }
    }
    assert!(checked > 10_000, "{checked}");
}

#[test]
fn a_firing_changes_the_key() {
    let (g, m0) = common::prizes(&[1, 2, 3]);
    let keys = g.counts_keys(&m0, &[0]).unwrap();
    assert_ne!(keys[0].0, keys[1].0);
}
