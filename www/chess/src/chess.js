// Chess Integer Reduction — Analysis Engine
// Pure computation: attack generation, drain sums, FEN parsing, normalization.

export const DIAG = [[-1,-1],[-1,1],[1,-1],[1,1]];
export const ORTHO = [[-1,0],[1,0],[0,-1],[0,1]];
export const ALL8 = [...DIAG, ...ORTHO];
export const KNIGHT_DELTAS = [[-2,-1],[-2,1],[-1,-2],[-1,2],[1,-2],[1,2],[2,-1],[2,1]];

export const PIECE_WEIGHTS = { P: 1, N: 3, B: 3, R: 5, Q: 9, K: 1 };

export function inBounds(r, c) { return r >= 0 && r < 8 && c >= 0 && c < 8; }

function knightMoves(r, c) {
    return KNIGHT_DELTAS
        .map(([dr, dc]) => [r + dr, c + dc])
        .filter(([nr, nc]) => inBounds(nr, nc));
}

function kingMoves(r, c) {
    return ALL8
        .map(([dr, dc]) => [r + dr, c + dc])
        .filter(([nr, nc]) => inBounds(nr, nc));
}

function slidingMoves(r, c, dirs) {
    const moves = [];
    for (const [dr, dc] of dirs) {
        let nr = r + dr, nc = c + dc;
        while (inBounds(nr, nc)) {
            moves.push([nr, nc]);
            nr += dr;
            nc += dc;
        }
    }
    return moves;
}

function pawnCaptures(r, c) {
    const moves = [];
    if (r > 0) {
        if (c > 0) moves.push([r - 1, c - 1]);
        if (c < 7) moves.push([r - 1, c + 1]);
    }
    if (r < 7) {
        if (c > 0) moves.push([r + 1, c - 1]);
        if (c < 7) moves.push([r + 1, c + 1]);
    }
    return moves;
}

const ATTACK_FNS = {
    P: pawnCaptures,
    N: knightMoves,
    B: (r, c) => slidingMoves(r, c, DIAG),
    R: (r, c) => slidingMoves(r, c, ORTHO),
    Q: (r, c) => slidingMoves(r, c, ALL8),
    K: kingMoves,
};

// Sliding attacks with blocking (for Phase 3 position analysis)
function slidingAttacksBlocked(r, c, dirs, occupied) {
    const attacks = [];
    for (const [dr, dc] of dirs) {
        let nr = r + dr, nc = c + dc;
        while (inBounds(nr, nc)) {
            attacks.push([nr, nc]);
            if (occupied[nr][nc]) break;
            nr += dr;
            nc += dc;
        }
    }
    return attacks;
}

export function pieceAttacksBlocked(type, color, r, c, occupied) {
    switch (type) {
        case 'N': return knightMoves(r, c);
        case 'K': return kingMoves(r, c);
        case 'B': return slidingAttacksBlocked(r, c, DIAG, occupied);
        case 'R': return slidingAttacksBlocked(r, c, ORTHO, occupied);
        case 'Q': return slidingAttacksBlocked(r, c, ALL8, occupied);
        case 'P': {
            const moves = [];
            if (color === 'w') {
                if (r > 0 && c > 0) moves.push([r - 1, c - 1]);
                if (r > 0 && c < 7) moves.push([r - 1, c + 1]);
            } else {
                if (r < 7 && c > 0) moves.push([r + 1, c - 1]);
                if (r < 7 && c < 7) moves.push([r + 1, c + 1]);
            }
            return moves;
        }
        default: return [];
    }
}

function normalize(raw) {
    let min = Infinity, max = -Infinity;
    for (const row of raw) {
        for (const v of row) {
            if (v > 0) {
                if (v < min) min = v;
                if (v > max) max = v;
            }
        }
    }
    if (min === Infinity) return { values: raw, min: 0, max: 0, ratio: 0 };
    const values = raw.map(row => row.map(v => v > 0 ? v / min : 0));
    return { values, min: 1.0, max: max / min, ratio: max / min };
}

export function computePhase1() {
    const sums = Array.from({ length: 8 }, () => Array(8).fill(0));
    for (let r = 0; r < 8; r++) {
        for (let c = 0; c < 8; c++) {
            for (const fn of Object.values(ATTACK_FNS)) {
                sums[r][c] += fn(r, c).length;
            }
        }
    }
    return normalize(sums);
}

export function computePhase2() {
    const sums = Array.from({ length: 8 }, () => Array(8).fill(0));
    for (let r = 0; r < 8; r++) {
        for (let c = 0; c < 8; c++) {
            for (const [piece, fn] of Object.entries(ATTACK_FNS)) {
                sums[r][c] += fn(r, c).length * PIECE_WEIGHTS[piece];
            }
        }
    }
    return normalize(sums);
}

export function parseFEN(fen) {
    const ranks = fen.trim().split(/\s+/)[0].split('/');
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
    return board;
}

export function computePhase3(fen) {
    const board = parseFEN(fen);
    const occupied = board.map(row => row.map(cell => cell !== null));
    const sums = Array.from({ length: 8 }, () => Array(8).fill(0));
    for (let r = 0; r < 8; r++) {
        for (let c = 0; c < 8; c++) {
            const piece = board[r][c];
            if (piece) {
                for (const [tr, tc] of pieceAttacksBlocked(piece.type, piece.color, r, c, occupied)) {
                    sums[tr][tc] += PIECE_WEIGHTS[piece.type];
                }
            }
        }
    }
    const result = normalize(sums);
    result.board = board;
    return result;
}

export function computePhase3Sided(fen) {
    const board = parseFEN(fen);
    const occupied = board.map(row => row.map(cell => cell !== null));
    const white = Array.from({ length: 8 }, () => Array(8).fill(0));
    const black = Array.from({ length: 8 }, () => Array(8).fill(0));
    for (let r = 0; r < 8; r++) {
        for (let c = 0; c < 8; c++) {
            const piece = board[r][c];
            if (piece) {
                const grid = piece.color === 'w' ? white : black;
                for (const [tr, tc] of pieceAttacksBlocked(piece.type, piece.color, r, c, occupied)) {
                    grid[tr][tc] += PIECE_WEIGHTS[piece.type];
                }
            }
        }
    }
    return { white, black };
}

export const PRESETS = [
    {
        name: 'Starting Position',
        fen: 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1',
    },
    {
        name: 'Italian Game',
        fen: 'r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3',
    },
    {
        name: 'Sicilian Najdorf',
        fen: 'rnbqkb1r/1p2pppp/p2p1n2/8/3NP3/2N5/PPP2PPP/R1BQKB1R w KQkq - 0 6',
    },
    {
        name: 'After 1.e4',
        fen: 'rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1',
    },
];
