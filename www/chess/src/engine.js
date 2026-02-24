// Chess Engine — Legal move generation, game state, FEN round-trip
// Pure functions, immutable state transitions.

import { DIAG, ORTHO, ALL8, KNIGHT_DELTAS, inBounds } from './chess.js';

// ── FEN Parsing ──────────────────────────────────────────────────────────

export function parseFENFull(fen) {
    const parts = fen.trim().split(/\s+/);
    const ranks = parts[0].split('/');
    const board = Array.from({ length: 8 }, () => Array(8).fill(null));
    for (let r = 0; r < 8; r++) {
        let c = 0;
        for (const ch of ranks[r]) {
            if (ch >= '1' && ch <= '8') {
                c += parseInt(ch);
            } else {
                board[r][c] = {
                    type: ch.toUpperCase(),
                    color: ch === ch.toUpperCase() ? 'w' : 'b',
                };
                c++;
            }
        }
    }
    return {
        board,
        turn: parts[1] || 'w',
        castling: parts[2] || '-',
        enPassant: parts[3] || '-',
        halfmove: parseInt(parts[4]) || 0,
        fullmove: parseInt(parts[5]) || 1,
    };
}

export function toFEN(state) {
    let fen = '';
    for (let r = 0; r < 8; r++) {
        let empty = 0;
        for (let c = 0; c < 8; c++) {
            const p = state.board[r][c];
            if (!p) { empty++; continue; }
            if (empty > 0) { fen += empty; empty = 0; }
            const ch = p.color === 'w' ? p.type : p.type.toLowerCase();
            fen += ch;
        }
        if (empty > 0) fen += empty;
        if (r < 7) fen += '/';
    }
    return `${fen} ${state.turn} ${state.castling} ${state.enPassant} ${state.halfmove} ${state.fullmove}`;
}

// ── Board Helpers ────────────────────────────────────────────────────────

function colorOf(board, r, c) {
    const p = board[r][c];
    return p ? p.color : null;
}

function cloneBoard(board) {
    return board.map(row => row.map(cell => cell ? { ...cell } : null));
}

// ── Attack Detection ─────────────────────────────────────────────────────

export function isSquareAttacked(board, r, c, byColor) {
    // Pawns
    const pawnDir = byColor === 'w' ? 1 : -1;
    for (const dc of [-1, 1]) {
        const pr = r + pawnDir, pc = c + dc;
        if (inBounds(pr, pc)) {
            const p = board[pr][pc];
            if (p && p.color === byColor && p.type === 'P') return true;
        }
    }
    // Knights
    for (const [dr, dc] of KNIGHT_DELTAS) {
        const nr = r + dr, nc = c + dc;
        if (inBounds(nr, nc)) {
            const p = board[nr][nc];
            if (p && p.color === byColor && p.type === 'N') return true;
        }
    }
    // King
    for (const [dr, dc] of ALL8) {
        const nr = r + dr, nc = c + dc;
        if (inBounds(nr, nc)) {
            const p = board[nr][nc];
            if (p && p.color === byColor && p.type === 'K') return true;
        }
    }
    // Sliding: bishop/queen on diagonals
    for (const [dr, dc] of DIAG) {
        let nr = r + dr, nc = c + dc;
        while (inBounds(nr, nc)) {
            const p = board[nr][nc];
            if (p) {
                if (p.color === byColor && (p.type === 'B' || p.type === 'Q')) return true;
                break;
            }
            nr += dr; nc += dc;
        }
    }
    // Sliding: rook/queen on orthogonals
    for (const [dr, dc] of ORTHO) {
        let nr = r + dr, nc = c + dc;
        while (inBounds(nr, nc)) {
            const p = board[nr][nc];
            if (p) {
                if (p.color === byColor && (p.type === 'R' || p.type === 'Q')) return true;
                break;
            }
            nr += dr; nc += dc;
        }
    }
    return false;
}

function findKing(board, color) {
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p && p.type === 'K' && p.color === color) return [r, c];
        }
    return null;
}

// ── Pseudo-Legal Move Generation ─────────────────────────────────────────

// Move: { from: [r,c], to: [r,c], promotion?: 'Q'|'R'|'B'|'N', castle?: 'K'|'Q' }

function addSlidingMoves(moves, board, r, c, dirs, color) {
    for (const [dr, dc] of dirs) {
        let nr = r + dr, nc = c + dc;
        while (inBounds(nr, nc)) {
            const target = board[nr][nc];
            if (target) {
                if (target.color !== color) {
                    moves.push({ from: [r, c], to: [nr, nc] });
                }
                break;
            }
            moves.push({ from: [r, c], to: [nr, nc] });
            nr += dr; nc += dc;
        }
    }
}

function generatePseudoLegal(state) {
    const { board, turn, castling, enPassant } = state;
    const moves = [];
    const enemy = turn === 'w' ? 'b' : 'w';

    for (let r = 0; r < 8; r++) {
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (!p || p.color !== turn) continue;

            switch (p.type) {
                case 'P': {
                    const dir = turn === 'w' ? -1 : 1;
                    const startRank = turn === 'w' ? 6 : 1;
                    const promoRank = turn === 'w' ? 0 : 7;

                    // Forward 1
                    const nr = r + dir;
                    if (inBounds(nr, c) && !board[nr][c]) {
                        if (nr === promoRank) {
                            for (const pr of ['Q', 'R', 'B', 'N'])
                                moves.push({ from: [r, c], to: [nr, c], promotion: pr });
                        } else {
                            moves.push({ from: [r, c], to: [nr, c] });
                            // Forward 2 from start
                            if (r === startRank) {
                                const nr2 = r + 2 * dir;
                                if (!board[nr2][c])
                                    moves.push({ from: [r, c], to: [nr2, c] });
                            }
                        }
                    }
                    // Captures
                    for (const dc of [-1, 1]) {
                        const nc = c + dc;
                        if (!inBounds(nr, nc)) continue;
                        const target = board[nr][nc];
                        const isEnPassant = enPassant !== '-' &&
                            nr === (turn === 'w' ? 2 : 5) &&
                            nc === 'abcdefgh'.indexOf(enPassant[0]) &&
                            parseInt(enPassant[1]) === (8 - nr);
                        if ((target && target.color === enemy) || isEnPassant) {
                            if (nr === promoRank) {
                                for (const pr of ['Q', 'R', 'B', 'N'])
                                    moves.push({ from: [r, c], to: [nr, nc], promotion: pr });
                            } else {
                                moves.push({ from: [r, c], to: [nr, nc] });
                            }
                        }
                    }
                    break;
                }
                case 'N':
                    for (const [dr, dc] of KNIGHT_DELTAS) {
                        const nr = r + dr, nc = c + dc;
                        if (inBounds(nr, nc) && colorOf(board, nr, nc) !== turn)
                            moves.push({ from: [r, c], to: [nr, nc] });
                    }
                    break;
                case 'B':
                    addSlidingMoves(moves, board, r, c, DIAG, turn);
                    break;
                case 'R':
                    addSlidingMoves(moves, board, r, c, ORTHO, turn);
                    break;
                case 'Q':
                    addSlidingMoves(moves, board, r, c, ALL8, turn);
                    break;
                case 'K':
                    for (const [dr, dc] of ALL8) {
                        const nr = r + dr, nc = c + dc;
                        if (inBounds(nr, nc) && colorOf(board, nr, nc) !== turn)
                            moves.push({ from: [r, c], to: [nr, nc] });
                    }
                    // Castling
                    if (turn === 'w') {
                        if (castling.includes('K') && r === 7 && c === 4 &&
                            !board[7][5] && !board[7][6] &&
                            board[7][7]?.type === 'R' && board[7][7]?.color === 'w' &&
                            !isSquareAttacked(board, 7, 4, 'b') &&
                            !isSquareAttacked(board, 7, 5, 'b') &&
                            !isSquareAttacked(board, 7, 6, 'b'))
                            moves.push({ from: [7, 4], to: [7, 6], castle: 'K' });
                        if (castling.includes('Q') && r === 7 && c === 4 &&
                            !board[7][3] && !board[7][2] && !board[7][1] &&
                            board[7][0]?.type === 'R' && board[7][0]?.color === 'w' &&
                            !isSquareAttacked(board, 7, 4, 'b') &&
                            !isSquareAttacked(board, 7, 3, 'b') &&
                            !isSquareAttacked(board, 7, 2, 'b'))
                            moves.push({ from: [7, 4], to: [7, 2], castle: 'Q' });
                    } else {
                        if (castling.includes('k') && r === 0 && c === 4 &&
                            !board[0][5] && !board[0][6] &&
                            board[0][7]?.type === 'R' && board[0][7]?.color === 'b' &&
                            !isSquareAttacked(board, 0, 4, 'w') &&
                            !isSquareAttacked(board, 0, 5, 'w') &&
                            !isSquareAttacked(board, 0, 6, 'w'))
                            moves.push({ from: [0, 4], to: [0, 6], castle: 'K' });
                        if (castling.includes('q') && r === 0 && c === 4 &&
                            !board[0][3] && !board[0][2] && !board[0][1] &&
                            board[0][0]?.type === 'R' && board[0][0]?.color === 'b' &&
                            !isSquareAttacked(board, 0, 4, 'w') &&
                            !isSquareAttacked(board, 0, 3, 'w') &&
                            !isSquareAttacked(board, 0, 2, 'w'))
                            moves.push({ from: [0, 4], to: [0, 2], castle: 'Q' });
                    }
                    break;
            }
        }
    }
    return moves;
}

// ── Legal Moves ──────────────────────────────────────────────────────────

export function legalMoves(state) {
    const pseudo = generatePseudoLegal(state);
    const legal = [];
    const enemy = state.turn === 'w' ? 'b' : 'w';

    for (const move of pseudo) {
        const newBoard = applyMoveOnBoard(state, move);
        const kingPos = findKing(newBoard, state.turn);
        if (kingPos && !isSquareAttacked(newBoard, kingPos[0], kingPos[1], enemy)) {
            legal.push(move);
        }
    }
    return legal;
}

// Apply move to board only (used for legality check, not full state transition)
function applyMoveOnBoard(state, move) {
    const board = cloneBoard(state.board);
    const [fr, fc] = move.from;
    const [tr, tc] = move.to;
    const piece = board[fr][fc];

    board[tr][tc] = move.promotion
        ? { type: move.promotion, color: piece.color }
        : piece;
    board[fr][fc] = null;

    // Castling: move rook
    if (move.castle) {
        if (move.castle === 'K') {
            board[fr][5] = board[fr][7];
            board[fr][7] = null;
        } else {
            board[fr][3] = board[fr][0];
            board[fr][0] = null;
        }
    }

    // En passant capture
    if (piece.type === 'P' && fc !== tc && !state.board[tr][tc]) {
        board[fr][tc] = null;
    }

    return board;
}

// ── Make Move (Full State Transition) ────────────────────────────────────

export function makeMove(state, move) {
    const board = cloneBoard(state.board);
    const [fr, fc] = move.from;
    const [tr, tc] = move.to;
    const piece = board[fr][fc];
    const captured = board[tr][tc];

    board[tr][tc] = move.promotion
        ? { type: move.promotion, color: piece.color }
        : piece;
    board[fr][fc] = null;

    // Castling: move rook
    if (move.castle) {
        if (move.castle === 'K') {
            board[fr][5] = board[fr][7];
            board[fr][7] = null;
        } else {
            board[fr][3] = board[fr][0];
            board[fr][0] = null;
        }
    }

    // En passant capture
    let epCapture = false;
    if (piece.type === 'P' && fc !== tc && !captured) {
        board[fr][tc] = null;
        epCapture = true;
    }

    // Update castling rights
    let cas = state.castling;
    if (piece.type === 'K') {
        if (piece.color === 'w') cas = cas.replace(/[KQ]/g, '');
        else cas = cas.replace(/[kq]/g, '');
    }
    if (piece.type === 'R') {
        if (fr === 7 && fc === 0) cas = cas.replace('Q', '');
        if (fr === 7 && fc === 7) cas = cas.replace('K', '');
        if (fr === 0 && fc === 0) cas = cas.replace('q', '');
        if (fr === 0 && fc === 7) cas = cas.replace('k', '');
    }
    // Rook captured
    if (captured?.type === 'R') {
        if (tr === 7 && tc === 0) cas = cas.replace('Q', '');
        if (tr === 7 && tc === 7) cas = cas.replace('K', '');
        if (tr === 0 && tc === 0) cas = cas.replace('q', '');
        if (tr === 0 && tc === 7) cas = cas.replace('k', '');
    }
    if (cas === '') cas = '-';

    // En passant target
    let ep = '-';
    if (piece.type === 'P' && Math.abs(tr - fr) === 2) {
        const epR = (fr + tr) / 2;
        ep = 'abcdefgh'[fc] + (8 - epR);
    }

    // Halfmove clock
    const halfmove = (piece.type === 'P' || captured || epCapture)
        ? 0 : state.halfmove + 1;

    const fullmove = state.turn === 'b' ? state.fullmove + 1 : state.fullmove;

    return {
        board,
        turn: state.turn === 'w' ? 'b' : 'w',
        castling: cas,
        enPassant: ep,
        halfmove,
        fullmove,
    };
}

// ── Game Status ──────────────────────────────────────────────────────────

export function gameStatus(state) {
    const moves = legalMoves(state);
    const enemy = state.turn === 'w' ? 'b' : 'w';
    const kingPos = findKing(state.board, state.turn);
    const inCheck = kingPos && isSquareAttacked(state.board, kingPos[0], kingPos[1], enemy);

    if (moves.length === 0) {
        return inCheck ? 'checkmate' : 'stalemate';
    }
    if (state.halfmove >= 100) return 'draw-50';
    if (isInsufficientMaterial(state.board)) return 'draw-material';
    if (inCheck) return 'check';
    return 'playing';
}

function isInsufficientMaterial(board) {
    const pieces = [];
    for (let r = 0; r < 8; r++)
        for (let c = 0; c < 8; c++) {
            const p = board[r][c];
            if (p && p.type !== 'K') pieces.push(p);
        }
    if (pieces.length === 0) return true; // K vs K
    if (pieces.length === 1 && (pieces[0].type === 'B' || pieces[0].type === 'N')) return true;
    if (pieces.length === 2 && pieces[0].type === 'B' && pieces[1].type === 'B' &&
        pieces[0].color !== pieces[1].color) {
        // Same-colored bishops
        // Find their squares
        let sq1 = null, sq2 = null;
        for (let r = 0; r < 8; r++)
            for (let c = 0; c < 8; c++) {
                const p = board[r][c];
                if (p?.type === 'B') {
                    if (!sq1) sq1 = (r + c) % 2;
                    else sq2 = (r + c) % 2;
                }
            }
        if (sq1 === sq2) return true;
    }
    return false;
}

// ── SAN Notation ─────────────────────────────────────────────────────────

const FILES = 'abcdefgh';

function squareToAlg(r, c) {
    return FILES[c] + (8 - r);
}

export function moveToSAN(state, move) {
    const [fr, fc] = move.from;
    const [tr, tc] = move.to;
    const piece = state.board[fr][fc];
    const captured = state.board[tr][tc];

    // Castling
    if (move.castle === 'K') return 'O-O';
    if (move.castle === 'Q') return 'O-O-O';

    let san = '';

    if (piece.type === 'P') {
        // Pawn
        const isCapture = captured || (fc !== tc);
        if (isCapture) san += FILES[fc] + 'x';
        san += squareToAlg(tr, tc);
        if (move.promotion) san += '=' + move.promotion;
    } else {
        san += piece.type;

        // Disambiguation: find other pieces of same type that can reach same square
        const allMoves = legalMoves(state);
        const ambiguous = allMoves.filter(m =>
            m.to[0] === tr && m.to[1] === tc &&
            (m.from[0] !== fr || m.from[1] !== fc) &&
            state.board[m.from[0]][m.from[1]]?.type === piece.type
        );
        if (ambiguous.length > 0) {
            const sameFile = ambiguous.some(m => m.from[1] === fc);
            const sameRank = ambiguous.some(m => m.from[0] === fr);
            if (!sameFile) san += FILES[fc];
            else if (!sameRank) san += (8 - fr);
            else san += FILES[fc] + (8 - fr);
        }

        if (captured) san += 'x';
        san += squareToAlg(tr, tc);
    }

    // Check / checkmate suffix
    const newState = makeMove(state, move);
    const status = gameStatus(newState);
    if (status === 'checkmate') san += '#';
    else if (status === 'check') san += '+';

    return san;
}

// ── Perft ────────────────────────────────────────────────────────────────

export function perft(state, depth) {
    if (depth === 0) return 1;
    const moves = legalMoves(state);
    let nodes = 0;
    for (const move of moves) {
        nodes += perft(makeMove(state, move), depth - 1);
    }
    return nodes;
}

// ── Starting State ───────────────────────────────────────────────────────

export const STARTING_FEN = 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1';
