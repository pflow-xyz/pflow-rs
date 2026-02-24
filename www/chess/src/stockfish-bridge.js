// Stockfish WASM bridge — Web Worker wrapper with UCI protocol
// Uses stockfish.js 10.0.2 (asm.js single-file, ~2MB, no NNUE dependency)

const STOCKFISH_CDN = 'https://cdn.jsdelivr.net/npm/stockfish.js@10.0.2/stockfish.js';

const DIFFICULTY = {
    easy:   { skill: 3,  depth: 5  },
    medium: { skill: 10, depth: 10 },
    hard:   { skill: 20, depth: 15 },
};

export class StockfishBridge {
    constructor() {
        this._worker = null;
        this._ready = false;
        this._pending = null; // { resolve, reject } for current findMove
    }

    async init() {
        if (this._ready) return;

        // Create a blob-URL worker that importScripts the CDN build
        const blob = new Blob(
            [`importScripts("${STOCKFISH_CDN}");`],
            { type: 'application/javascript' }
        );
        this._worker = new Worker(URL.createObjectURL(blob));

        return new Promise((resolve, reject) => {
            const timeout = setTimeout(() => reject(new Error('Stockfish init timeout')), 15000);
            this._worker.onmessage = (e) => {
                const line = typeof e.data === 'string' ? e.data : '';
                if (line === 'uciok') {
                    clearTimeout(timeout);
                    this._ready = true;
                    // Install the permanent message handler
                    this._worker.onmessage = (ev) => this._onMessage(ev);
                    resolve();
                }
            };
            this._worker.onerror = (e) => {
                clearTimeout(timeout);
                reject(new Error('Stockfish worker error: ' + e.message));
            };
            this._worker.postMessage('uci');
        });
    }

    setSkillLevel(level) {
        if (!this._worker) return;
        this._worker.postMessage(`setoption name Skill Level value ${level}`);
    }

    async findMove(fen, { depth = 10, skill = 10 } = {}) {
        if (!this._ready) await this.init();

        this.setSkillLevel(skill);
        this._worker.postMessage('ucinewgame');
        this._worker.postMessage(`position fen ${fen}`);
        this._worker.postMessage(`go depth ${depth}`);

        return new Promise((resolve, reject) => {
            const timeout = setTimeout(() => {
                this._pending = null;
                reject(new Error('Stockfish search timeout'));
            }, 30000);
            this._pending = { resolve, reject, timeout, fen };
        });
    }

    _onMessage(e) {
        const line = typeof e.data === 'string' ? e.data : '';
        if (!this._pending) return;

        if (line.startsWith('bestmove')) {
            const { resolve, timeout } = this._pending;
            clearTimeout(timeout);
            this._pending = null;
            const uci = line.split(/\s+/)[1];
            if (!uci || uci === '(none)') {
                resolve(null);
            } else {
                resolve(parseUCIMove(uci));
            }
        }
    }

    stop() {
        if (this._worker) this._worker.postMessage('stop');
    }

    destroy() {
        if (this._worker) {
            this._worker.terminate();
            this._worker = null;
            this._ready = false;
        }
    }
}

// Convert UCI move string (e.g. "e2e4", "e7e8q") to {from:[r,c], to:[r,c], promotion?}
function parseUCIMove(uci) {
    const files = 'abcdefgh';
    const fromC = files.indexOf(uci[0]);
    const fromR = 8 - parseInt(uci[1]);
    const toC = files.indexOf(uci[2]);
    const toR = 8 - parseInt(uci[3]);

    const result = { from: [fromR, fromC], to: [toR, toC] };

    // Promotion suffix (e.g. "q" in "e7e8q")
    if (uci.length === 5) {
        result.promotion = uci[4].toUpperCase();
    }

    // Detect castling: king moves 2 squares horizontally
    if (fromR === toR && Math.abs(toC - fromC) === 2) {
        // Could be castling — caller checks if the piece is a king
        result.maybeCastle = toC > fromC ? 'K' : 'Q';
    }

    return result;
}

export { DIFFICULTY };
