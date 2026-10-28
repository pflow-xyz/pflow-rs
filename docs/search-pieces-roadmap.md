# Composable search pieces: roadmap

Status, Oct 27 2026: slice 1 is merged in pflow-rs main as PR #7 (merge c396d46). Slices 2 to 6 run on the Mac (workbit), one branch and PR per slice. Matt says yes before every push. Generic pflow-rs library work only: a piece reads what the net declares (sides, reward places, auto transitions, arcs) and nothing else.

Design doc: https://claude.ai/artifact/DJGTswKTg29cpKuUVPyvHW

## Done: slice 1
- Move order is a chain of pieces: tt_best, immediate_win, history, centrality, threats (order.rs). Chain keeps a DECISIVE tier for tt_best and immediate_win.
- SearchPlan JSON with a registry and a legality check (plan.rs). deficit() on BitModel and IndexedModel.
- Golden gate: tic-tac-toe, Connect-3, 50-position Connect-4 sample (tests/search_order.rs, tests/golden/search-gate.txt).
- Connect-4 nodes: baseline 1,471,928. Centrality -7.6%, threats +1.7%, both -8.9%. Use node counts; wall time is noisy.

## Slice 2: exactness and memory
- Cap the transposition table inside a search by default. Today Solver::with_memo is opt-in and the table is only cleared at the start of Solver::value; hard positions reached a 3.3 GB allocation.
- Compact TT entries keyed by the BitModel bit encoding (fa7b407): the packed marking plus small lo/hi/best, with no heap allocation per entry.
- Bounded table with a replacement policy (fixed-size buckets, depth- or age-preferred). Report bytes per entry and peak memory before and after.
- Sound filters must_win and non_losing, read from declared goals (non_losing: drop moves after which the opponent has an immediately enabled winning claim; if every move loses, return the value at once). Wire into SearchPlan filters, allowed in exact mode.
- Keep threats only if a wider sample (~200 positions) shows a gain in some chain placement; otherwise make it non-default and say why.
- Values must stay identical on the golden gate; node counts may change; update the golden file deliberately and say so.

Slice 2 results (branch search-pieces-slice2):
- memo.rs: a bounded table, 2^23 entries by default (Solver::MEMO). An entry over bits is the packed marking (3 words on Connect-4's 131 places) plus one meta word (best move, bounds as one-byte indices, log2 work, generation): 32 bytes, and a 1-byte hash tag beside it. Two-choice buckets of four; a new key takes an empty slot, else the oldest generation, else the least work. It starts at 1024 slots and doubles when both of a key's buckets are full and a quarter of the slots are taken.
- Peak memory, Connect-4 ply 10, three positions under a 30M-node budget (search_memory example): 2,774 MB before, 555 MB after. The first position took 29.26M nodes in 13.1 s before; now 22.37M nodes in 11.7 s (must_win, below, is most of that). Without filters, its nodes against table size: 2^21 entries +46%, 2^23 +3.6%, unbounded 0.
- Cost: about 7% wall time on the 50-position bench at equal nodes (0.605 s to 0.65 s), about 12% per node on the 30M-node search. A HashMap of the same keys was slower still; the old HashMap of 128-bit hashes is what it gives up.
- filter.rs: must_win (a move ending the game at the mover's best bound settles the node, recording the cutoff it replaces) and non_losing (drop a move after which the other side's reply ends the game at the mover's worst bound; replies to check come from the declared goals' deficits). Both read sides, reward bounds and goals only. must_win is now a default filter: gate values identical, nodes -27% tic-tac-toe, -29% Connect-3, -24% Connect-4 (1,471,928 to 1,119,152), and the bench is faster than before slice 2 (0.589 s to 0.528 s). non_losing is off by default: -3% nodes on top of must_win (-36% with centrality), but 2.9x the time, since it plays replies at every node; it is for root moves (slice 3).
- Threats, on a 200-position Connect-4 sample (150 at plies 20-26, 50 at 16-18), with must_win: baseline 4,875,155 nodes; threats after immediate_win 2,123,028 (-56%); threats last 3,157,770; centrality 1,854,885 (-62%); centrality then threats 1,954,852; threats then centrality 1,844,175 (-0.6% against centrality alone, 10% slower). Kept: it gains a lot on the default chain, nothing worth its time beside centrality, and it stays out of the default chain as before.
- Golden gate regenerated for must_win: every value the same, node counts as above.

## Slice 3: ODE as a first pass
- ode root orderer via pflow-solver, with the continuous net compiled once per game (today ~6 ms per tsit5 solve on Connect-4, about 52 steps, mostly recompiling the net; a 7-move root decision ~54 ms). Measure before and after hoisting.
- root.order in SearchPlan, with non_losing after the ODE as the refiner (the tic-tac-toe heat map missed must-blocks; prior art: petri-pilot zk-ode/evaluate.go and experiments/ode-minimax, pflow-native OdePlayer). Exact mode: reorder only.
- Aspiration driver centred on the ODE estimate, falling back to the full window. Values identical.

Slice 3 results (branch search-pieces-slice3):
- root.rs: pieces that run once at the position searched. An Estimate (a trait; this crate has none) breaks the order chain's ties among the root's moves; non_losing at the root drops moves that leave a win at once; the aspiration driver starts the run of yes/no searches at the gap nearest the best guess (a window an eighth of the range wide, with the full range as fallback, when the rewards are too many to step through). SearchPlan: root: {"order": ["ode", "non_losing"]} and the "aspiration" driver (refused without an estimate in root.order); SearchPlan::solver_with takes the estimate, and pflow_solver::game::solver supplies ode. Values held to minimax on tic-tac-toe and Nim with estimates wrong on purpose, and to the default search on tic-tac-toe and Connect-3 with the real ODE.
- pflow_solver::game::OdeEstimate: the mass-action ODE of the game's net (every transition at rate 1, read arcs gate, inhibitors left out), horizon 3, tsit5, pflow-native's options (dtmax = horizon/50, 52 steps). Compiled once per game it gives the same state as a pflow-core net built per solve (1e-9, same steps). A solve on Connect-4: 0.99 ms built per solve, 0.41 ms compiled once (2.4x); ranking a 7-move root: 8.4 ms to 2.5 ms. The ~6 ms above was measured in pflow-native, whose solve also converts its document to JSON and then to a net; that part is not measured here.
- Tic-tac-toe, 4,520 positions with a move: the first move is a best one in the net's order 58.7%, by the ODE 96.1%, by the ODE then non_losing 96.8%. The ODE alone leaves a win to block in some positions; with non_losing after it, in none (test).
- Connect-4 nodes (default chain and filters, a fresh solver per position; times include the ODE):

  | plan | 50-position sample | 200-position wide sample |
  | --- | --- | --- |
  | default | 1,119,152, 0.53 s | 4,875,155, 2.37 s |
  | root non_losing | -0.0% | -0.0% |
  | root ode | +21.8%, 0.75 s | -41.3%, 1.82 s |
  | root ode, non_losing | +21.8%, 0.69 s | -41.3%, 1.61 s |
  | ode, non_losing + aspiration | +44.1%, 0.81 s | -41.5%, 1.60 s |
  | centrality | -6.1%, 0.47 s | -62.0%, 0.83 s |
  | centrality + ode, non_losing | -6.6%, 0.52 s | -62.9%, 1.07 s |
  | centrality + ode, non_losing + aspiration | +23.3%, 0.67 s | -65.7%, 1.00 s |

  The 50-position sample is dominated by its ten hard positions (plies 16-17), where the net's own order happens to be cheap to prove: a perfect root order (exact values as the estimate) took 1,119,137 nodes there, no better than the default, so the root is not a lever on that sample and the ODE's different order costs. On the wide sample it is: -41%.
- Aspiration: the ODE's guess is an expected reward near 0 (about ±0.1 on Connect-4), not a calibrated value, so on the hard sample it points at the expensive "is it a win?" search first. With an exact guess the driver saves 4.1% (1,073,550 nodes on the 50-position sample). Off by default; worth it with a calibrated estimate (slice 4's cache seeds exact values).
- Placement: the ODE leading the chain instead of breaking its ties was within a few percent overall (with centrality: -70.2% on the wide sample, but +25% on the 50); ties it is, so the guess never overrides the chain.
- Nothing is on by default, so the golden gate is unchanged.

## Slice 4: reuse and play
- Seed the memo from the label cache as a generic opening book (exact entries only, keyed like the TT).
- mode: play, which returns a best move under a node or time budget. Unsound pieces are allowed only here.

## Slice 5: outside pflow-rs (ask Matt first)
- game.move / game.value from a SearchPlan as pflow-native commands, then as pflow.xyz registry and MCP tools, so the Assistant and goals.json can call a plan as one deterministic tool.

## Slice 6: diagrams of the search constructs
- Draw category-theoretic diagrams (string diagrams) of how the pieces compose: orderers as a monoid under chaining, filters under intersection, bounds as an interval meet, drivers as endomorphisms wrapping the core, and a SearchPlan as one composite.
- Frame them with the ontology from ontology.pflow.xyz, so each piece kind maps to a named concept there.
- Reuse pflow-native's string-diagram export (core/src/string_diagram.rs) where it fits, so a SearchPlan can render its own diagram. Check each diagram against the code's composition laws (a test per law where possible).

## Sketch: more pieces to mix in (not committed)

| Piece | Kind | Needs from the net | Exact? |
| --- | --- | --- | --- |
| Killer moves (per ply) | Orderer | Transition indices, ply count | Yes, ordering only |
| Countermove / butterfly history | Orderer | Transition indices | Yes, ordering only |
| Principal variation search | Driver | Nothing extra | Yes |
| Aspiration windows on any estimate | Driver | An estimate piece (ODE, cache, learned) | Yes, full-window fallback |
| Iterative deepening | Driver | Exact: a declared move-count place for score bounds; depth-limited: a heuristic | Exact as a window driver; play mode when depth-limited |
| Score bounds from remaining moves (Pons) | Bound | Reward that depends on a declared move counter | Yes |
| Symmetry reduction | Memo key | Net automorphisms that keep arcs, sides and rewards | Yes, canonical TT keys |
| Dead-goal pruning | Bound / Filter | Siphons and P-invariants: a goal whose input siphon is empty can never fire | Yes, structural |
| Invariant bounds | Bound | P-invariants cap how much reward is still reachable | Yes |
| Enhanced transposition cutoffs | Driver | Memo lookups on children | Yes |
| Stubborn sets / partial-order reduction | Engine | Independence of auto transitions while settling | Yes, for settle and reachability |
| Proof-number search | Driver | Win/loss/draw sign only | Yes, for the sign |
| Endgame tables by backward reachability | Memo | Small reachable tail of the net | Yes |
| Parallel search (shared bounded TT) | Driver | Nothing extra | Yes, values; speed varies |
| Learned orderer (pflow-native rnn policy) | Orderer | Trained weights per net | Yes as ordering; play mode as a filter |
| MCTS with ODE rollouts | Driver | ODE estimate | No, play mode only |

The structural ones (symmetry, siphons, invariants) are the most Petri-net-native: each is a classic net analysis that becomes a sound search piece.
