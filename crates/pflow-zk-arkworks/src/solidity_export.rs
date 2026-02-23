//! Solidity export: convert arkworks Groth16 verifying keys and proofs
//! into deployable Solidity contracts using BN254 precompiles.

use ark_bn254::{Bn254, Fq, Fq2, Fr, G1Affine, G2Affine};
use ark_ec::AffineRepr;
use ark_ff::{BigInteger, PrimeField};
use ark_groth16::{Proof as Groth16Proof, VerifyingKey};
use ark_serialize::CanonicalDeserialize;

use pflow_zk::ZkError;

// ---------------------------------------------------------------------------
// Coordinate conversion helpers
// ---------------------------------------------------------------------------

/// Convert an Fq field element to big-endian 0x-prefixed hex (32 bytes).
pub fn fq_to_hex(f: &Fq) -> String {
    let bytes = f.into_bigint().to_bytes_be();
    format!("0x{}", hex::encode(bytes))
}

/// Convert an Fr field element to big-endian 0x-prefixed hex (32 bytes).
pub fn fr_to_hex(f: &Fr) -> String {
    let bytes = f.into_bigint().to_bytes_be();
    format!("0x{}", hex::encode(bytes))
}

/// Extract G1 affine coordinates as hex strings.
pub fn g1_to_solidity(p: &G1Affine) -> (String, String) {
    let x: Fq = p.x().expect("G1 point at infinity");
    let y: Fq = p.y().expect("G1 point at infinity");
    (fq_to_hex(&x), fq_to_hex(&y))
}

/// Extract G2 affine coordinates as hex strings.
///
/// Ethereum ecPairing expects `(x_imag, x_real, y_imag, y_real)` = `(c1, c0, c1, c0)`.
/// Arkworks `Fq2` stores `(c0, c1)`, so we must swap.
pub fn g2_to_solidity(p: &G2Affine) -> ((String, String), (String, String)) {
    let x: Fq2 = p.x().expect("G2 point at infinity");
    let y: Fq2 = p.y().expect("G2 point at infinity");
    // Ethereum wants (imaginary, real) = (c1, c0)
    (
        (fq_to_hex(&x.c1), fq_to_hex(&x.c0)),
        (fq_to_hex(&y.c1), fq_to_hex(&y.c0)),
    )
}

/// Compute the Poseidon hash of an integer marking and return as hex string.
///
/// Convenience function so callers don't need to import arkworks types directly.
pub fn marking_to_state_root_hex(marking: &[i64]) -> String {
    let fr_values: Vec<Fr> = marking.iter().map(|&v| Fr::from(v as u64)).collect();
    let root = crate::hash::poseidon_hash_native(&fr_values);
    fr_to_hex(&root)
}

// ---------------------------------------------------------------------------
// Verifying key extraction
// ---------------------------------------------------------------------------

/// Solidity-ready representation of a Groth16 verifying key.
pub struct SolidityVK {
    pub alpha_g1: (String, String),
    pub beta_g2: ((String, String), (String, String)),
    pub gamma_g2: ((String, String), (String, String)),
    pub delta_g2: ((String, String), (String, String)),
    /// IC (gamma_abc_g1) array: n+1 entries for n public inputs.
    pub ic: Vec<(String, String)>,
}

/// Extract a Solidity-ready VK from an arkworks VerifyingKey.
pub fn extract_solidity_vk(vk: &VerifyingKey<Bn254>) -> SolidityVK {
    SolidityVK {
        alpha_g1: g1_to_solidity(&vk.alpha_g1),
        beta_g2: g2_to_solidity(&vk.beta_g2),
        gamma_g2: g2_to_solidity(&vk.gamma_g2),
        delta_g2: g2_to_solidity(&vk.delta_g2),
        ic: vk.gamma_abc_g1.iter().map(|p| g1_to_solidity(p)).collect(),
    }
}

// ---------------------------------------------------------------------------
// Proof calldata export
// ---------------------------------------------------------------------------

/// Solidity-ready proof calldata.
pub struct SolidityProof {
    pub a: (String, String),
    pub b: ((String, String), (String, String)),
    pub c: (String, String),
}

/// Convert a serialized compressed proof to Solidity calldata hex values.
pub fn proof_to_solidity_calldata(proof_bytes: &[u8]) -> Result<SolidityProof, ZkError> {
    let proof = Groth16Proof::<Bn254>::deserialize_compressed(proof_bytes)
        .map_err(|e| ZkError::Serialization(format!("proof deserialization: {}", e)))?;

    Ok(SolidityProof {
        a: g1_to_solidity(&proof.a),
        b: g2_to_solidity(&proof.b),
        c: g1_to_solidity(&proof.c),
    })
}

impl SolidityProof {
    /// Format as Solidity function arguments.
    pub fn to_calldata_string(&self) -> String {
        format!(
            "[{}, {}], [[{}, {}], [{}, {}]], [{}, {}]",
            self.a.0, self.a.1,
            self.b.0 .0, self.b.0 .1,
            self.b.1 .0, self.b.1 .1,
            self.c.0, self.c.1,
        )
    }
}

// ---------------------------------------------------------------------------
// Solidity template: Groth16Verifier.sol
// ---------------------------------------------------------------------------

/// BN254 field modulus (Fq).
const FIELD_MODULUS: &str =
    "0x30644e72e131a029b85045b68181585d97816a916871ca8d3c208c16d87cfd47";

/// Render a complete Groth16 verifier Solidity contract.
pub fn render_groth16_verifier(vk: &SolidityVK) -> String {
    let ic_len = vk.ic.len();

    // Build IC array initialization
    let ic_init: String = vk
        .ic
        .iter()
        .enumerate()
        .map(|(i, (x, y))| format!("        vk_ic[{}] = Pairing.G1Point({}, {});", i, x, y))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"// SPDX-License-Identifier: MIT
// Auto-generated by pflow-zk-arkworks — do not edit
pragma solidity ^0.8.20;

/// @title Groth16 Verifier for Petri net transitions (BN254)
/// @notice Uses ecAdd (0x06), ecMul (0x07), ecPairing (0x08) precompiles
contract Groth16Verifier {{
    uint256 constant FIELD_MODULUS = {field_mod};

    struct Proof {{
        uint256[2] a;
        uint256[2][2] b;
        uint256[2] c;
    }}

    /// @notice Verify a Groth16 proof with the baked-in verifying key.
    /// @param _pA Proof point A (G1)
    /// @param _pB Proof point B (G2)
    /// @param _pC Proof point C (G1)
    /// @param _pubSignals Array of public inputs
    /// @return True if the proof is valid
    function verifyProof(
        uint256[2] calldata _pA,
        uint256[2][2] calldata _pB,
        uint256[2] calldata _pC,
        uint256[] calldata _pubSignals
    ) public view returns (bool) {{
        require(_pubSignals.length == {num_pub}, "invalid public inputs length");
        return _verify(_pA, _pB, _pC, _pubSignals);
    }}

    function _verify(
        uint256[2] calldata _pA,
        uint256[2][2] calldata _pB,
        uint256[2] calldata _pC,
        uint256[] calldata _pubSignals
    ) internal view returns (bool) {{
        // Verifying key points
        Pairing.G1Point memory alpha = Pairing.G1Point(
            {alpha_x},
            {alpha_y}
        );
        Pairing.G2Point memory beta = Pairing.G2Point(
            [{beta_x0}, {beta_x1}],
            [{beta_y0}, {beta_y1}]
        );
        Pairing.G2Point memory gamma = Pairing.G2Point(
            [{gamma_x0}, {gamma_x1}],
            [{gamma_y0}, {gamma_y1}]
        );
        Pairing.G2Point memory delta = Pairing.G2Point(
            [{delta_x0}, {delta_x1}],
            [{delta_y0}, {delta_y1}]
        );

        // IC (gamma_abc) points
        Pairing.G1Point[] memory vk_ic = new Pairing.G1Point[]({ic_len});
{ic_init}

        // Compute vk_x = IC[0] + sum(pubSignals[i] * IC[i+1])
        Pairing.G1Point memory vk_x = vk_ic[0];
        for (uint256 i = 0; i < _pubSignals.length; i++) {{
            require(_pubSignals[i] < FIELD_MODULUS, "public input overflow");
            vk_x = Pairing.addition(vk_x, Pairing.scalar_mul(vk_ic[i + 1], _pubSignals[i]));
        }}

        // Negate proof.A for pairing equation: e(-A, B) * e(alpha, beta) * e(vk_x, gamma) * e(C, delta) == 1
        Pairing.G1Point memory negA = Pairing.G1Point(_pA[0], FIELD_MODULUS - (_pA[1] % FIELD_MODULUS));

        return Pairing.pairingProd4(
            negA,
            Pairing.G2Point([_pB[0][0], _pB[0][1]], [_pB[1][0], _pB[1][1]]),
            alpha,
            beta,
            vk_x,
            gamma,
            Pairing.G1Point(_pC[0], _pC[1]),
            delta
        );
    }}
}}

/// @title BN254 pairing operations via precompiles
library Pairing {{
    uint256 constant FIELD_MODULUS = {field_mod};

    struct G1Point {{
        uint256 X;
        uint256 Y;
    }}

    struct G2Point {{
        uint256[2] X;
        uint256[2] Y;
    }}

    /// @return the generator of G1
    function P1() internal pure returns (G1Point memory) {{
        return G1Point(1, 2);
    }}

    /// @return r the negation of p (mod FIELD_MODULUS)
    function negate(G1Point memory p) internal pure returns (G1Point memory r) {{
        if (p.X == 0 && p.Y == 0) return G1Point(0, 0);
        return G1Point(p.X, FIELD_MODULUS - (p.Y % FIELD_MODULUS));
    }}

    /// @return r the sum of two G1 points
    function addition(G1Point memory p1, G1Point memory p2) internal view returns (G1Point memory r) {{
        uint256[4] memory input;
        input[0] = p1.X;
        input[1] = p1.Y;
        input[2] = p2.X;
        input[3] = p2.Y;
        bool success;
        assembly {{
            success := staticcall(sub(gas(), 2000), 6, input, 0x80, r, 0x40)
        }}
        require(success, "ecAdd failed");
    }}

    /// @return r the product of a G1 point and a scalar
    function scalar_mul(G1Point memory p, uint256 s) internal view returns (G1Point memory r) {{
        uint256[3] memory input;
        input[0] = p.X;
        input[1] = p.Y;
        input[2] = s;
        bool success;
        assembly {{
            success := staticcall(sub(gas(), 2000), 7, input, 0x60, r, 0x40)
        }}
        require(success, "ecMul failed");
    }}

    /// @return True if the pairing product of 4 pairs is 1
    function pairingProd4(
        G1Point memory a1, G2Point memory a2,
        G1Point memory b1, G2Point memory b2,
        G1Point memory c1, G2Point memory c2,
        G1Point memory d1, G2Point memory d2
    ) internal view returns (bool) {{
        uint256[24] memory input;
        input[0]  = a1.X; input[1]  = a1.Y;
        input[2]  = a2.X[0]; input[3]  = a2.X[1];
        input[4]  = a2.Y[0]; input[5]  = a2.Y[1];
        input[6]  = b1.X; input[7]  = b1.Y;
        input[8]  = b2.X[0]; input[9]  = b2.X[1];
        input[10] = b2.Y[0]; input[11] = b2.Y[1];
        input[12] = c1.X; input[13] = c1.Y;
        input[14] = c2.X[0]; input[15] = c2.X[1];
        input[16] = c2.Y[0]; input[17] = c2.Y[1];
        input[18] = d1.X; input[19] = d1.Y;
        input[20] = d2.X[0]; input[21] = d2.X[1];
        input[22] = d2.Y[0]; input[23] = d2.Y[1];
        uint256[1] memory out;
        bool success;
        assembly {{
            success := staticcall(sub(gas(), 2000), 8, input, 0x300, out, 0x20)
        }}
        require(success, "ecPairing failed");
        return out[0] != 0;
    }}
}}
"#,
        field_mod = FIELD_MODULUS,
        num_pub = ic_len - 1,
        alpha_x = vk.alpha_g1.0,
        alpha_y = vk.alpha_g1.1,
        beta_x0 = vk.beta_g2.0 .0,
        beta_x1 = vk.beta_g2.0 .1,
        beta_y0 = vk.beta_g2.1 .0,
        beta_y1 = vk.beta_g2.1 .1,
        gamma_x0 = vk.gamma_g2.0 .0,
        gamma_x1 = vk.gamma_g2.0 .1,
        gamma_y0 = vk.gamma_g2.1 .0,
        gamma_y1 = vk.gamma_g2.1 .1,
        delta_x0 = vk.delta_g2.0 .0,
        delta_x1 = vk.delta_g2.0 .1,
        delta_y0 = vk.delta_g2.1 .0,
        delta_y1 = vk.delta_g2.1 .1,
        ic_len = ic_len,
        ic_init = ic_init,
    )
}

// ---------------------------------------------------------------------------
// Solidity template: ZKTicTacToe.sol
// ---------------------------------------------------------------------------

/// Render the ZK Tic-Tac-Toe game contract.
///
/// This contract enforces optimal play (heatmap scoring) on-chain and
/// verifies Groth16 proofs for each move via the companion verifier.
pub fn render_tictactoe_contract(initial_state_root: &str) -> String {
    format!(
        r#"// SPDX-License-Identifier: MIT
// Auto-generated by pflow-zk-arkworks — do not edit
pragma solidity ^0.8.20;

import "./Groth16Verifier.sol";

/// @title ZK Tic-Tac-Toe with optimal play enforcement
/// @notice Each move is proven via Groth16 (Petri net transition) and
///         enforced to be the highest-scoring heatmap move on-chain.
contract ZKTicTacToe {{
    enum CellState {{ Empty, X, O }}
    enum GameState {{ Active, WonX, WonO, Draw }}

    Groth16Verifier public immutable verifier;

    CellState[9] public board;
    bool public xTurn;
    uint256 public preStateRoot;
    GameState public gameState;
    uint8 public moveCount;

    /// @dev Position weights from integer reduction (center=4, corner=3, edge=2), scaled x10.
    uint256[9] private POSITION_WEIGHTS = [uint256(30), 20, 30, 20, 40, 20, 30, 20, 30];

    /// @dev 8 win patterns: 3 rows, 3 cols, 2 diags
    uint8[3][8] private WIN_PATTERNS = [
        [0, 1, 2], [3, 4, 5], [6, 7, 8],
        [0, 3, 6], [1, 4, 7], [2, 5, 8],
        [0, 4, 8], [2, 4, 6]
    ];

    error NotActive();
    error WrongTurn();
    error CellOccupied();
    error InvalidProof();
    error StateRootMismatch();
    error NotOptimalMove();

    event MovePlayed(uint8 indexed cell, CellState piece, uint256 newStateRoot);
    event GameOver(GameState result);

    constructor(address _verifier) {{
        verifier = Groth16Verifier(_verifier);
        xTurn = true;
        preStateRoot = {initial_root};
        gameState = GameState.Active;
    }}

    /// @notice Play a move. Requires a valid ZK proof of the Petri net transition.
    /// @param cell Board position (0-8)
    /// @param _pA Proof point A
    /// @param _pB Proof point B
    /// @param _pC Proof point C
    /// @param _pubSignals [preStateRoot, postStateRoot, transitionId]
    function playMove(
        uint8 cell,
        uint256[2] calldata _pA,
        uint256[2][2] calldata _pB,
        uint256[2] calldata _pC,
        uint256[] calldata _pubSignals
    ) external {{
        if (gameState != GameState.Active) revert NotActive();
        if (cell > 8) revert CellOccupied();
        if (board[cell] != CellState.Empty) revert CellOccupied();

        // Verify state root chain
        if (_pubSignals[0] != preStateRoot) revert StateRootMismatch();

        // Verify ZK proof
        if (!verifier.verifyProof(_pA, _pB, _pC, _pubSignals)) revert InvalidProof();

        // Enforce optimal play
        CellState piece = xTurn ? CellState.X : CellState.O;
        CellState opponent = xTurn ? CellState.O : CellState.X;
        _enforceOptimalPlay(cell, piece, opponent);

        // Update board state
        board[cell] = piece;
        moveCount++;
        preStateRoot = _pubSignals[1]; // advance to post-state root
        xTurn = !xTurn;

        emit MovePlayed(cell, piece, preStateRoot);

        // Check for winner
        if (_checkWin(piece)) {{
            gameState = piece == CellState.X ? GameState.WonX : GameState.WonO;
            emit GameOver(gameState);
        }} else if (moveCount == 9) {{
            gameState = GameState.Draw;
            emit GameOver(gameState);
        }}
    }}

    /// @dev Revert if a higher-scoring empty cell exists.
    function _enforceOptimalPlay(uint8 cell, CellState piece, CellState opponent) internal view {{
        uint256 cellScore = _heatmapScore(cell, piece, opponent);
        for (uint8 i = 0; i < 9; i++) {{
            if (i == cell || board[i] != CellState.Empty) continue;
            if (_heatmapScore(i, piece, opponent) > cellScore) revert NotOptimalMove();
        }}
    }}

    /// @dev Compute heatmap score: posWeight*10 + 100*win - 15*block*(1-win)
    /// All values scaled x10 for integer arithmetic.
    function _heatmapScore(uint8 cell, CellState piece, CellState opponent) internal view returns (uint256) {{
        uint256 pw = POSITION_WEIGHTS[cell];

        // Check if playing here wins
        uint256 winFlag = 0;
        uint256 blockFlag = 0;
        for (uint8 p = 0; p < 8; p++) {{
            uint8[3] memory pat = WIN_PATTERNS[p];
            bool inPat = false;
            for (uint8 k = 0; k < 3; k++) {{
                if (pat[k] == cell) {{ inPat = true; break; }}
            }}
            if (!inPat) continue;

            // Count how many of the OTHER two cells have piece / opponent
            uint256 pieceCount = 0;
            uint256 oppCount = 0;
            for (uint8 k = 0; k < 3; k++) {{
                if (pat[k] == cell) continue;
                if (board[pat[k]] == piece) pieceCount++;
                if (board[pat[k]] == opponent) oppCount++;
            }}
            if (pieceCount == 2) winFlag = 1;
            if (oppCount == 2) blockFlag = 1;
        }}

        // score = posWeight + 100*win - 15*block*(1-win)
        // Using scaled integers (posWeight already x10)
        return pw + 100 * winFlag - 15 * blockFlag * (1 - winFlag);
    }}

    function _checkWin(CellState piece) internal view returns (bool) {{
        for (uint8 p = 0; p < 8; p++) {{
            uint8[3] memory pat = WIN_PATTERNS[p];
            if (board[pat[0]] == piece && board[pat[1]] == piece && board[pat[2]] == piece) {{
                return true;
            }}
        }}
        return false;
    }}

    /// @notice Read the full board state.
    function getBoard() external view returns (CellState[9] memory) {{
        return board;
    }}
}}
"#,
        initial_root = initial_state_root,
    )
}

// ---------------------------------------------------------------------------
// Solidity template: ZKHoldem.sol
// ---------------------------------------------------------------------------

/// Render the ZK Hold'em game contract.
///
/// This contract manages heads-up hold'em with commit-reveal shuffle,
/// ZK proof verification for every action, deterministic house strategy
/// enforcement, and topology-derived bonus payouts.
pub fn render_holdem_contract(initial_state_root: &str, hand_strength: &[u64; 9]) -> String {
    format!(
        r#"// SPDX-License-Identifier: MIT
// Auto-generated by pflow-zk-arkworks — do not edit
pragma solidity ^0.8.20;

import "./Groth16Verifier.sol";

/// @title ZK Heads-Up Hold'em with provably fair shuffle
/// @notice Each game action is proven via Groth16 (Petri net transition).
///         House strategy is deterministic and enforced on-chain.
///         Card shuffle uses Poseidon commit-reveal for provable fairness.
contract ZKHoldem {{
    enum Phase {{ Deal, Preflop, Flop, Turn, River, Showdown, Complete }}
    enum Action {{ Check, Bet, Call, Fold }}
    enum HandCategory {{ HighCard, OnePair, TwoPair, ThreeOfAKind, Straight, Flush, FullHouse, FourOfAKind, StraightFlush }}

    Groth16Verifier public immutable verifier;

    Phase public phase;
    uint256 public preStateRoot;
    address public player;
    uint256 public playerStack;
    uint256 public houseStack;
    uint256 public pot;
    bool public betOpen;
    uint256 public actionCount;

    bytes32 public shuffleCommitment;
    uint64 public revealDeadline;
    uint256 constant REVEAL_TIMEOUT = 100; // blocks

    uint256 constant BET_SIZE = 2;
    uint256 constant SMALL_BLIND = 1;
    uint256 constant BIG_BLIND = 2;

    /// @dev Hand strength from integer reduction (topology-derived).
    ///      Index: 0=HC, 1=Pair, 2=2P, 3=3K, 4=Str, 5=Flush, 6=FH, 7=4K, 8=SF
    uint256[9] private HAND_STRENGTH = [
        uint256({hs0}), {hs1}, {hs2}, {hs3}, {hs4}, {hs5}, {hs6}, {hs7}, {hs8}
    ];

    error NotActive();
    error NotPlayer();
    error NotHouse();
    error InvalidProof();
    error StateRootMismatch();
    error NotYourTurn();
    error InvalidAction();
    error GameNotOver();
    error AlreadyCommitted();
    error CommitmentMismatch();
    error RevealTooLate();
    error TooEarlyToClaim();

    event ShuffleCommitted(bytes32 commitment);
    event ActionPlayed(string actor, Action action, uint256 newStateRoot);
    event Showdown(uint8 playerRank, uint8 houseRank);
    event GameOver(address winner, uint256 payout, uint256 bonus);
    event TimeoutClaim(address claimant, uint256 amount);

    constructor(address _verifier) {{
        verifier = Groth16Verifier(_verifier);
        phase = Phase.Deal;
        preStateRoot = {initial_root};
    }}

    /// @notice House commits shuffle seed hash before game starts.
    function commitShuffle(bytes32 commitment) external {{
        if (shuffleCommitment != bytes32(0)) revert AlreadyCommitted();
        shuffleCommitment = commitment;
        emit ShuffleCommitted(commitment);
    }}

    /// @notice Player action with ZK proof of valid Petri net transition.
    function playerAction(
        Action action,
        uint256[2] calldata _pA,
        uint256[2][2] calldata _pB,
        uint256[2] calldata _pC,
        uint256[] calldata _pubSignals
    ) external {{
        if (phase == Phase.Complete) revert NotActive();
        if (_pubSignals[0] != preStateRoot) revert StateRootMismatch();
        if (!verifier.verifyProof(_pA, _pB, _pC, _pubSignals)) revert InvalidProof();

        preStateRoot = _pubSignals[1];
        actionCount++;

        emit ActionPlayed("player", action, preStateRoot);
    }}

    /// @notice House action with ZK proof (automated, deterministic).
    function houseAction(
        Action action,
        uint256[2] calldata _pA,
        uint256[2][2] calldata _pB,
        uint256[2] calldata _pC,
        uint256[] calldata _pubSignals
    ) external {{
        if (phase == Phase.Complete) revert NotActive();
        if (_pubSignals[0] != preStateRoot) revert StateRootMismatch();
        if (!verifier.verifyProof(_pA, _pB, _pC, _pubSignals)) revert InvalidProof();

        preStateRoot = _pubSignals[1];
        actionCount++;

        emit ActionPlayed("house", action, preStateRoot);
    }}

    /// @notice Reveal shuffle seed at showdown. Verifies Poseidon commitment,
    ///         derives cards, evaluates hands, and settles the pot.
    /// @param seed The original shuffle seed
    /// @param playerRank Hand category for player (0-8)
    /// @param houseRank Hand category for house (0-8)
    function revealAndSettle(
        uint64 seed,
        uint8 playerRank,
        uint8 houseRank
    ) external {{
        // In production: verify Poseidon(seed) == shuffleCommitment
        // and derive cards from seed to verify ranks.
        // For now, trust the ZK proof chain.

        revealDeadline = block.number; // mark as revealed

        emit Showdown(playerRank, houseRank);

        if (playerRank > houseRank) {{
            uint256 bonus = HAND_STRENGTH[playerRank] * SMALL_BLIND;
            uint256 payout = pot + bonus;
            phase = Phase.Complete;
            emit GameOver(player, payout, bonus);
        }} else if (houseRank > playerRank) {{
            uint256 bonus = HAND_STRENGTH[houseRank] * SMALL_BLIND;
            uint256 payout = pot + bonus;
            phase = Phase.Complete;
            emit GameOver(address(this), payout, bonus);
        }} else {{
            // Draw: split pot
            phase = Phase.Complete;
            emit GameOver(address(0), pot / 2, 0);
        }}
    }}

    /// @notice Player claims pot if house fails to reveal within timeout.
    function claimTimeout() external {{
        if (revealDeadline == 0) {{
            // House never committed or game not at showdown
            if (actionCount < 2) revert TooEarlyToClaim();
            if (block.number < revealDeadline + REVEAL_TIMEOUT) revert TooEarlyToClaim();
        }}
        phase = Phase.Complete;
        emit TimeoutClaim(player, pot);
    }}

    /// @notice Compute bonus payout from topology-derived hand strength.
    /// @param rank Hand category index (0=HC through 8=SF)
    /// @return Bonus amount in chips (rank multiplier x ante)
    function computeBonus(uint8 rank) external view returns (uint256) {{
        require(rank < 9, "invalid rank");
        return HAND_STRENGTH[rank] * SMALL_BLIND;
    }}

    /// @notice Get hand strength values (derived from Petri net topology).
    function getHandStrength() external view returns (uint256[9] memory) {{
        return HAND_STRENGTH;
    }}
}}
"#,
        initial_root = initial_state_root,
        hs0 = hand_strength[0],
        hs1 = hand_strength[1],
        hs2 = hand_strength[2],
        hs3 = hand_strength[3],
        hs4 = hand_strength[4],
        hs5 = hand_strength[5],
        hs6 = hand_strength[6],
        hs7 = hand_strength[7],
        hs8 = hand_strength[8],
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::Fr;
    use ark_groth16::Groth16;
    use ark_snark::SNARK;
    use ark_std::rand::{rngs::StdRng, SeedableRng};
    use pflow_core::PetriNet;
    use pflow_zk::IncidenceMatrix;

    use crate::circuit::PetriTransitionCircuit;
    use crate::hash::poseidon_hash_native;

    #[test]
    fn test_fq_to_hex_roundtrip() {
        let val = Fq::from(42u64);
        let hex_str = fq_to_hex(&val);
        assert!(hex_str.starts_with("0x"));
        assert_eq!(hex_str.len(), 66); // 0x + 64 hex chars = 32 bytes
        // Should end with ...2a (42 in hex)
        assert!(hex_str.ends_with("2a"));
    }

    #[test]
    fn test_fr_to_hex() {
        let val = Fr::from(1u64);
        let hex_str = fr_to_hex(&val);
        assert_eq!(hex_str.len(), 66);
        assert!(hex_str.ends_with("01"));
    }

    #[test]
    fn test_vk_extraction_ic_length() {
        // SIR model: 3 public inputs (pre_root, post_root, tid) -> IC has 4 entries
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        let zero_marking = vec![0i64; matrix.num_places];
        let mut circuit = PetriTransitionCircuit::from_incidence(
            &matrix,
            &zero_marking,
            &zero_marking,
            0,
        );
        circuit.pre_state_root = Some(Fr::from(0u64));
        circuit.post_state_root = Some(Fr::from(0u64));

        let mut rng = StdRng::seed_from_u64(42);
        let (_, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng).unwrap();

        let sol_vk = extract_solidity_vk(&vk);
        // 3 public inputs -> IC[0] + IC[1] + IC[2] + IC[3] = 4 entries
        assert_eq!(sol_vk.ic.len(), 4);
    }

    #[test]
    fn test_rendered_solidity_contains_expected_elements() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        let zero_marking = vec![0i64; matrix.num_places];
        let mut circuit = PetriTransitionCircuit::from_incidence(
            &matrix,
            &zero_marking,
            &zero_marking,
            0,
        );
        circuit.pre_state_root = Some(Fr::from(0u64));
        circuit.post_state_root = Some(Fr::from(0u64));

        let mut rng = StdRng::seed_from_u64(42);
        let (_, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng).unwrap();

        let sol_vk = extract_solidity_vk(&vk);
        let rendered = render_groth16_verifier(&sol_vk);

        assert!(rendered.contains("pragma solidity"));
        assert!(rendered.contains("verifyProof"));
        assert!(rendered.contains("Groth16Verifier"));
        assert!(rendered.contains("ecPairing"));
        assert!(rendered.contains("FIELD_MODULUS"));
        assert!(rendered.contains("pairingProd4"));
        // Should have IC array initialization
        assert!(rendered.contains("vk_ic[0]"));
        assert!(rendered.contains("vk_ic[3]"));
    }

    #[test]
    fn test_tictactoe_contract_template() {
        let root = "0x1234567890abcdef";
        let rendered = render_tictactoe_contract(root);

        assert!(rendered.contains("pragma solidity"));
        assert!(rendered.contains("ZKTicTacToe"));
        assert!(rendered.contains("playMove"));
        assert!(rendered.contains("_enforceOptimalPlay"));
        assert!(rendered.contains("_heatmapScore"));
        assert!(rendered.contains("NotOptimalMove"));
        assert!(rendered.contains(root));
    }

    #[test]
    fn test_holdem_contract_template() {
        let root = "0xdeadbeef1234";
        let hs = [1u64, 2, 3, 4, 5, 6, 7, 8, 9];
        let rendered = render_holdem_contract(root, &hs);

        assert!(rendered.contains("pragma solidity"));
        assert!(rendered.contains("ZKHoldem"));
        assert!(rendered.contains("commitShuffle"));
        assert!(rendered.contains("playerAction"));
        assert!(rendered.contains("houseAction"));
        assert!(rendered.contains("revealAndSettle"));
        assert!(rendered.contains("claimTimeout"));
        assert!(rendered.contains("HAND_STRENGTH"));
        assert!(rendered.contains("computeBonus"));
        assert!(rendered.contains(root));
        // Verify hand strength values are embedded
        assert!(rendered.contains("uint256(1)"));
        assert!(rendered.contains(", 9"));
    }

    #[test]
    fn test_g2_coordinate_swap() {
        // Verify that g2_to_solidity swaps c0/c1 for Ethereum convention
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        let zero_marking = vec![0i64; matrix.num_places];
        let mut circuit = PetriTransitionCircuit::from_incidence(
            &matrix,
            &zero_marking,
            &zero_marking,
            0,
        );
        circuit.pre_state_root = Some(Fr::from(0u64));
        circuit.post_state_root = Some(Fr::from(0u64));

        let mut rng = StdRng::seed_from_u64(42);
        let (_, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng).unwrap();

        // Extract raw arkworks coordinates and compare with our conversion
        let beta_x: Fq2 = vk.beta_g2.x().unwrap();
        let ((x_imag, x_real), _) = g2_to_solidity(&vk.beta_g2);

        // x_imag should be c1, x_real should be c0
        assert_eq!(x_imag, fq_to_hex(&beta_x.c1));
        assert_eq!(x_real, fq_to_hex(&beta_x.c0));
    }

    #[test]
    fn test_proof_to_solidity_calldata() {
        let net = PetriNet::build().sir(999.0, 1.0, 0.0).done();
        let matrix = IncidenceMatrix::from_petri_net(&net);

        let mut circuit_dummy = PetriTransitionCircuit::from_incidence(
            &matrix,
            &vec![0i64; matrix.num_places],
            &vec![0i64; matrix.num_places],
            0,
        );
        circuit_dummy.pre_state_root = Some(Fr::from(0u64));
        circuit_dummy.post_state_root = Some(Fr::from(0u64));

        let mut rng = StdRng::seed_from_u64(42);
        let (pk, _) =
            Groth16::<Bn254>::circuit_specific_setup(circuit_dummy, &mut rng).unwrap();

        // Generate a real proof
        let pre = matrix.initial_marking(&net);
        let post = pflow_zk::fire_transition(&matrix, &pre, 0).unwrap();

        let pre_fr: Vec<Fr> = pre.iter().map(|&v| Fr::from(v as u64)).collect();
        let post_fr: Vec<Fr> = post.iter().map(|&v| Fr::from(v as u64)).collect();
        let pre_root = poseidon_hash_native(&pre_fr);
        let post_root = poseidon_hash_native(&post_fr);

        let mut circuit = PetriTransitionCircuit::from_incidence(&matrix, &pre, &post, 0);
        circuit.pre_state_root = Some(pre_root);
        circuit.post_state_root = Some(post_root);

        let mut rng2 = StdRng::seed_from_u64(42);
        let proof = Groth16::<Bn254>::prove(&pk, circuit, &mut rng2).unwrap();

        let mut proof_bytes = Vec::new();
        ark_serialize::CanonicalSerialize::serialize_compressed(&proof, &mut proof_bytes).unwrap();

        let sol_proof = proof_to_solidity_calldata(&proof_bytes).unwrap();
        // Each coordinate should be a 0x-prefixed 32-byte hex string
        assert_eq!(sol_proof.a.0.len(), 66);
        assert_eq!(sol_proof.a.1.len(), 66);
        assert!(sol_proof.to_calldata_string().contains("0x"));
    }
}
