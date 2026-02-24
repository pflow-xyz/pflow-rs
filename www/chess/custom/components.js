// Chess Integer Reduction — Custom Elements
// Shadow DOM components following the petri-pilot pattern.

import { computePhase1, computePhase2, computePhase3, PRESETS, PIECE_WEIGHTS } from '../src/chess.js';
import { parseFENFull, toFEN, legalMoves, makeMove, gameStatus, moveToSAN, perft, STARTING_FEN } from '../src/engine.js';
import { findBestMove } from '../src/ai.js';
import { StockfishBridge, DIFFICULTY } from '../src/stockfish-bridge.js';

// ---------------------------------------------------------------------------
// Base class
// ---------------------------------------------------------------------------

class ChessComponent extends HTMLElement {
    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
    }

    emit(name, detail = {}) {
        this.dispatchEvent(new CustomEvent(name, {
            detail,
            bubbles: true,
            composed: true,
        }));
    }

    $(sel) { return this.shadowRoot.querySelector(sel); }
    $$(sel) { return this.shadowRoot.querySelectorAll(sel); }
}

// ---------------------------------------------------------------------------
// <chess-board> — 8x8 heatmap board (analysis mode)
// ---------------------------------------------------------------------------

const PIECE_UNICODE = {
    K: { w: '\u2654', b: '\u265A' },
    Q: { w: '\u2655', b: '\u265B' },
    R: { w: '\u2656', b: '\u265C' },
    B: { w: '\u2657', b: '\u265D' },
    N: { w: '\u2658', b: '\u265E' },
    P: { w: '\u2659', b: '\u265F' },
};

class ChessBoard extends ChessComponent {
    connectedCallback() {
        this._data = null;
        this.render();
    }

    render() {
        const files = 'abcdefgh';
        this.shadowRoot.innerHTML = `
<style>
:host { display: block; }
.wrap { display: inline-block; }
.board {
    display: grid;
    grid-template-columns: 28px repeat(8, 1fr);
    grid-template-rows: repeat(8, 1fr) 24px;
    width: 520px; height: 520px;
    border: 2px solid #555;
    border-radius: 4px;
    overflow: hidden;
}
.cell {
    position: relative;
    display: flex; align-items: center; justify-content: center;
    cursor: default;
    transition: filter 0.15s;
}
.cell:hover { filter: brightness(1.2); }
.overlay {
    position: absolute; inset: 0;
    display: flex; flex-direction: column;
    align-items: center; justify-content: center;
    pointer-events: none;
}
.piece { font-size: 30px; line-height: 1; filter: drop-shadow(0 1px 2px rgba(0,0,0,0.6)); }
.val {
    font-size: 11px; font-weight: 700; font-family: 'SF Mono', monospace;
    text-shadow: 0 0 4px rgba(0,0,0,0.9);
    color: #fff;
}
.val.zero { color: #555; }
.light { background: #f0d9b5; }
.dark  { background: #b58863; }
.rank-label, .file-label {
    display: flex; align-items: center; justify-content: center;
    font-size: 12px; color: #777; font-family: monospace;
}
.scale-row {
    display: flex; align-items: center; gap: 8px;
    margin-top: 10px; padding-left: 28px;
}
.scale-bar {
    height: 10px; width: 200px; border-radius: 3px;
    background: linear-gradient(to right,
        hsla(240,70%,50%,0.75),
        hsla(180,70%,50%,0.75),
        hsla(120,70%,50%,0.75),
        hsla(60,70%,50%,0.75),
        hsla(0,70%,50%,0.75));
}
.scale-label { font-size: 11px; color: #888; font-family: monospace; }
.stats {
    margin-top: 6px; padding-left: 28px;
    font-size: 12px; color: #777; font-family: monospace;
}
</style>
<div class="wrap">
  <div class="board" id="board"></div>
  <div class="scale-row">
    <span class="scale-label">Low</span>
    <div class="scale-bar"></div>
    <span class="scale-label">High</span>
  </div>
  <div class="stats" id="stats"></div>
</div>`;
        this._buildGrid(files);
    }

    _buildGrid(files) {
        const board = this.$('#board');
        let html = '';
        for (let r = 0; r < 8; r++) {
            html += `<div class="rank-label">${8 - r}</div>`;
            for (let c = 0; c < 8; c++) {
                const cls = (r + c) % 2 === 1 ? 'dark' : 'light';
                html += `<div class="cell ${cls}" id="c${r}${c}"><div class="overlay" id="o${r}${c}"></div></div>`;
            }
        }
        html += '<div></div>';
        for (let c = 0; c < 8; c++) html += `<div class="file-label">${files[c]}</div>`;
        board.innerHTML = html;
    }

    _valueColor(v) {
        if (!this._data || v === 0) return 'transparent';
        const { min, max } = this._data;
        const t = max > min ? (v - min) / (max - min) : 0;
        const hue = 240 - t * 240;
        return `hsla(${hue}, 70%, 50%, 0.55)`;
    }

    update(data) {
        this._data = data;
        const { values, board: pieces, min, max, ratio } = data;
        for (let r = 0; r < 8; r++) {
            for (let c = 0; c < 8; c++) {
                const overlay = this.$(`#o${r}${c}`);
                overlay.style.background = this._valueColor(values[r][c]);
                let inner = '';
                if (pieces && pieces[r] && pieces[r][c]) {
                    const p = pieces[r][c];
                    const sym = PIECE_UNICODE[p.type]?.[p.color] || '?';
                    inner += `<span class="piece">${sym}</span>`;
                }
                const v = values[r][c];
                inner += `<span class="val${v === 0 ? ' zero' : ''}">${v > 0 ? v.toFixed(1) : '-'}</span>`;
                overlay.innerHTML = inner;
            }
        }
        this.$('#stats').textContent = max > 0
            ? `range: ${min.toFixed(2)} \u2013 ${max.toFixed(2)}   ratio: ${ratio.toFixed(3)}`
            : '';
    }
}

// ---------------------------------------------------------------------------
// <chess-play-board> — Interactive board for playing
// ---------------------------------------------------------------------------

class ChessPlayBoard extends ChessComponent {
    connectedCallback() {
        this._selected = null;   // [r,c] of selected piece
        this._legalTargets = [];  // legal moves from selected piece
        this._lastFrom = null;
        this._lastTo = null;
        this._interactive = true;
        this._flipped = false;
        this._showHeatmap = false;
        this._heatmapData = null;
        this.render();
    }

    render() {
        this.shadowRoot.innerHTML = `
<style>
:host { display: block; }
.wrap { display: inline-block; position: relative; }
.board {
    display: grid;
    grid-template-columns: 28px repeat(8, 1fr);
    grid-template-rows: repeat(8, 1fr) 24px;
    width: 520px; height: 520px;
    border: 2px solid #555;
    border-radius: 4px;
    overflow: hidden;
}
.cell {
    position: relative;
    display: flex; align-items: center; justify-content: center;
    cursor: pointer;
    transition: filter 0.15s;
}
.cell:hover { filter: brightness(1.15); }
.overlay {
    position: absolute; inset: 0;
    display: flex; flex-direction: column;
    align-items: center; justify-content: center;
    pointer-events: none;
}
.piece { font-size: 36px; line-height: 1; filter: drop-shadow(0 1px 2px rgba(0,0,0,0.6)); }
.light { background: #f0d9b5; }
.dark  { background: #b58863; }
.selected { box-shadow: inset 0 0 0 3px #4a90d9; }
.legal-dot::after {
    content: '';
    position: absolute;
    width: 14px; height: 14px;
    background: rgba(0,0,0,0.25);
    border-radius: 50%;
    pointer-events: none;
}
.legal-capture::after {
    content: '';
    position: absolute;
    width: 44px; height: 44px;
    border: 4px solid rgba(0,0,0,0.25);
    border-radius: 50%;
    pointer-events: none;
}
.last-from { background: #cdd26a !important; }
.last-to { background: #f6f669 !important; }
.in-check { box-shadow: inset 0 0 0 3px #e74c3c; }
.rank-label, .file-label {
    display: flex; align-items: center; justify-content: center;
    font-size: 12px; color: #777; font-family: monospace;
}
.promo-overlay {
    position: absolute; z-index: 10;
    display: flex; flex-direction: column;
    background: #2a2a3e; border: 2px solid #667eea;
    border-radius: 6px; box-shadow: 0 4px 16px rgba(0,0,0,0.6);
}
.promo-btn {
    padding: 4px 8px; font-size: 32px; cursor: pointer;
    background: transparent; border: none; color: #e0e0e0;
    transition: background 0.12s;
}
.promo-btn:hover { background: #454578; }
.hidden { display: none; }
</style>
<div class="wrap">
  <div class="board" id="board"></div>
  <div class="promo-overlay hidden" id="promo"></div>
</div>`;
        this._buildGrid();
    }

    _buildGrid() {
        const board = this.$('#board');
        let html = '';
        const files = 'abcdefgh';
        for (let vr = 0; vr < 8; vr++) {
            const r = this._flipped ? (7 - vr) : vr;
            html += `<div class="rank-label">${8 - r}</div>`;
            for (let vc = 0; vc < 8; vc++) {
                const c = this._flipped ? (7 - vc) : vc;
                const cls = (r + c) % 2 === 1 ? 'dark' : 'light';
                html += `<div class="cell ${cls}" data-r="${r}" data-c="${c}" id="c${r}${c}"><div class="overlay" id="o${r}${c}"></div></div>`;
            }
        }
        html += '<div></div>';
        for (let vc = 0; vc < 8; vc++) {
            const c = this._flipped ? (7 - vc) : vc;
            html += `<div class="file-label">${files[c]}</div>`;
        }
        board.innerHTML = html;

        board.addEventListener('click', (e) => {
            const cell = e.target.closest('.cell');
            if (!cell) return;
            const r = parseInt(cell.dataset.r);
            const c = parseInt(cell.dataset.c);
            this.emit('cell-click', { r, c });
        });
    }

    set flipped(v) {
        if (this._flipped !== v) {
            this._flipped = v;
            this._buildGrid();
        }
    }

    set interactive(v) { this._interactive = v; }

    updatePosition(board, opts = {}) {
        const { selected, legalTargets, lastFrom, lastTo, checkSquare, heatmapData } = opts;
        this._selected = selected || null;
        this._legalTargets = legalTargets || [];
        this._lastFrom = lastFrom || null;
        this._lastTo = lastTo || null;
        this._heatmapData = heatmapData || null;

        for (let r = 0; r < 8; r++) {
            for (let c = 0; c < 8; c++) {
                const cell = this.$(`#c${r}${c}`);
                const overlay = this.$(`#o${r}${c}`);
                if (!cell || !overlay) continue;

                // Reset classes
                cell.classList.remove('selected', 'legal-dot', 'legal-capture', 'last-from', 'last-to', 'in-check');

                // Restore base color
                const baseCls = (r + c) % 2 === 1 ? 'dark' : 'light';
                cell.className = `cell ${baseCls}`;

                // Last move highlight
                if (this._lastFrom && this._lastFrom[0] === r && this._lastFrom[1] === c) {
                    cell.classList.add('last-from');
                }
                if (this._lastTo && this._lastTo[0] === r && this._lastTo[1] === c) {
                    cell.classList.add('last-to');
                }

                // Selected piece
                if (this._selected && this._selected[0] === r && this._selected[1] === c) {
                    cell.classList.add('selected');
                }

                // Legal move targets
                const isLegalTarget = this._legalTargets.some(m => m.to[0] === r && m.to[1] === c);
                if (isLegalTarget) {
                    cell.classList.add(board[r][c] ? 'legal-capture' : 'legal-dot');
                }

                // Check highlight
                if (checkSquare && checkSquare[0] === r && checkSquare[1] === c) {
                    cell.classList.add('in-check');
                }

                // Heatmap background
                let heatBg = '';
                if (this._heatmapData) {
                    const v = this._heatmapData[r][c];
                    if (v > 0) {
                        const maxV = Math.max(...this._heatmapData.flat());
                        const t = maxV > 0 ? v / maxV : 0;
                        const hue = 240 - t * 240;
                        heatBg = `hsla(${hue}, 70%, 50%, 0.25)`;
                    }
                }
                overlay.style.background = heatBg;

                // Piece
                let inner = '';
                const p = board[r]?.[c];
                if (p) {
                    const sym = PIECE_UNICODE[p.type]?.[p.color] || '?';
                    inner = `<span class="piece">${sym}</span>`;
                }
                overlay.innerHTML = inner;
            }
        }
    }

    showPromotion(r, c, color, callback) {
        const promo = this.$('#promo');
        const pieces = ['Q', 'R', 'B', 'N'];
        // Position near the promotion square
        const cell = this.$(`#c${r}${c}`);
        if (!cell) return;
        const rect = cell.getBoundingClientRect();
        const boardRect = this.$('#board').getBoundingClientRect();
        const left = cell.offsetLeft;
        const top = color === 'w' ? cell.offsetTop : cell.offsetTop - 160;

        promo.style.left = left + 'px';
        promo.style.top = Math.max(0, top) + 'px';
        promo.classList.remove('hidden');

        promo.innerHTML = pieces.map(p => {
            const sym = PIECE_UNICODE[p][color];
            return `<button class="promo-btn" data-piece="${p}">${sym}</button>`;
        }).join('');

        const handler = (e) => {
            const btn = e.target.closest('.promo-btn');
            if (!btn) return;
            promo.classList.add('hidden');
            promo.removeEventListener('click', handler);
            callback(btn.dataset.piece);
        };
        promo.addEventListener('click', handler);
    }

    hidePromotion() {
        this.$('#promo')?.classList.add('hidden');
    }
}

// ---------------------------------------------------------------------------
// <chess-controls> — phase tabs, FEN input, presets (analysis mode)
// ---------------------------------------------------------------------------

class ChessControls extends ChessComponent {
    connectedCallback() {
        this._phase = 1;
        this.render();
        this._setup();
    }

    render() {
        this.shadowRoot.innerHTML = `
<style>
:host { display: block; margin-bottom: 16px; }
.tabs { display: flex; gap: 4px; margin-bottom: 10px; }
.tab {
    padding: 8px 16px; border: 1px solid #444; background: #252538;
    color: #999; border-radius: 6px 6px 0 0; cursor: pointer;
    font-size: 13px; transition: all 0.15s; user-select: none;
}
.tab:hover { background: #353558; color: #ccc; }
.tab.active { background: #454578; color: #fff; border-bottom-color: #454578; }
.desc { font-size: 13px; color: #888; margin-bottom: 10px; }
.fen-section { display: none; }
.fen-section.show { display: block; }
.fen-input {
    width: 100%; padding: 8px 10px; background: #252538; border: 1px solid #444;
    color: #eee; border-radius: 4px; font-family: 'SF Mono', monospace; font-size: 12px;
    box-sizing: border-box;
}
.fen-input:focus { outline: none; border-color: #667eea; }
.presets { display: flex; gap: 6px; margin-top: 8px; flex-wrap: wrap; }
.preset {
    padding: 5px 12px; background: #252538; border: 1px solid #555;
    color: #aaa; border-radius: 4px; cursor: pointer; font-size: 12px;
    transition: all 0.12s;
}
.preset:hover { background: #353558; color: #fff; }
.weights {
    font-size: 12px; color: #666; margin-top: 6px; font-family: monospace;
}
</style>
<div class="tabs" id="tabs">
  <div class="tab active" data-phase="1">Phase 1: Unweighted</div>
  <div class="tab" data-phase="2">Phase 2: Weighted</div>
  <div class="tab" data-phase="3">Phase 3: Position</div>
</div>
<div class="desc" id="desc">Each piece type contributes 1 drain per reachable square on an empty board.</div>
<div class="fen-section" id="fen">
  <input class="fen-input" id="fen-input"
    value="rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    placeholder="Enter FEN string..." />
  <div class="presets" id="presets"></div>
</div>
<div class="weights" id="weights"></div>`;
    }

    _setup() {
        const descs = {
            1: 'Each piece type contributes 1 drain per reachable square on an empty board.',
            2: 'Drain weights: P=1, N=3, B=3, R=5, Q=9, K=1 (standard piece values).',
            3: 'Weighted attack pressure per square from actual piece positions.',
        };

        this.$$('.tab').forEach(tab => {
            tab.addEventListener('click', () => {
                this._phase = parseInt(tab.dataset.phase);
                this.$$('.tab').forEach(t => t.classList.remove('active'));
                tab.classList.add('active');
                this.$('#fen').classList.toggle('show', this._phase === 3);
                this.$('#desc').textContent = descs[this._phase];
                this.$('#weights').textContent = this._phase === 2
                    ? Object.entries(PIECE_WEIGHTS).map(([p, w]) => `${p}=${w}`).join('  ')
                    : '';
                this.emit('phase-change', { phase: this._phase });
            });
        });

        let timer;
        this.$('#fen-input').addEventListener('input', (e) => {
            clearTimeout(timer);
            timer = setTimeout(() => this.emit('fen-change', { fen: e.target.value }), 250);
        });

        const presets = this.$('#presets');
        presets.innerHTML = PRESETS.map((p, i) =>
            `<button class="preset" data-idx="${i}">${p.name}</button>`
        ).join('');
        presets.querySelectorAll('.preset').forEach(btn => {
            btn.addEventListener('click', () => {
                const p = PRESETS[parseInt(btn.dataset.idx)];
                this.$('#fen-input').value = p.fen;
                this.emit('fen-change', { fen: p.fen });
            });
        });
    }

    get phase() { return this._phase; }
    get fen() { return this.$('#fen-input')?.value || ''; }

    setFen(fen) {
        const input = this.$('#fen-input');
        if (input) input.value = fen;
    }
}

// ---------------------------------------------------------------------------
// <chess-game-controls> — game controls for play mode
// ---------------------------------------------------------------------------

class ChessGameControls extends ChessComponent {
    connectedCallback() {
        this.render();
        this._setup();
    }

    render() {
        this.shadowRoot.innerHTML = `
<style>
:host { display: block; margin-bottom: 16px; }
.row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 8px; }
.btn {
    padding: 6px 14px; background: #252538; border: 1px solid #555;
    color: #ccc; border-radius: 4px; cursor: pointer; font-size: 13px;
    transition: all 0.12s;
}
.btn:hover { background: #353558; color: #fff; }
.btn:disabled { opacity: 0.4; cursor: default; }
.btn.active { background: #454578; border-color: #667eea; color: #fff; }
label { font-size: 13px; color: #999; }
select {
    padding: 4px 8px; background: #252538; border: 1px solid #555;
    color: #ccc; border-radius: 4px; font-size: 13px;
}
.status {
    font-size: 14px; color: #e0e0e0; font-weight: 600;
    min-height: 22px;
}
.status.check { color: #e67e22; }
.status.end { color: #e74c3c; }
.move-list {
    max-height: 160px; overflow-y: auto;
    font-size: 12px; color: #aaa; font-family: 'SF Mono', monospace;
    padding: 6px 8px; background: #1e1e30; border-radius: 4px;
    border: 1px solid #333;
    line-height: 1.6;
}
.move-list .move-num { color: #666; }
.move-list .white-move { color: #ddd; }
.move-list .black-move { color: #aaa; }
.toggle-row { display: flex; gap: 12px; align-items: center; }
.toggle-label {
    font-size: 12px; color: #888; cursor: pointer;
    display: flex; align-items: center; gap: 4px;
}
.toggle-label input { cursor: pointer; }
</style>
<div class="status" id="status">White to move</div>
<div class="row">
  <button class="btn" id="new-game">New Game</button>
  <button class="btn" id="undo" disabled>Undo</button>
  <label>Engine:
    <select id="engine">
      <option value="pressure-2">Phase 3 Pressure (Easy)</option>
      <option value="pressure-3">Phase 3 Pressure (Medium)</option>
      <option value="stockfish-easy">Stockfish (Easy)</option>
      <option value="stockfish-medium" selected>Stockfish (Medium)</option>
      <option value="stockfish-hard">Stockfish (Hard)</option>
    </select>
  </label>
</div>
<div class="row">
  <label>Play as:</label>
  <button class="btn active" id="play-white" data-color="w">White</button>
  <button class="btn" id="play-black" data-color="b">Black</button>
</div>
<div class="toggle-row">
  <label class="toggle-label"><input type="checkbox" id="show-heatmap"> Show pressure heatmap</label>
  <label class="toggle-label"><input type="checkbox" id="flip-board"> Flip board</label>
</div>
<div class="move-list" id="moves"></div>`;
    }

    _setup() {
        this.$('#new-game').addEventListener('click', () => this.emit('new-game'));
        this.$('#undo').addEventListener('click', () => this.emit('undo'));
        this.$('#engine').addEventListener('change', (e) => {
            const val = e.target.value;
            const [type, param] = val.split('-');
            this.emit('engine-change', { engine: type, param });
        });
        this.$('#show-heatmap').addEventListener('change', (e) =>
            this.emit('toggle-heatmap', { on: e.target.checked }));
        this.$('#flip-board').addEventListener('change', (e) =>
            this.emit('toggle-flip', { on: e.target.checked }));

        for (const btn of [this.$('#play-white'), this.$('#play-black')]) {
            btn.addEventListener('click', () => {
                this.$('#play-white').classList.toggle('active', btn.dataset.color === 'w');
                this.$('#play-black').classList.toggle('active', btn.dataset.color === 'b');
                this.emit('play-as', { color: btn.dataset.color });
            });
        }
    }

    setStatus(text, type = '') {
        const el = this.$('#status');
        el.textContent = text;
        el.className = 'status' + (type ? ` ${type}` : '');
    }

    setUndoEnabled(v) {
        this.$('#undo').disabled = !v;
    }

    updateMoveList(sanList) {
        const el = this.$('#moves');
        let html = '';
        for (let i = 0; i < sanList.length; i += 2) {
            const num = Math.floor(i / 2) + 1;
            html += `<span class="move-num">${num}.</span> `;
            html += `<span class="white-move">${sanList[i]}</span> `;
            if (i + 1 < sanList.length) {
                html += `<span class="black-move">${sanList[i + 1]}</span> `;
            }
        }
        el.innerHTML = html;
        el.scrollTop = el.scrollHeight;
    }
}

// ---------------------------------------------------------------------------
// <chess-app> — orchestrator with Analysis / Play tabs
// ---------------------------------------------------------------------------

class ChessApp extends ChessComponent {
    connectedCallback() {
        this._mode = 'analysis';
        this._gameState = null;
        this._history = [];      // array of {state, san}
        this._sanList = [];
        this._humanColor = 'w';
        this._aiThinking = false;
        this._selected = null;
        this._showHeatmap = false;
        this._lastFrom = null;
        this._lastTo = null;
        // Engine state
        this._engineType = 'stockfish'; // 'pressure' | 'stockfish'
        this._engineParam = 'medium';   // pressure: '2'|'3', stockfish: 'easy'|'medium'|'hard'
        this._stockfish = null;
        this.render();
        this._init();
    }

    render() {
        this.shadowRoot.innerHTML = `
<style>
:host { display: block; }
h1 { margin: 0 0 4px; font-size: 22px; font-weight: 600; color: #e0e0e0; }
.subtitle { font-size: 13px; color: #777; margin-bottom: 16px; }
a { color: #667eea; }
.mode-tabs { display: flex; gap: 4px; margin-bottom: 16px; }
.mode-tab {
    padding: 10px 24px; border: 1px solid #444; background: #1e1e30;
    color: #999; border-radius: 8px 8px 0 0; cursor: pointer;
    font-size: 14px; font-weight: 600; transition: all 0.15s; user-select: none;
}
.mode-tab:hover { background: #2a2a44; color: #ccc; }
.mode-tab.active { background: #2a2a44; color: #fff; border-bottom-color: #2a2a44; }
.panel { display: none; }
.panel.active { display: block; }
</style>
<h1>Chess Integer Reduction</h1>
<div class="subtitle">
  Petri net drain-count analysis recovers strategic square values from pure game topology.
</div>
<div class="mode-tabs">
  <div class="mode-tab active" data-mode="analysis">Analysis</div>
  <div class="mode-tab" data-mode="play">Play</div>
</div>
<div class="panel active" id="analysis-panel">
  <chess-controls id="controls"></chess-controls>
  <chess-board id="board"></chess-board>
</div>
<div class="panel" id="play-panel">
  <chess-game-controls id="game-controls"></chess-game-controls>
  <chess-play-board id="play-board"></chess-play-board>
</div>`;
    }

    _init() {
        // Mode tabs
        this.$$('.mode-tab').forEach(tab => {
            tab.addEventListener('click', () => {
                this._mode = tab.dataset.mode;
                this.$$('.mode-tab').forEach(t => t.classList.toggle('active', t === tab));
                this.$('#analysis-panel').classList.toggle('active', this._mode === 'analysis');
                this.$('#play-panel').classList.toggle('active', this._mode === 'play');
                if (this._mode === 'play' && !this._gameState) this._newGame();
            });
        });

        // Analysis mode
        const controls = this.$('#controls');
        const board = this.$('#board');

        const refreshAnalysis = () => {
            const phase = controls.phase;
            try {
                if (phase === 1) board.update(computePhase1());
                else if (phase === 2) board.update(computePhase2());
                else board.update(computePhase3(controls.fen));
            } catch (e) {
                console.error('Analysis error:', e);
            }
        };

        controls.addEventListener('phase-change', refreshAnalysis);
        controls.addEventListener('fen-change', refreshAnalysis);
        refreshAnalysis();

        // Play mode
        const gc = this.$('#game-controls');
        const pb = this.$('#play-board');

        gc.addEventListener('new-game', () => this._newGame());
        gc.addEventListener('undo', () => this._undo());
        gc.addEventListener('engine-change', (e) => {
            this._engineType = e.detail.engine;
            this._engineParam = e.detail.param;
        });
        gc.addEventListener('play-as', (e) => {
            this._humanColor = e.detail.color;
            this._newGame();
        });
        gc.addEventListener('toggle-heatmap', (e) => {
            this._showHeatmap = e.detail.on;
            this._refreshPlayBoard();
        });
        gc.addEventListener('toggle-flip', (e) => {
            pb.flipped = e.detail.on;
            this._refreshPlayBoard();
        });

        pb.addEventListener('cell-click', (e) => this._onCellClick(e.detail.r, e.detail.c));

        // Expose perft on window for console testing
        window.perft = (depth) => {
            const state = parseFENFull(STARTING_FEN);
            return perft(state, depth);
        };
    }

    _newGame() {
        this._stockfish?.stop();
        this._gameState = parseFENFull(STARTING_FEN);
        this._history = [];
        this._sanList = [];
        this._selected = null;
        this._lastFrom = null;
        this._lastTo = null;
        this._aiThinking = false;

        const gc = this.$('#game-controls');
        gc.setStatus('White to move');
        gc.setUndoEnabled(false);
        gc.updateMoveList([]);

        const pb = this.$('#play-board');
        pb.flipped = this._humanColor === 'b';
        // Update the flip checkbox to match
        const flipCheck = gc.$('#flip-board');
        if (flipCheck) flipCheck.checked = this._humanColor === 'b';

        this._refreshPlayBoard();

        // If human is black, AI moves first
        if (this._humanColor === 'b') {
            setTimeout(() => this._aiMove(), 50);
        }
    }

    _refreshPlayBoard() {
        if (!this._gameState) return;
        const pb = this.$('#play-board');
        const status = gameStatus(this._gameState);

        // Compute legal targets for selected piece
        let legalTargets = [];
        if (this._selected) {
            const allLegal = legalMoves(this._gameState);
            legalTargets = allLegal.filter(m =>
                m.from[0] === this._selected[0] && m.from[1] === this._selected[1]);
        }

        // Check square
        let checkSquare = null;
        if (status === 'check' || status === 'checkmate') {
            for (let r = 0; r < 8; r++)
                for (let c = 0; c < 8; c++) {
                    const p = this._gameState.board[r][c];
                    if (p?.type === 'K' && p.color === this._gameState.turn)
                        checkSquare = [r, c];
                }
        }

        // Heatmap data
        let heatmapData = null;
        if (this._showHeatmap) {
            const { values } = computePhase3(toFEN(this._gameState));
            heatmapData = values;
        }

        pb.updatePosition(this._gameState.board, {
            selected: this._selected,
            legalTargets,
            lastFrom: this._lastFrom,
            lastTo: this._lastTo,
            checkSquare,
            heatmapData,
        });
    }

    _onCellClick(r, c) {
        if (this._aiThinking) return;
        if (!this._gameState) return;

        const status = gameStatus(this._gameState);
        if (status === 'checkmate' || status === 'stalemate' ||
            status === 'draw-50' || status === 'draw-material') return;

        // Not human's turn
        if (this._gameState.turn !== this._humanColor) return;

        const piece = this._gameState.board[r][c];

        if (this._selected) {
            // Check if clicking on a legal target
            const allLegal = legalMoves(this._gameState);
            const movesFromSelected = allLegal.filter(m =>
                m.from[0] === this._selected[0] && m.from[1] === this._selected[1] &&
                m.to[0] === r && m.to[1] === c);

            if (movesFromSelected.length > 0) {
                if (movesFromSelected.length > 1 && movesFromSelected[0].promotion) {
                    // Promotion — show picker
                    const pb = this.$('#play-board');
                    pb.showPromotion(r, c, this._humanColor, (promoPiece) => {
                        const move = movesFromSelected.find(m => m.promotion === promoPiece);
                        if (move) this._doMove(move);
                    });
                    return;
                }
                this._doMove(movesFromSelected[0]);
                return;
            }

            // Clicking on another friendly piece — reselect
            if (piece && piece.color === this._humanColor) {
                this._selected = [r, c];
                this._refreshPlayBoard();
                return;
            }

            // Click elsewhere — deselect
            this._selected = null;
            this._refreshPlayBoard();
            return;
        }

        // No selection yet — select friendly piece
        if (piece && piece.color === this._humanColor) {
            this._selected = [r, c];
            this._refreshPlayBoard();
        }
    }

    _doMove(move) {
        const san = moveToSAN(this._gameState, move);
        this._history.push({ state: this._gameState, san });
        this._sanList.push(san);
        this._gameState = makeMove(this._gameState, move);
        this._selected = null;
        this._lastFrom = move.from;
        this._lastTo = move.to;

        const gc = this.$('#game-controls');
        gc.updateMoveList(this._sanList);
        gc.setUndoEnabled(this._history.length > 0);

        this._updateStatus();
        this._refreshPlayBoard();

        // After human move, trigger AI
        const status = gameStatus(this._gameState);
        if (status === 'playing' || status === 'check') {
            if (this._gameState.turn !== this._humanColor) {
                setTimeout(() => this._aiMove(), 50);
            }
        }
    }

    _aiMove() {
        if (this._aiThinking) return;
        this._aiThinking = true;
        const gc = this.$('#game-controls');

        if (this._engineType === 'stockfish') {
            this._aiMoveStockfish(gc);
        } else {
            this._aiMovePressure(gc);
        }
    }

    _aiMovePressure(gc) {
        const depth = parseInt(this._engineParam) || 3;
        gc.setStatus('AI thinking...', '');

        setTimeout(() => {
            const result = findBestMove(this._gameState, depth);
            this._aiThinking = false;

            if (!result) {
                this._updateStatus();
                return;
            }
            this._applyAIMove(result.move);
        }, 20);
    }

    async _aiMoveStockfish(gc) {
        try {
            if (!this._stockfish) {
                gc.setStatus('Loading Stockfish...', '');
                this._stockfish = new StockfishBridge();
                await this._stockfish.init();
            }
            gc.setStatus('Stockfish thinking...', '');

            const preset = DIFFICULTY[this._engineParam] || DIFFICULTY.medium;
            const fen = toFEN(this._gameState);
            const result = await this._stockfish.findMove(fen, preset);

            // Guard: game may have been reset while we awaited
            if (!this._aiThinking) return;
            this._aiThinking = false;

            if (!result) {
                this._updateStatus();
                return;
            }

            // Match the UCI result to a legal move
            const move = this._matchUCIMove(result);
            if (!move) {
                console.error('Stockfish returned illegal move', result);
                this._updateStatus();
                return;
            }
            this._applyAIMove(move);
        } catch (err) {
            console.error('Stockfish error:', err);
            this._aiThinking = false;
            this._stockfish?.destroy();
            this._stockfish = null;
            // Fallback to pressure engine
            gc.setStatus('Stockfish failed — falling back to Phase 3', '');
            this._engineType = 'pressure';
            this._engineParam = '3';
            setTimeout(() => this._aiMove(), 100);
        }
    }

    _matchUCIMove(uci) {
        const legal = legalMoves(this._gameState);
        for (const m of legal) {
            if (m.from[0] === uci.from[0] && m.from[1] === uci.from[1] &&
                m.to[0] === uci.to[0] && m.to[1] === uci.to[1]) {
                // Check promotion match
                if (uci.promotion) {
                    if (m.promotion === uci.promotion) return m;
                } else if (!m.promotion) {
                    return m;
                }
            }
        }
        return null;
    }

    _applyAIMove(move) {
        const gc = this.$('#game-controls');
        const san = moveToSAN(this._gameState, move);
        this._history.push({ state: this._gameState, san });
        this._sanList.push(san);
        this._gameState = makeMove(this._gameState, move);
        this._lastFrom = move.from;
        this._lastTo = move.to;

        gc.updateMoveList(this._sanList);
        gc.setUndoEnabled(this._history.length > 0);

        this._updateStatus();
        this._refreshPlayBoard();
    }

    _undo() {
        // Undo 2 plies (human + AI)
        if (this._history.length < 2) return;
        this._history.pop();
        this._sanList.pop();
        this._history.pop();
        this._sanList.pop();

        const prev = this._history.length > 0
            ? this._history[this._history.length - 1]
            : null;

        if (prev) {
            // Replay the last move in history to get current state
            this._gameState = makeMove(prev.state,
                this._findMoveForSAN(prev.state, prev.san));
        } else {
            this._gameState = parseFENFull(STARTING_FEN);
        }

        // Actually we need the state AFTER the last history entry.
        // History stores {state: before, san}. After undo we want state after last entry.
        // Simpler: rebuild from the SAN list.
        this._rebuildFromSANList();

        this._selected = null;
        this._lastFrom = null;
        this._lastTo = null;

        const gc = this.$('#game-controls');
        gc.updateMoveList(this._sanList);
        gc.setUndoEnabled(this._history.length > 0);
        this._updateStatus();
        this._refreshPlayBoard();
    }

    _rebuildFromSANList() {
        let state = parseFENFull(STARTING_FEN);
        const newHistory = [];
        for (const san of this._sanList) {
            const move = this._findMoveForSAN(state, san);
            if (!move) break;
            newHistory.push({ state, san });
            state = makeMove(state, move);
        }
        this._history = newHistory;
        this._gameState = state;
    }

    _findMoveForSAN(state, targetSAN) {
        const moves = legalMoves(state);
        for (const move of moves) {
            if (moveToSAN(state, move) === targetSAN) return move;
        }
        return null;
    }

    _updateStatus() {
        const gc = this.$('#game-controls');
        const status = gameStatus(this._gameState);
        const turnName = this._gameState.turn === 'w' ? 'White' : 'Black';
        const enemyName = this._gameState.turn === 'w' ? 'Black' : 'White';

        switch (status) {
            case 'playing':
                gc.setStatus(`${turnName} to move`);
                break;
            case 'check':
                gc.setStatus(`${turnName} to move \u2014 Check!`, 'check');
                break;
            case 'checkmate':
                gc.setStatus(`Checkmate \u2014 ${enemyName} wins!`, 'end');
                break;
            case 'stalemate':
                gc.setStatus('Stalemate \u2014 Draw', 'end');
                break;
            case 'draw-50':
                gc.setStatus('Draw \u2014 50-move rule', 'end');
                break;
            case 'draw-material':
                gc.setStatus('Draw \u2014 Insufficient material', 'end');
                break;
        }
    }
}

// ---------------------------------------------------------------------------
// Register
// ---------------------------------------------------------------------------

customElements.define('chess-board', ChessBoard);
customElements.define('chess-play-board', ChessPlayBoard);
customElements.define('chess-controls', ChessControls);
customElements.define('chess-game-controls', ChessGameControls);
customElements.define('chess-app', ChessApp);
