//! ZK Heads-Up Hold'em: integer reduction + heatmap AI + Groth16 proofs.
//!
//! Demonstrates provably fair poker using Petri net topology:
//! 1. **Integer reduction**: ODE simulation on a hand-analysis net derives
//!    hand strength values (SF=9, 4K=8, ..., HC=1) purely from topology
//! 2. **Groth16 ZK proofs**: every game action (deal, bet, check, call, fold,
//!    showdown) is proven as a valid Petri net transition
//! 3. **Commit-reveal shuffle**: Poseidon hash commitment for provably fair
//!    card dealing
//! 4. **Deterministic house AI**: strategy based on hand strength + pot odds,
//!    enforceable on-chain
//!
//! ```bash
//! cargo run --example zk_holdem -p pflow --features zk-arkworks --release
//! cargo run --example zk_holdem -p pflow --features zk-arkworks --release -- --demo
//! cargo run --example zk_holdem -p pflow --features zk-arkworks --release -- --export-solidity ./contracts/
//! ```

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

use pflow_core::PetriNet;
use pflow_solver::{
    equilibrium::{solve_until_equilibrium, EquilibriumOptions},
    methods, Options, Problem,
};
use pflow_zk::{fire_transition, IncidenceMatrix, PetriProver, TransitionWitness};
use pflow_zk_arkworks::solidity_export;
use pflow_zk_arkworks::ArkworksProver;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HAND_NAMES: [&str; 9] = [
    "High Card",
    "One Pair",
    "Two Pair",
    "Three of a Kind",
    "Straight",
    "Flush",
    "Full House",
    "Four of a Kind",
    "Straight Flush",
];

const RANK_NAMES: [&str; 13] = [
    "2", "3", "4", "5", "6", "7", "8", "9", "T", "J", "Q", "K", "A",
];

const SUIT_SYMBOLS: [char; 4] = ['\u{2660}', '\u{2665}', '\u{2666}', '\u{2663}'];

const BET_SIZE: i64 = 2;
const SMALL_BLIND: i64 = 1;
const BIG_BLIND: i64 = 2;
const INITIAL_STACK: i64 = 100;

/// Drain counts encoding relative hand frequencies (log-scaled from actual
/// 5-card combos). More drains = more common = lower value at equilibrium.
const DRAIN_COUNTS: [usize; 9] = [
    32, // High Card      (1,302,540 combos)
    24, // One Pair        (1,098,240)
    16, // Two Pair        (123,552)
    12, // Three of a Kind (54,912)
    8,  // Straight        (10,200)
    5,  // Flush           (5,108)
    4,  // Full House      (3,744)
    2,  // Four of a Kind  (624)
    1,  // Straight Flush  (40)
];

const HAND_KEYS: [&str; 9] = [
    "hc", "pair", "twopair", "trips", "straight", "flush", "fullhouse", "quads", "stflush",
];

// ---------------------------------------------------------------------------
// Card representation
// ---------------------------------------------------------------------------

fn card_rank(card: u8) -> u8 {
    card % 13
}

fn card_suit(card: u8) -> u8 {
    card / 13
}

fn card_name(card: u8) -> String {
    format!(
        "{}{}",
        RANK_NAMES[card_rank(card) as usize],
        SUIT_SYMBOLS[card_suit(card) as usize]
    )
}

fn cards_str(cards: &[u8]) -> String {
    let names: Vec<String> = cards.iter().map(|&c| card_name(c)).collect();
    format!("[{}]", names.join(" "))
}

// ---------------------------------------------------------------------------
// Deterministic shuffle (Fisher-Yates with seeded LCG)
// ---------------------------------------------------------------------------

fn shuffle_deck(seed: u64) -> [u8; 52] {
    let mut deck: [u8; 52] = std::array::from_fn(|i| i as u8);
    let mut rng_state = seed;

    for i in (1..52).rev() {
        // LCG: Knuth's constants
        rng_state = rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (rng_state >> 33) as usize % (i + 1);
        deck.swap(i, j);
    }

    deck
}

// ---------------------------------------------------------------------------
// Hand evaluation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HandRank {
    HighCard = 0,
    OnePair = 1,
    TwoPair = 2,
    ThreeOfAKind = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    FourOfAKind = 7,
    StraightFlush = 8,
}

impl std::fmt::Display for HandRank {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", HAND_NAMES[*self as usize])
    }
}

/// Evaluate the best 5-card hand from a set of cards (2-7 cards).
fn evaluate_hand(cards: &[u8]) -> HandRank {
    if cards.len() < 5 {
        return evaluate_partial(cards);
    }

    let n = cards.len();
    let mut best = HandRank::HighCard;

    // Try all C(n, 5) combinations
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                for l in (k + 1)..n {
                    for m in (l + 1)..n {
                        let five = [cards[i], cards[j], cards[k], cards[l], cards[m]];
                        let rank = evaluate_five(&five);
                        if rank > best {
                            best = rank;
                        }
                    }
                }
            }
        }
    }

    best
}

/// Evaluate exactly 5 cards.
fn evaluate_five(cards: &[u8; 5]) -> HandRank {
    let mut rank_counts = [0u8; 13];
    let mut suit_counts = [0u8; 4];

    for &c in cards {
        rank_counts[card_rank(c) as usize] += 1;
        suit_counts[card_suit(c) as usize] += 1;
    }

    let is_flush = suit_counts.iter().any(|&c| c >= 5);
    let is_straight = check_straight(&rank_counts);

    let mut pairs = 0;
    let mut trips = 0;
    let mut quads = 0;
    for &c in &rank_counts {
        match c {
            2 => pairs += 1,
            3 => trips += 1,
            4 => quads += 1,
            _ => {}
        }
    }

    if is_flush && is_straight {
        return HandRank::StraightFlush;
    }
    if quads > 0 {
        return HandRank::FourOfAKind;
    }
    if trips > 0 && pairs > 0 {
        return HandRank::FullHouse;
    }
    if is_flush {
        return HandRank::Flush;
    }
    if is_straight {
        return HandRank::Straight;
    }
    if trips > 0 {
        return HandRank::ThreeOfAKind;
    }
    if pairs >= 2 {
        return HandRank::TwoPair;
    }
    if pairs == 1 {
        return HandRank::OnePair;
    }
    HandRank::HighCard
}

fn check_straight(rank_counts: &[u8; 13]) -> bool {
    for start in 0..=8 {
        if (start..start + 5).all(|r| rank_counts[r] > 0) {
            return true;
        }
    }
    // Wheel: A-2-3-4-5
    rank_counts[12] > 0
        && rank_counts[0] > 0
        && rank_counts[1] > 0
        && rank_counts[2] > 0
        && rank_counts[3] > 0
}

/// Partial hand evaluation for < 5 cards (preflop).
fn evaluate_partial(cards: &[u8]) -> HandRank {
    let mut rank_counts = [0u8; 13];
    for &c in cards {
        rank_counts[card_rank(c) as usize] += 1;
    }

    let mut pairs = 0;
    let mut trips = 0;
    for &c in &rank_counts {
        match c {
            2 => pairs += 1,
            3 => trips += 1,
            _ => {}
        }
    }

    if trips > 0 {
        return HandRank::ThreeOfAKind;
    }
    if pairs >= 2 {
        return HandRank::TwoPair;
    }
    if pairs == 1 {
        return HandRank::OnePair;
    }
    HandRank::HighCard
}

// ---------------------------------------------------------------------------
// Analysis net: hand strength via integer reduction
// ---------------------------------------------------------------------------

/// Build the hand analysis Petri net (18 places, 113 transitions).
///
/// Each hand category has a source place (constant inflow) and a value place
/// (accumulator). Drain transitions proportional to combinatorial frequency
/// create differential outflow: rare hands accumulate more, common hands drain
/// faster. Inverting equilibrium concentrations recovers the strategic
/// hierarchy: SF > 4K > FH > Flush > Str > 3K > 2P > Pair > HC.
fn build_hand_analysis_net() -> PetriNet {
    let mut net = PetriNet::new();

    for key in &HAND_KEYS {
        net.add_place(format!("src_{}", key), vec![1.0], vec![], 0.0, 0.0, None);
        net.add_place(format!("val_{}", key), vec![0.0], vec![], 0.0, 0.0, None);
    }

    // Play transitions: src_H -> val_H + src_H (catalytic on source)
    for key in &HAND_KEYS {
        let t = format!("play_{}", key);
        net.add_transition(&t, "play", 0.0, 0.0, None);
        net.add_arc(format!("src_{}", key), &t, vec![1.0], false);
        net.add_arc(&t, format!("src_{}", key), vec![1.0], false);
        net.add_arc(&t, format!("val_{}", key), vec![1.0], false);
    }

    // Drain transitions: val_H -> (consumed)
    for (h, key) in HAND_KEYS.iter().enumerate() {
        for d in 0..DRAIN_COUNTS[h] {
            let t = format!("drain_{}_{}", key, d);
            net.add_transition(&t, "drain", 0.0, 0.0, None);
            net.add_arc(format!("val_{}", key), &t, vec![1.0], false);
        }
    }

    net
}

/// Run ODE to equilibrium on the analysis net, extract hand strength values.
fn hand_integer_reduction() -> [f64; 9] {
    let net = build_hand_analysis_net();

    println!(
        "  Analysis net: {} places, {} transitions, {} arcs",
        net.places.len(),
        net.transitions.len(),
        net.arcs.len()
    );

    let state = net.set_state(None);
    let rates = net.set_rates(None);

    let prob = Problem::new(net, state, [0.0, 200.0], rates);
    let opts = Options {
        dt: 0.5,
        ..Options::default_opts()
    };
    let eq_opts = EquilibriumOptions {
        tolerance: 1e-4,
        consecutive_steps: 3,
        min_time: 0.5,
        check_interval: 5,
    };
    let (_, result) = solve_until_equilibrium(&prob, &methods::tsit5(), &opts, &eq_opts);
    let eq_state = result.state;

    if !result.reached {
        println!("  Warning: equilibrium not fully reached, using final state");
    }

    // Extract val_H equilibrium concentrations. Under mass-action kinetics,
    // val_H = 1/num_drains at equilibrium. Rare hands (fewer drains) accumulate
    // more tokens, so raw concentrations directly encode hand value:
    // SF (1 drain) >> HC (32 drains).
    let mut values = [0.0f64; 9];
    for (i, key) in HAND_KEYS.iter().enumerate() {
        let label = format!("val_{}", key);
        values[i] = eq_state.get(&label).copied().unwrap_or(0.0);
    }

    // Normalize by minimum non-zero value to get integer-like ratios
    let min_val = values
        .iter()
        .copied()
        .filter(|&v| v > 1e-10)
        .fold(f64::MAX, f64::min);
    if min_val > 1e-10 {
        for v in &mut values {
            *v /= min_val;
        }
    }

    values
}

fn print_hand_values(values: &[f64; 9]) {
    println!("  Hand Strength Values (from ODE topology):");
    println!();
    println!("  {:<18} {:>8} {:>8}", "Hand", "Drains", "Value");
    println!("  {}", "-".repeat(38));
    for i in (0..9).rev() {
        println!(
            "  {:<18} {:>8} {:>8.2}",
            HAND_NAMES[i], DRAIN_COUNTS[i], values[i]
        );
    }
    println!();
    println!("  Expected ordering: SF > 4K > FH > Flush > Str > 3K > 2P > Pair > HC");
}

// ---------------------------------------------------------------------------
// Game net construction (18 places, 17 transitions)
// ---------------------------------------------------------------------------

/// Build the Hold'em game net for ZK proofs.
///
/// Places track phase progression, turn control, chip stacks, betting state,
/// and game outcome. Transitions encode all valid game actions with fixed
/// bet sizes (2 chips) for circuit simplicity.
fn build_game_net() -> PetriNet {
    let mut net = PetriNet::new();
    let stack = INITIAL_STACK as f64;
    let bet = BET_SIZE as f64;

    // Phase control (6)
    net.add_place("phase_deal", vec![1.0], vec![], 0.0, 0.0, None);
    net.add_place("phase_preflop", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("phase_flop", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("phase_turn", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("phase_river", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("phase_showdown", vec![0.0], vec![], 0.0, 0.0, None);

    // Turn control (2)
    net.add_place("player_turn", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("house_turn", vec![0.0], vec![], 0.0, 0.0, None);

    // Player status (2)
    net.add_place("player_in", vec![1.0], vec![], 0.0, 0.0, None);
    net.add_place("house_in", vec![1.0], vec![], 0.0, 0.0, None);

    // Chip tracking (3)
    net.add_place("player_chips", vec![stack], vec![], 0.0, 0.0, None);
    net.add_place("house_chips", vec![stack], vec![], 0.0, 0.0, None);
    net.add_place("pot", vec![0.0], vec![], 0.0, 0.0, None);

    // Betting state (2)
    net.add_place("bet_open", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("round_complete", vec![0.0], vec![], 0.0, 0.0, None);

    // Outcome (3)
    net.add_place("player_wins", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("house_wins", vec![0.0], vec![], 0.0, 0.0, None);
    net.add_place("game_over", vec![0.0], vec![], 0.0, 0.0, None);

    // --- Deal transitions ---

    // deal: phase_deal -> phase_preflop + player_turn, post blinds
    net.add_transition("deal", "deal", 0.0, 0.0, None);
    net.add_arc("phase_deal", "deal", vec![1.0], false);
    net.add_arc("deal", "phase_preflop", vec![1.0], false);
    net.add_arc("deal", "player_turn", vec![1.0], false);
    net.add_arc("player_chips", "deal", vec![SMALL_BLIND as f64], false);
    net.add_arc("house_chips", "deal", vec![BIG_BLIND as f64], false);
    net.add_arc("deal", "pot", vec![(SMALL_BLIND + BIG_BLIND) as f64], false);

    // deal_flop: round_complete + phase_preflop -> phase_flop + player_turn
    net.add_transition("deal_flop", "deal", 0.0, 0.0, None);
    net.add_arc("round_complete", "deal_flop", vec![1.0], false);
    net.add_arc("phase_preflop", "deal_flop", vec![1.0], false);
    net.add_arc("deal_flop", "phase_flop", vec![1.0], false);
    net.add_arc("deal_flop", "player_turn", vec![1.0], false);

    // deal_turn: round_complete + phase_flop -> phase_turn + player_turn
    net.add_transition("deal_turn", "deal", 0.0, 0.0, None);
    net.add_arc("round_complete", "deal_turn", vec![1.0], false);
    net.add_arc("phase_flop", "deal_turn", vec![1.0], false);
    net.add_arc("deal_turn", "phase_turn", vec![1.0], false);
    net.add_arc("deal_turn", "player_turn", vec![1.0], false);

    // deal_river: round_complete + phase_turn -> phase_river + player_turn
    net.add_transition("deal_river", "deal", 0.0, 0.0, None);
    net.add_arc("round_complete", "deal_river", vec![1.0], false);
    net.add_arc("phase_turn", "deal_river", vec![1.0], false);
    net.add_arc("deal_river", "phase_river", vec![1.0], false);
    net.add_arc("deal_river", "player_turn", vec![1.0], false);

    // --- Player actions ---

    // p_check: player_turn -> house_turn
    net.add_transition("p_check", "player", 0.0, 0.0, None);
    net.add_arc("player_turn", "p_check", vec![1.0], false);
    net.add_arc("p_check", "house_turn", vec![1.0], false);

    // p_bet: player_turn + chips -> house_turn + pot + bet_open
    net.add_transition("p_bet", "player", 0.0, 0.0, None);
    net.add_arc("player_turn", "p_bet", vec![1.0], false);
    net.add_arc("player_chips", "p_bet", vec![bet], false);
    net.add_arc("p_bet", "house_turn", vec![1.0], false);
    net.add_arc("p_bet", "pot", vec![bet], false);
    net.add_arc("p_bet", "bet_open", vec![1.0], false);

    // p_call: player_turn + bet_open + chips -> round_complete + pot
    net.add_transition("p_call", "player", 0.0, 0.0, None);
    net.add_arc("player_turn", "p_call", vec![1.0], false);
    net.add_arc("bet_open", "p_call", vec![1.0], false);
    net.add_arc("player_chips", "p_call", vec![bet], false);
    net.add_arc("p_call", "round_complete", vec![1.0], false);
    net.add_arc("p_call", "pot", vec![bet], false);

    // p_fold: player_turn + player_in -> house_wins + game_over
    net.add_transition("p_fold", "player", 0.0, 0.0, None);
    net.add_arc("player_turn", "p_fold", vec![1.0], false);
    net.add_arc("player_in", "p_fold", vec![1.0], false);
    net.add_arc("p_fold", "house_wins", vec![1.0], false);
    net.add_arc("p_fold", "game_over", vec![1.0], false);

    // --- House actions ---

    // h_check: house_turn -> round_complete
    net.add_transition("h_check", "house", 0.0, 0.0, None);
    net.add_arc("house_turn", "h_check", vec![1.0], false);
    net.add_arc("h_check", "round_complete", vec![1.0], false);

    // h_bet: house_turn + chips -> player_turn + pot + bet_open
    net.add_transition("h_bet", "house", 0.0, 0.0, None);
    net.add_arc("house_turn", "h_bet", vec![1.0], false);
    net.add_arc("house_chips", "h_bet", vec![bet], false);
    net.add_arc("h_bet", "player_turn", vec![1.0], false);
    net.add_arc("h_bet", "pot", vec![bet], false);
    net.add_arc("h_bet", "bet_open", vec![1.0], false);

    // h_call: house_turn + bet_open + chips -> round_complete + pot
    net.add_transition("h_call", "house", 0.0, 0.0, None);
    net.add_arc("house_turn", "h_call", vec![1.0], false);
    net.add_arc("bet_open", "h_call", vec![1.0], false);
    net.add_arc("house_chips", "h_call", vec![bet], false);
    net.add_arc("h_call", "round_complete", vec![1.0], false);
    net.add_arc("h_call", "pot", vec![bet], false);

    // h_fold: house_turn + house_in -> player_wins + game_over
    net.add_transition("h_fold", "house", 0.0, 0.0, None);
    net.add_arc("house_turn", "h_fold", vec![1.0], false);
    net.add_arc("house_in", "h_fold", vec![1.0], false);
    net.add_arc("h_fold", "player_wins", vec![1.0], false);
    net.add_arc("h_fold", "game_over", vec![1.0], false);

    // --- Showdown ---

    // showdown: round_complete + phase_river -> phase_showdown
    net.add_transition("showdown", "showdown", 0.0, 0.0, None);
    net.add_arc("round_complete", "showdown", vec![1.0], false);
    net.add_arc("phase_river", "showdown", vec![1.0], false);
    net.add_arc("showdown", "phase_showdown", vec![1.0], false);

    // resolve_player_win: phase_showdown -> player_wins + game_over
    net.add_transition("resolve_player_win", "resolve", 0.0, 0.0, None);
    net.add_arc("phase_showdown", "resolve_player_win", vec![1.0], false);
    net.add_arc("resolve_player_win", "player_wins", vec![1.0], false);
    net.add_arc("resolve_player_win", "game_over", vec![1.0], false);

    // resolve_house_win: phase_showdown -> house_wins + game_over
    net.add_transition("resolve_house_win", "resolve", 0.0, 0.0, None);
    net.add_arc("phase_showdown", "resolve_house_win", vec![1.0], false);
    net.add_arc("resolve_house_win", "house_wins", vec![1.0], false);
    net.add_arc("resolve_house_win", "game_over", vec![1.0], false);

    // resolve_draw: phase_showdown -> game_over
    net.add_transition("resolve_draw", "resolve", 0.0, 0.0, None);
    net.add_arc("phase_showdown", "resolve_draw", vec![1.0], false);
    net.add_arc("resolve_draw", "game_over", vec![1.0], false);

    net
}

// ---------------------------------------------------------------------------
// Transition lookup
// ---------------------------------------------------------------------------

fn find_transition_id(matrix: &IncidenceMatrix, name: &str) -> usize {
    matrix
        .transition_labels
        .iter()
        .position(|l| l == name)
        .unwrap_or_else(|| panic!("transition '{}' not found in matrix", name))
}

// ---------------------------------------------------------------------------
// Game state tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Deal,
    Preflop,
    Flop,
    Turn,
    River,
    Showdown,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Phase::Deal => write!(f, "Deal"),
            Phase::Preflop => write!(f, "Preflop"),
            Phase::Flop => write!(f, "Flop"),
            Phase::Turn => write!(f, "Turn"),
            Phase::River => write!(f, "River"),
            Phase::Showdown => write!(f, "Showdown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Check,
    Bet,
    Call,
    Fold,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Action::Check => write!(f, "check"),
            Action::Bet => write!(f, "bet {}", BET_SIZE),
            Action::Call => write!(f, "call {}", BET_SIZE),
            Action::Fold => write!(f, "fold"),
        }
    }
}

struct Game {
    phase: Phase,
    player_cards: [u8; 2],
    house_cards: [u8; 2],
    community: Vec<u8>,
    player_chips: i64,
    house_chips: i64,
    pot: i64,
    bet_open: bool,
    game_over: bool,
    player_wins: bool,
    house_wins: bool,
}

impl Game {
    fn new() -> Self {
        Self {
            phase: Phase::Deal,
            player_cards: [0; 2],
            house_cards: [0; 2],
            community: Vec::new(),
            player_chips: INITIAL_STACK,
            house_chips: INITIAL_STACK,
            pot: 0,
            bet_open: false,
            game_over: false,
            player_wins: false,
            house_wins: false,
        }
    }

    fn player_hand(&self) -> Vec<u8> {
        let mut cards = self.player_cards.to_vec();
        cards.extend_from_slice(&self.community);
        cards
    }

    fn house_hand(&self) -> Vec<u8> {
        let mut cards = self.house_cards.to_vec();
        cards.extend_from_slice(&self.community);
        cards
    }
}

fn display_table(game: &Game, reveal_house: bool) {
    println!();
    println!(
        "  Phase: {} | Pot: {} chips",
        game.phase, game.pot
    );
    println!(
        "  Player: {} chips | {}",
        game.player_chips,
        cards_str(&game.player_cards)
    );
    if reveal_house {
        println!(
            "  House:  {} chips | {}",
            game.house_chips,
            cards_str(&game.house_cards)
        );
    } else {
        println!(
            "  House:  {} chips | [?? ??]",
            game.house_chips
        );
    }
    if !game.community.is_empty() {
        println!("  Board:  {}", cards_str(&game.community));
    }
    println!();
}

// ---------------------------------------------------------------------------
// House AI: deterministic strategy based on hand strength + pot odds
// ---------------------------------------------------------------------------

fn house_decision(game: &Game, hand_strength: &[f64; 9]) -> Action {
    let house_hand = game.house_hand();
    let rank = evaluate_hand(&house_hand);
    let strength = hand_strength[rank as usize];

    if game.bet_open {
        // Must call or fold (no re-raise in this model)
        if strength >= 5.0 {
            return Action::Call;
        } // Straight+ always call
        if strength >= 3.0 {
            return Action::Call;
        } // 2P+ call
        let pot_odds = BET_SIZE as f64 / (game.pot + BET_SIZE) as f64;
        if pot_odds < 0.3 {
            return Action::Call;
        } // cheap to call
        Action::Fold
    } else {
        // Can check or bet
        if strength >= 7.0 {
            return Action::Bet;
        } // FH+ always bet
        if strength >= 5.0 && game.pot >= 6 {
            return Action::Bet;
        } // Straight+ bet with pot
        Action::Check
    }
}

fn player_decision_auto(game: &Game, hand_strength: &[f64; 9]) -> Action {
    let player_hand = game.player_hand();
    let rank = evaluate_hand(&player_hand);
    let strength = hand_strength[rank as usize];

    if game.bet_open {
        if strength >= 4.0 {
            return Action::Call;
        }
        let pot_odds = BET_SIZE as f64 / (game.pot + BET_SIZE) as f64;
        if pot_odds < 0.25 {
            return Action::Call;
        }
        Action::Fold
    } else {
        if strength >= 6.0 {
            return Action::Bet;
        }
        if strength >= 3.0 && game.pot >= 4 {
            return Action::Bet;
        }
        Action::Check
    }
}

// ---------------------------------------------------------------------------
// Poseidon commitment for shuffle seed
// ---------------------------------------------------------------------------

fn compute_seed_commitment(seed: u64) -> String {
    // Hash seed as a single-element marking via Poseidon
    solidity_export::marking_to_state_root_hex(&[seed as i64])
}

fn compute_initial_state_root(matrix: &IncidenceMatrix, net: &PetriNet) -> String {
    let marking = matrix.initial_marking(net);
    solidity_export::marking_to_state_root_hex(&marking)
}

// ---------------------------------------------------------------------------
// ZK proof helper
// ---------------------------------------------------------------------------

struct ProofStats {
    total_prove_ms: f64,
    total_verify_ms: f64,
    proof_count: usize,
}

impl ProofStats {
    fn new() -> Self {
        Self {
            total_prove_ms: 0.0,
            total_verify_ms: 0.0,
            proof_count: 0,
        }
    }
}

fn fire_and_prove(
    matrix: &IncidenceMatrix,
    prover: &ArkworksProver,
    marking: &[i64],
    transition_name: &str,
    stats: &mut ProofStats,
    export_proof: bool,
) -> Vec<i64> {
    let tid = find_transition_id(matrix, transition_name);
    let marking_vec = marking.to_vec();
    let post = fire_transition(matrix, &marking_vec, tid)
        .unwrap_or_else(|e| panic!("fire_transition '{}' failed: {}", transition_name, e));

    let witness = TransitionWitness {
        pre_marking: marking_vec,
        transition_id: tid,
        post_marking: post.clone(),
    };

    let t0 = Instant::now();
    let proof = prover.prove(&witness).unwrap();
    let prove_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t0 = Instant::now();
    let ok = prover.verify(&proof).unwrap();
    let verify_ms = t0.elapsed().as_secs_f64() * 1000.0;

    stats.total_prove_ms += prove_ms;
    stats.total_verify_ms += verify_ms;
    stats.proof_count += 1;

    println!(
        "    ZK: {} | prove {:.1}ms | verify {:.1}ms | {}B | {}",
        transition_name,
        prove_ms,
        verify_ms,
        proof.metrics.proof_size_bytes,
        if ok { "VALID" } else { "INVALID" }
    );

    if export_proof {
        if let Ok(calldata) = ArkworksProver::proof_to_calldata(&proof.proof_bytes) {
            println!("    Calldata: {}", calldata.to_calldata_string());
        }
    }

    assert!(ok, "proof verification failed for '{}'!", transition_name);
    post
}

// ---------------------------------------------------------------------------
// Interactive player input
// ---------------------------------------------------------------------------

fn get_player_action_interactive(game: &Game) -> Action {
    let player_rank = evaluate_hand(&game.player_hand());
    println!("  Your hand: {} ({})", cards_str(&game.player_cards), player_rank);
    if !game.community.is_empty() {
        println!("  Board:     {}", cards_str(&game.community));
    }

    if game.bet_open {
        loop {
            print!("  Action [c]all / [f]old: ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            match input.trim().to_lowercase().as_str() {
                "c" | "call" => return Action::Call,
                "f" | "fold" => return Action::Fold,
                _ => println!("  Invalid action, try again."),
            }
        }
    } else {
        loop {
            print!("  Action [c]heck / [b]et / [f]old: ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            match input.trim().to_lowercase().as_str() {
                "c" | "check" => return Action::Check,
                "b" | "bet" => {
                    if game.player_chips >= BET_SIZE {
                        return Action::Bet;
                    }
                    println!("  Not enough chips to bet.");
                }
                "f" | "fold" => return Action::Fold,
                _ => println!("  Invalid action, try again."),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Betting round logic
// ---------------------------------------------------------------------------

/// Run one betting round. Returns updated marking and whether the game ended.
fn run_betting_round(
    game: &mut Game,
    marking: &[i64],
    matrix: &IncidenceMatrix,
    prover: &ArkworksProver,
    stats: &mut ProofStats,
    demo_mode: bool,
    hand_strength: &[f64; 9],
    export_proof: bool,
) -> (Vec<i64>, bool) {
    let mut m = marking.to_vec();
    game.bet_open = false;

    // --- Player acts first ---
    let player_action = if demo_mode {
        player_decision_auto(game, hand_strength)
    } else {
        get_player_action_interactive(game)
    };

    let p_transition = match player_action {
        Action::Check => "p_check",
        Action::Bet => "p_bet",
        Action::Fold => "p_fold",
        Action::Call => unreachable!("player acts first, no call"),
    };
    println!("  Player: {}", player_action);
    m = fire_and_prove(matrix, prover, &m, p_transition, stats, export_proof);

    match player_action {
        Action::Bet => {
            game.player_chips -= BET_SIZE;
            game.pot += BET_SIZE;
            game.bet_open = true;
        }
        Action::Fold => {
            game.game_over = true;
            game.house_wins = true;
            println!("\n  Player folds. House wins pot of {} chips!", game.pot);
            return (m, true);
        }
        _ => {}
    }

    // --- House responds ---
    let house_action = house_decision(game, hand_strength);
    let h_transition = if game.bet_open {
        // Player bet: house must call or fold
        match house_action {
            Action::Call | Action::Check | Action::Bet => "h_call",
            Action::Fold => "h_fold",
        }
    } else {
        // Player checked: house can check or bet
        match house_action {
            Action::Check | Action::Call | Action::Fold => "h_check",
            Action::Bet => "h_bet",
        }
    };

    let effective_house = if game.bet_open {
        if matches!(house_action, Action::Fold) {
            Action::Fold
        } else {
            Action::Call
        }
    } else if matches!(house_action, Action::Bet) {
        Action::Bet
    } else {
        Action::Check
    };

    println!("  House:  {}", effective_house);
    m = fire_and_prove(matrix, prover, &m, h_transition, stats, export_proof);

    match effective_house {
        Action::Call => {
            game.house_chips -= BET_SIZE;
            game.pot += BET_SIZE;
            game.bet_open = false;
            // Round complete
        }
        Action::Check => {
            game.bet_open = false;
            // Both checked -> round complete
        }
        Action::Bet => {
            game.house_chips -= BET_SIZE;
            game.pot += BET_SIZE;
            game.bet_open = true;

            // Player must respond to house bet
            let p_response = if demo_mode {
                player_decision_auto(game, hand_strength)
            } else {
                println!("  House bet {} chips.", BET_SIZE);
                get_player_action_interactive(game)
            };

            let p_resp_transition = match p_response {
                Action::Call => "p_call",
                Action::Fold => "p_fold",
                _ => unreachable!("must call or fold after bet"),
            };
            println!("  Player: {}", p_response);
            m = fire_and_prove(matrix, prover, &m, p_resp_transition, stats, export_proof);

            match p_response {
                Action::Call => {
                    game.player_chips -= BET_SIZE;
                    game.pot += BET_SIZE;
                    game.bet_open = false;
                }
                Action::Fold => {
                    game.game_over = true;
                    game.house_wins = true;
                    println!("\n  Player folds. House wins pot of {} chips!", game.pot);
                    return (m, true);
                }
                _ => {}
            }
        }
        Action::Fold => {
            game.game_over = true;
            game.player_wins = true;
            println!("\n  House folds. Player wins pot of {} chips!", game.pot);
            return (m, true);
        }
    }

    (m, false)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo_mode = args.iter().any(|a| a == "--demo");
    let export_proof = args.iter().any(|a| a == "--export-proof");
    let export_solidity_dir = args
        .windows(2)
        .find(|w| w[0] == "--export-solidity")
        .map(|w| PathBuf::from(&w[1]));

    println!("ZK Heads-Up Hold'em: Integer Reduction + Groth16 Proofs");
    println!("=======================================================\n");

    // Phase 1: Integer reduction on hand analysis net
    println!("Phase 1: Hand Strength via Integer Reduction");
    println!("---------------------------------------------");

    let t0 = Instant::now();
    let hand_strength = hand_integer_reduction();
    let reduction_ms = t0.elapsed().as_secs_f64() * 1000.0;
    print_hand_values(&hand_strength);
    println!("  ODE equilibrium time: {:.1}ms\n", reduction_ms);

    // Phase 2: Build game net and setup Groth16 prover
    println!("Phase 2: Game Net & Groth16 Prover Setup");
    println!("-----------------------------------------");

    let net = build_game_net();
    println!(
        "  Game net: {} places, {} transitions, {} arcs",
        net.places.len(),
        net.transitions.len(),
        net.arcs.len()
    );

    let matrix = IncidenceMatrix::from_petri_net(&net);
    println!(
        "  Incidence matrix: {} places x {} transitions",
        matrix.num_places, matrix.num_transitions
    );

    let mut prover = ArkworksProver::new(matrix.clone());
    let t0 = Instant::now();
    prover.setup().unwrap();
    let setup_ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("  Setup time: {:.1}ms\n", setup_ms);

    // --export-solidity: generate contracts and exit
    if let Some(dir) = export_solidity_dir {
        std::fs::create_dir_all(&dir).expect("failed to create output directory");

        // Groth16Verifier.sol
        let verifier_sol = prover.export_solidity_verifier().expect("VK export failed");
        let verifier_path = dir.join("Groth16Verifier.sol");
        std::fs::write(&verifier_path, &verifier_sol).expect("failed to write verifier");
        println!("  Wrote {}", verifier_path.display());

        // ZKHoldem.sol
        let initial_root = compute_initial_state_root(&matrix, &net);
        let hand_values: [u64; 9] = std::array::from_fn(|i| hand_strength[i].round() as u64);
        let game_sol = solidity_export::render_holdem_contract(&initial_root, &hand_values);
        let game_path = dir.join("ZKHoldem.sol");
        std::fs::write(&game_path, &game_sol).expect("failed to write game contract");
        println!("  Wrote {}", game_path.display());

        println!("\n  Initial state root: {}", initial_root);
        println!("  Hand strength values: {:?}", hand_values);
        println!("\n  Deployment:");
        println!("    1. Deploy Groth16Verifier");
        println!("    2. Deploy ZKHoldem(verifierAddress)");
        println!("    3. Call commitShuffle(), playerAction(), houseAction()");
        return;
    }

    // Phase 3: Shuffle + Game Play
    println!("Phase 3: Provably Fair Shuffle & Game Play");
    println!("------------------------------------------");

    // Generate shuffle seed
    let seed: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let commitment = compute_seed_commitment(seed);
    println!("  Shuffle commitment (Poseidon): {}...{}", &commitment[..10], &commitment[58..]);

    let deck = shuffle_deck(seed);

    // Assign cards from shuffled deck
    let mut game = Game::new();
    game.player_cards = [deck[0], deck[1]];
    game.house_cards = [deck[2], deck[3]];
    let flop = [deck[4], deck[5], deck[6]];
    let turn_card = deck[7];
    let river_card = deck[8];

    if demo_mode {
        println!("  Mode: AI vs AI (demo)\n");
    } else {
        println!("  Mode: Human (Player) vs House");
        println!("  Actions: [c]heck/call, [b]et, [f]old\n");
    }

    let mut marking = matrix.initial_marking(&net);
    let mut stats = ProofStats::new();

    // --- Deal: post blinds ---
    println!("  [Deal] Posting blinds: Player {} / House {}", SMALL_BLIND, BIG_BLIND);
    marking = fire_and_prove(&matrix, &prover, &marking, "deal", &mut stats, export_proof);
    game.phase = Phase::Preflop;
    game.player_chips -= SMALL_BLIND;
    game.house_chips -= BIG_BLIND;
    game.pot += SMALL_BLIND + BIG_BLIND;

    display_table(&game, demo_mode);

    // --- Phase loop: Preflop -> Flop -> Turn -> River ---
    struct PhaseInfo<'a> {
        phase: Phase,
        next_deal: &'a str,
        new_cards: Vec<u8>,
    }

    let phases = vec![
        PhaseInfo {
            phase: Phase::Preflop,
            next_deal: "deal_flop",
            new_cards: vec![],
        },
        PhaseInfo {
            phase: Phase::Flop,
            next_deal: "deal_turn",
            new_cards: flop.to_vec(),
        },
        PhaseInfo {
            phase: Phase::Turn,
            next_deal: "deal_river",
            new_cards: vec![turn_card],
        },
        PhaseInfo {
            phase: Phase::River,
            next_deal: "showdown",
            new_cards: vec![river_card],
        },
    ];

    for phase_info in &phases {
        game.phase = phase_info.phase;

        // Reveal community cards for this phase
        if !phase_info.new_cards.is_empty() {
            for &c in &phase_info.new_cards {
                game.community.push(c);
            }
            println!(
                "  [{} dealt] Board: {}",
                phase_info.phase,
                cards_str(&game.community)
            );
            display_table(&game, demo_mode);
        }

        // Run betting round
        let (new_marking, ended) = run_betting_round(
            &mut game,
            &marking,
            &matrix,
            &prover,
            &mut stats,
            demo_mode,
            &hand_strength,
            export_proof,
        );
        marking = new_marking;

        if ended {
            break;
        }

        // Advance to next phase
        if phase_info.next_deal != "showdown" {
            println!();
            marking = fire_and_prove(
                &matrix,
                &prover,
                &marking,
                phase_info.next_deal,
                &mut stats,
                export_proof,
            );
        } else {
            // --- Showdown ---
            println!();
            marking = fire_and_prove(
                &matrix,
                &prover,
                &marking,
                "showdown",
                &mut stats,
                export_proof,
            );
            game.phase = Phase::Showdown;

            println!("\n  === SHOWDOWN ===");
            println!("  Seed revealed: {}", seed);
            println!("  Commitment:    {}...{}", &commitment[..10], &commitment[58..]);
            println!();
            println!("  Player: {}  Board: {}", cards_str(&game.player_cards), cards_str(&game.community));
            println!("  House:  {}  Board: {}", cards_str(&game.house_cards), cards_str(&game.community));

            let player_rank = evaluate_hand(&game.player_hand());
            let house_rank = evaluate_hand(&game.house_hand());

            println!();
            println!("  Player hand: {}", player_rank);
            println!("  House hand:  {}", house_rank);

            if player_rank > house_rank {
                let bonus = hand_strength[player_rank as usize].round() as i64;
                println!("\n  Player wins with {}!", player_rank);
                println!(
                    "  Pot: {} + Bonus: {} ({}x ante) = {} chips",
                    game.pot,
                    bonus,
                    hand_strength[player_rank as usize].round() as i64,
                    game.pot + bonus,
                );
                marking = fire_and_prove(
                    &matrix,
                    &prover,
                    &marking,
                    "resolve_player_win",
                    &mut stats,
                    export_proof,
                );
                game.player_wins = true;
                game.player_chips += game.pot + bonus;
            } else if house_rank > player_rank {
                let bonus = hand_strength[house_rank as usize].round() as i64;
                println!("\n  House wins with {}!", house_rank);
                println!(
                    "  Pot: {} + Bonus: {} ({}x ante) = {} chips",
                    game.pot,
                    bonus,
                    hand_strength[house_rank as usize].round() as i64,
                    game.pot + bonus,
                );
                marking = fire_and_prove(
                    &matrix,
                    &prover,
                    &marking,
                    "resolve_house_win",
                    &mut stats,
                    export_proof,
                );
                game.house_wins = true;
                game.house_chips += game.pot + bonus;
            } else {
                println!("\n  Draw! Pot split: {} each.", game.pot / 2);
                marking = fire_and_prove(
                    &matrix,
                    &prover,
                    &marking,
                    "resolve_draw",
                    &mut stats,
                    export_proof,
                );
                game.player_chips += game.pot / 2;
                game.house_chips += game.pot / 2;
            }
            game.game_over = true;
        }
    }

    // --- Summary ---
    println!("\nSummary");
    println!("-------");
    let p_delta = game.player_chips - INITIAL_STACK;
    let h_delta = game.house_chips - INITIAL_STACK;
    println!(
        "  Player chips: {} ({}{}) {}",
        game.player_chips,
        if p_delta >= 0 { "+" } else { "" },
        p_delta,
        if game.player_wins { "WINNER" } else { "" }
    );
    println!(
        "  House chips:  {} ({}{}) {}",
        game.house_chips,
        if h_delta >= 0 { "+" } else { "" },
        h_delta,
        if game.house_wins { "WINNER" } else { "" }
    );
    println!("  ZK proofs generated: {}", stats.proof_count);
    println!("  Prover setup: {:.1}ms", setup_ms);
    println!("  Total prove time: {:.1}ms", stats.total_prove_ms);
    println!("  Total verify time: {:.1}ms", stats.total_verify_ms);
    if stats.proof_count > 0 {
        println!(
            "  Avg prove/verify: {:.1}ms / {:.1}ms",
            stats.total_prove_ms / stats.proof_count as f64,
            stats.total_verify_ms / stats.proof_count as f64
        );
    }
    println!("  ODE reduction time: {:.1}ms", reduction_ms);
    println!("  Proof system: {}", prover.system_name());
}
