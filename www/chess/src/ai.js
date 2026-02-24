// Chess AI — Minimax with alpha-beta pruning + Phase 3 pressure evaluation
// Evaluation = material (centipawns) + 0.3 * phase3 pressure differential

import { computePhase3Sided } from './chess.js';
import { legalMoves, makeMove, toFEN, gameStatus } from './engine.js';

// Material values in centipawns
const MATERIAL = { P: 100, N: 320, B: 330, R: 500, Q: 900, K: 20000 };

function materialScore(board) {
    let score = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p) {
                const v = MATERIAL[p.type];
                score += p.color === 'w' ? v : -v;
            }
        }
    return score;
}

function pressureScore(fen) {
    const { white, black } = computePhase3Sided(fen);
    let wSum = 0, bSum = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            wSum += white[r][c];
            bSum += black[r][c];
        }
    return wSum - bSum;
}

function evaluate(state) {
    const mat = materialScore(state.board);
    const pressure = pressureScore(toFEN(state));
    return mat + 0.3 * pressure;
}

// MVV-LVA move ordering for better pruning
function moveOrderScore(state, move) {
    let score = 0;
    const victim = state.board[move.to[0]][move.to[1]];
    const attacker = state.board[move.from[0]][move.from[1]];
    if (victim) {
        // MVV-LVA: high victim value - low attacker value
        score += MATERIAL[victim.type] * 10 - MATERIAL[attacker.type];
    }
    if (move.promotion) score += MATERIAL[move.promotion];
    return score;
}

function orderedMoves(state) {
    const moves = legalMoves(state);
    return moves.sort((a, b) => moveOrderScore(state, b) - moveOrderScore(state, a));
}

function alphaBeta(state, depth, alpha, beta, maximizing) {
    const status = gameStatus(state);
    if (status === 'checkmate') return maximizing ? -100000 + (4 - depth) : 100000 - (4 - depth);
    if (status === 'stalemate' || status === 'draw-50' || status === 'draw-material') return 0;
    if (depth === 0) return evaluate(state);

    const moves = orderedMoves(state);
    if (maximizing) {
        let value = -Infinity;
        for (const move of moves) {
            value = Math.max(value, alphaBeta(makeMove(state, move), depth - 1, alpha, beta, false));
            alpha = Math.max(alpha, value);
            if (alpha >= beta) break;
        }
        return value;
    } else {
        let value = Infinity;
        for (const move of moves) {
            value = Math.min(value, alphaBeta(makeMove(state, move), depth - 1, alpha, beta, true));
            beta = Math.min(beta, value);
            if (alpha >= beta) break;
        }
        return value;
    }
}

export function findBestMove(state, depth = 3) {
    const moves = orderedMoves(state);
    if (moves.length === 0) return null;

    const maximizing = state.turn === 'w';
    let bestMove = moves[0];
    let bestScore = maximizing ? -Infinity : Infinity;

    for (const move of moves) {
        const score = alphaBeta(
            makeMove(state, move),
            depth - 1,
            maximizing ? bestScore : -Infinity,
            maximizing ? Infinity : bestScore,
            !maximizing
        );
        if (maximizing ? score > bestScore : score < bestScore) {
            bestScore = score;
            bestMove = move;
        }
    }
    return { move: bestMove, score: bestScore };
}
