// Pressure chess engine — Minimax with alpha-beta pruning + Petri net pressure evaluation
// Evaluation = material + phase-dependent pressure features + quiescence search
// Features: king zone, territory, center, mobility, hanging, convergence, passed pawns, pawn shield, mating drive

import { computePressureFromBoard } from './chess.js';
import { legalMoves, makeMove, toFEN, gameStatus, isSquareAttacked } from './engine.js';

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

// ── Spatial Features (exact structural measurements) ────────────────────

// King zone pressure: sum opponent pressure in 3x3 area around each king
function kingZonePressure(board, white, black) {
    let wKingR = 0, wKingC = 0, bKingR = 0, bKingC = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p?.type === 'K') {
                if (p.color === 'w') { wKingR = r; wKingC = c; }
                else { bKingR = r; bKingC = c; }
            }
        }

    let dangerToWhite = 0, dangerToBlack = 0;
    for (let dr = -1; dr <= 1; dr++)
        for (let dc = -1; dc <= 1; dc++) {
            const wr = wKingR + dr, wc = wKingC + dc;
            if (wr >= 0 && wr < 8 && wc >= 0 && wc < 8)
                dangerToWhite += black[wr][wc];
            const br = bKingR + dr, bc = bKingC + dc;
            if (br >= 0 && br < 8 && bc >= 0 && bc < 8)
                dangerToBlack += white[br][bc];
        }

    return dangerToBlack - dangerToWhite;
}

// Territory pressure: reward projecting pressure into opponent's half
function territoryPressure(white, black) {
    let wInEnemyHalf = 0, bInEnemyHalf = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            if (r < 4) wInEnemyHalf += white[r][c];
            if (r >= 4) bInEnemyHalf += black[r][c];
        }
    return wInEnemyHalf - bInEnemyHalf;
}

// Central pressure bonus: extra weight for center and extended center squares
function centralPressure(white, black) {
    let wCentral = 0, bCentral = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            let mult = 0;
            if ((r === 3 || r === 4) && (c === 3 || c === 4)) {
                mult = 2.0;
            } else if (r >= 2 && r <= 5 && c >= 2 && c <= 5) {
                mult = 1.5;
            }
            if (mult > 0) {
                wCentral += white[r][c] * mult;
                bCentral += black[r][c] * mult;
            }
        }
    return wCentral - bCentral;
}

// Pressure mobility: count squares where each side has pressure > 0
function pressureMobility(white, black) {
    let wCount = 0, bCount = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            if (white[r][c] > 0) wCount++;
            if (black[r][c] > 0) bCount++;
        }
    return wCount - bCount;
}

// ── New Structural Features ─────────────────────────────────────────────

// Game phase detection: count total non-pawn non-king material
function gamePhase(board) {
    let total = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p && p.type !== 'P' && p.type !== 'K') {
                total += MATERIAL[p.type];
            }
        }
    if (total >= 5000) return 'opening';
    if (total >= 2000) return 'middlegame';
    return 'endgame';
}

// Hanging pieces: piece with 0 own pressure but opponent pressure > 0
function hangingPenalty(board, white, black) {
    let score = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (!p || p.type === 'K') continue;
            if (p.color === 'w') {
                // White piece: defended by white[r][c], attacked by black[r][c]
                if (white[r][c] === 0 && black[r][c] > 0)
                    score -= MATERIAL[p.type]; // Bad for white
            } else {
                // Black piece: defended by black[r][c], attacked by white[r][c]
                if (black[r][c] === 0 && white[r][c] > 0)
                    score += MATERIAL[p.type]; // Good for white
            }
        }
    return score;
}

// Convergence: count squares where 2+ pieces converge (attack same square)
function convergence(whiteCount, blackCount) {
    let wConv = 0, bConv = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            if (whiteCount[r][c] >= 2) wConv += whiteCount[r][c] - 1;
            if (blackCount[r][c] >= 2) bConv += blackCount[r][c] - 1;
        }
    return wConv - bConv;
}

// Passed pawns: pawn with no opposing pawns on same/adjacent files ahead
function passedPawnScore(board) {
    let score = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (!p || p.type !== 'P') continue;

            let passed = true;
            if (p.color === 'w') {
                // White pawn advances toward row 0
                for (let rr = r - 1; rr >= 0; rr--) {
                    for (let dc = -1; dc <= 1; dc++) {
                        const cc = c + dc;
                        if (cc < 0 || cc > 7) continue;
                        const opp = board[rr][cc];
                        if (opp && opp.type === 'P' && opp.color === 'b') {
                            passed = false;
                            break;
                        }
                    }
                    if (!passed) break;
                }
                if (passed) {
                    // Advancement: rows from promotion (row 0). Start rank is 6.
                    score += (6 - r) * 15; // 15cp per rank advanced
                }
            } else {
                // Black pawn advances toward row 7
                for (let rr = r + 1; rr < 8; rr++) {
                    for (let dc = -1; dc <= 1; dc++) {
                        const cc = c + dc;
                        if (cc < 0 || cc > 7) continue;
                        const opp = board[rr][cc];
                        if (opp && opp.type === 'P' && opp.color === 'w') {
                            passed = false;
                            break;
                        }
                    }
                    if (!passed) break;
                }
                if (passed) {
                    score -= (r - 1) * 15;
                }
            }
        }
    return score;
}

// Pawn shield: count friendly pawns in 2 ranks ahead of own king, columns ±1
function pawnShield(board) {
    let wKingR = 7, wKingC = 4, bKingR = 0, bKingC = 4;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p?.type === 'K') {
                if (p.color === 'w') { wKingR = r; wKingC = c; }
                else { bKingR = r; bKingC = c; }
            }
        }

    let wShield = 0, bShield = 0;
    // White king: look at rows king-1 and king-2 (toward row 0)
    for (let dr = -1; dr >= -2; dr--) {
        const rr = wKingR + dr;
        if (rr < 0) continue;
        for (let dc = -1; dc <= 1; dc++) {
            const cc = wKingC + dc;
            if (cc < 0 || cc > 7) continue;
            const p = board[rr][cc];
            if (p && p.type === 'P' && p.color === 'w') wShield++;
        }
    }
    // Black king: look at rows king+1 and king+2 (toward row 7)
    for (let dr = 1; dr <= 2; dr++) {
        const rr = bKingR + dr;
        if (rr > 7) continue;
        for (let dc = -1; dc <= 1; dc++) {
            const cc = bKingC + dc;
            if (cc < 0 || cc > 7) continue;
            const p = board[rr][cc];
            if (p && p.type === 'P' && p.color === 'b') bShield++;
        }
    }
    return wShield - bShield;
}

// Mating drive: push opponent king to corner when ahead in material
function matingDriveScore(board, white, black, mat) {
    // Find kings
    let wKingR = 0, wKingC = 0, bKingR = 0, bKingC = 0;
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p?.type === 'K') {
                if (p.color === 'w') { wKingR = r; wKingC = c; }
                else { bKingR = r; bKingC = c; }
            }
        }

    const sign = mat > 0 ? 1 : -1;
    const loserR = mat > 0 ? bKingR : wKingR;
    const loserC = mat > 0 ? bKingC : wKingC;
    const winnerR = mat > 0 ? wKingR : bKingR;
    const winnerC = mat > 0 ? wKingC : bKingC;

    // Push loser king toward corner: Manhattan distance from center (3.5, 3.5)
    const centerDist = Math.abs(loserR - 3.5) + Math.abs(loserC - 3.5);

    // Reward king proximity (winner king close to loser king helps mate)
    const kingDist = Math.abs(winnerR - loserR) + Math.abs(winnerC - loserC);
    const proximity = 14 - kingDist; // Max Manhattan distance is 14

    // Count loser king escape squares (low = more restricted)
    let escapes = 0;
    const loserColor = mat > 0 ? 'b' : 'w';
    const winnerColor = mat > 0 ? 'w' : 'b';
    const attackGrid = mat > 0 ? white : black;
    for (let dr = -1; dr <= 1; dr++)
        for (let dc = -1; dc <= 1; dc++) {
            if (dr === 0 && dc === 0) continue;
            const nr = loserR + dr, nc = loserC + dc;
            if (nr < 0 || nr > 7 || nc < 0 || nc > 7) continue;
            const occ = board[nr][nc];
            if (occ && occ.color === loserColor) continue; // Own piece blocks
            if (attackGrid[nr][nc] === 0) escapes++;
        }
    const restriction = 8 - escapes; // Max 8 possible adjacent squares

    return sign * (centerDist * 10 + proximity * 5 + restriction * 15);
}

// ── Phase-Dependent Evaluation ──────────────────────────────────────────

function evaluate(state) {
    const { white, black, whiteCount, blackCount } = computePressureFromBoard(state.board);
    const phase = gamePhase(state.board);
    const mat = materialScore(state.board);

    const kz = kingZonePressure(state.board, white, black);
    const terr = territoryPressure(white, black);
    const cent = centralPressure(white, black);
    const mob = pressureMobility(white, black);
    const hang = hangingPenalty(state.board, white, black);
    const conv = convergence(whiteCount, blackCount);

    let score = mat + 0.5 * hang;

    if (phase === 'opening') {
        score += 2.0*kz + 0.5*terr + 1.5*cent + 1.0*mob + 0.5*conv + 15.0*pawnShield(state.board);
    } else if (phase === 'middlegame') {
        score += 3.0*kz + 0.5*terr + 1.0*cent + 1.0*mob + 1.0*conv + 10.0*pawnShield(state.board)
               + 1.0*passedPawnScore(state.board);
    } else {
        score += 1.0*kz + 0.5*terr + 0.5*cent + 1.5*mob + 0.5*conv
               + 5.0*passedPawnScore(state.board);
        if (Math.abs(mat) >= 300) score += matingDriveScore(state.board, white, black, mat);
    }
    return score;
}

// ── Move Ordering ───────────────────────────────────────────────────────

function moveOrderScore(state, move) {
    let score = 0;
    const victim = state.board[move.to[0]][move.to[1]];
    const attacker = state.board[move.from[0]][move.from[1]];
    if (victim) {
        score += MATERIAL[victim.type] * 10 - MATERIAL[attacker.type];
    }
    if (move.promotion) score += MATERIAL[move.promotion];
    return score;
}

function orderedMoves(state) {
    const moves = legalMoves(state);
    return moves.sort((a, b) => moveOrderScore(state, b) - moveOrderScore(state, a));
}

// ── Capture / Check Detection Helpers ───────────────────────────────────

function isCapture(state, move) {
    const [tr, tc] = move.to;
    if (state.board[tr][tc]) return true;
    // En passant: pawn moves diagonally to empty square
    const piece = state.board[move.from[0]][move.from[1]];
    if (piece?.type === 'P' && move.from[1] !== tc) return true;
    return false;
}

function isCaptureOrPromotion(state, move) {
    return isCapture(state, move) || !!move.promotion;
}

function findKingPos(board, color) {
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p && p.type === 'K' && p.color === color) return [r, c];
        }
    return null;
}

function movePutsInCheck(state, move) {
    const newState = makeMove(state, move);
    const enemyColor = newState.turn; // After makeMove, turn flipped to enemy
    const kingPos = findKingPos(newState.board, enemyColor);
    if (!kingPos) return false;
    const attackerColor = enemyColor === 'w' ? 'b' : 'w';
    return isSquareAttacked(newState.board, kingPos[0], kingPos[1], attackerColor);
}

// ── Quiescence Search ───────────────────────────────────────────────────

const MAX_QDEPTH = 8;

function quiescence(state, alpha, beta, maximizing, qDepth) {
    // Stand-pat: evaluate the quiet position
    const standPat = evaluate(state);

    if (qDepth >= MAX_QDEPTH) return standPat;

    if (maximizing) {
        if (standPat >= beta) return beta;
        if (standPat > alpha) alpha = standPat;
    } else {
        if (standPat <= alpha) return alpha;
        if (standPat < beta) beta = standPat;
    }

    // Generate and order capture/promotion moves only
    const moves = legalMoves(state);
    const tactical = [];
    for (const move of moves) {
        if (isCaptureOrPromotion(state, move)) {
            tactical.push(move);
        }
    }
    tactical.sort((a, b) => moveOrderScore(state, b) - moveOrderScore(state, a));

    if (maximizing) {
        for (const move of tactical) {
            const score = quiescence(makeMove(state, move), alpha, beta, false, qDepth + 1);
            if (score > alpha) alpha = score;
            if (alpha >= beta) return beta;
        }
        return alpha;
    } else {
        for (const move of tactical) {
            const score = quiescence(makeMove(state, move), alpha, beta, true, qDepth + 1);
            if (score < beta) beta = score;
            if (alpha >= beta) return alpha;
        }
        return beta;
    }
}

// ── Alpha-Beta with Check Extensions ────────────────────────────────────

function alphaBeta(state, depth, alpha, beta, maximizing, extensions) {
    const status = gameStatus(state);
    if (status === 'checkmate') return maximizing ? -100000 + (4 - depth) : 100000 - (4 - depth);
    if (status === 'stalemate' || status === 'draw-50' || status === 'draw-material') return 0;
    if (depth <= 0) return quiescence(state, alpha, beta, maximizing, 0);

    const moves = orderedMoves(state);
    if (maximizing) {
        let value = -Infinity;
        for (const move of moves) {
            // Check extension: if move gives check, search 1 ply deeper
            let ext = 0;
            if (extensions < 2 && movePutsInCheck(state, move)) {
                ext = 1;
            }
            value = Math.max(value, alphaBeta(
                makeMove(state, move), depth - 1 + ext, alpha, beta, false, extensions + ext
            ));
            alpha = Math.max(alpha, value);
            if (alpha >= beta) break;
        }
        return value;
    } else {
        let value = Infinity;
        for (const move of moves) {
            let ext = 0;
            if (extensions < 2 && movePutsInCheck(state, move)) {
                ext = 1;
            }
            value = Math.min(value, alphaBeta(
                makeMove(state, move), depth - 1 + ext, alpha, beta, true, extensions + ext
            ));
            beta = Math.min(beta, value);
            if (alpha >= beta) break;
        }
        return value;
    }
}

// ── Public API ──────────────────────────────────────────────────────────

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
            !maximizing,
            0 // extensions counter starts at 0
        );
        if (maximizing ? score > bestScore : score < bestScore) {
            bestScore = score;
            bestMove = move;
        }
    }
    return { move: bestMove, score: bestScore };
}
