//! Independent tic-tac-toe game engine for Conjecture 4.
//!
//! This is a standalone TTT implementation that knows NOTHING about Petri nets.
//! It generates raw game traces (board state vectors before/after each move)
//! for training ReLU networks. The Petri net structure must be discovered
//! from this data, not assumed.

/// Cell state: Empty=0, X=1, O=2
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cell {
    Empty,
    X,
    O,
}

/// Who plays next.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Turn {
    X,
    O,
}

/// Game state — pure game logic, no Petri net.
#[derive(Clone, Debug)]
pub struct TttGame {
    pub board: [Cell; 9],
    pub turn: Turn,
    pub move_count: u32,
    pub winner: Option<Turn>,
}

const WIN_LINES: [[usize; 3]; 8] = [
    [0, 1, 2], [3, 4, 5], [6, 7, 8], // rows
    [0, 3, 6], [1, 4, 7], [2, 5, 8], // cols
    [0, 4, 8], [2, 4, 6],            // diags
];

impl TttGame {
    pub fn new() -> Self {
        Self {
            board: [Cell::Empty; 9],
            turn: Turn::X,
            move_count: 0,
            winner: None,
        }
    }

    pub fn is_over(&self) -> bool {
        self.winner.is_some() || self.move_count == 9
    }

    pub fn play(&mut self, cell: usize) -> bool {
        if cell >= 9 || self.board[cell] != Cell::Empty || self.is_over() {
            return false;
        }
        let mark = match self.turn {
            Turn::X => Cell::X,
            Turn::O => Cell::O,
        };
        self.board[cell] = mark;
        self.move_count += 1;

        // Check win
        for line in &WIN_LINES {
            if self.board[line[0]] == mark
                && self.board[line[1]] == mark
                && self.board[line[2]] == mark
            {
                self.winner = Some(self.turn);
                break;
            }
        }

        // Switch turn
        self.turn = match self.turn {
            Turn::X => Turn::O,
            Turn::O => Turn::X,
        };
        true
    }

    /// Encode game state as a 33-element vector matching the petri-pilot schema:
    ///
    /// Indices 0-8:   p00..p22 (cell available: 1 if empty, 0 otherwise)
    /// Indices 9-17:  x00..x22 (X placed: 1 if X, 0 otherwise)
    /// Indices 18-26: o00..o22 (O placed: 1 if O, 0 otherwise)
    /// Index 27: game_active (1 if not over)
    /// Index 28: move_tokens (count of moves made)
    /// Index 29: o_turn (1 if O's turn)
    /// Index 30: win_o (1 if O won)
    /// Index 31: win_x (1 if X won)
    /// Index 32: x_turn (1 if X's turn)
    ///
    /// This ordering matches alphabetical sort of the petri-pilot place labels:
    /// game_active, move_tokens, o00..o22, o_turn, p00..p22, win_o, win_x, x00..x22, x_turn
    pub fn to_vector(&self) -> Vec<f64> {
        let mut v = vec![0.0; 33];

        // game_active (idx 0)
        v[0] = if self.is_over() { 0.0 } else { 1.0 };

        // move_tokens (idx 1)
        v[1] = self.move_count as f64;

        // o00..o22 (idx 2..10)
        for i in 0..9 {
            v[2 + i] = if self.board[i] == Cell::O { 1.0 } else { 0.0 };
        }

        // o_turn (idx 11)
        v[11] = if !self.is_over() && self.turn == Turn::O {
            1.0
        } else {
            0.0
        };

        // p00..p22 (idx 12..20)
        for i in 0..9 {
            v[12 + i] = if self.board[i] == Cell::Empty { 1.0 } else { 0.0 };
        }

        // win_o (idx 21)
        v[21] = if self.winner == Some(Turn::O) { 1.0 } else { 0.0 };

        // win_x (idx 22)
        v[22] = if self.winner == Some(Turn::X) { 1.0 } else { 0.0 };

        // x00..x22 (idx 23..31)
        for i in 0..9 {
            v[23 + i] = if self.board[i] == Cell::X { 1.0 } else { 0.0 };
        }

        // x_turn (idx 32)
        v[32] = if !self.is_over() && self.turn == Turn::X {
            1.0
        } else {
            0.0
        };

        v
    }
}

/// Generate training data from random games. Each sample is (pre_state, post_state)
/// for a specific cell being played. Returns data for ALL moves, tagged by cell index
/// and player.
pub fn generate_game_traces(
    num_games: usize,
    seed: u64,
) -> Vec<(usize, Turn, Vec<f64>, Vec<f64>)> {
    let mut traces = Vec::new();
    let mut rng_state = seed;
    let mut next_rand = || -> u64 {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        rng_state >> 33
    };

    for _ in 0..num_games {
        let mut game = TttGame::new();

        while !game.is_over() {
            // Find empty cells
            let empty: Vec<usize> = (0..9)
                .filter(|&i| game.board[i] == Cell::Empty)
                .collect();
            if empty.is_empty() {
                break;
            }

            let cell = empty[next_rand() as usize % empty.len()];
            let pre = game.to_vector();
            let player = game.turn;
            game.play(cell);
            let post = game.to_vector();

            traces.push((cell, player, pre, post));
        }
    }

    traces
}
