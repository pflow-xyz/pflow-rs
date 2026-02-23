// Chess Integer Reduction — Custom Elements
// Shadow DOM components following the petri-pilot pattern.

import { computePhase1, computePhase2, computePhase3, PRESETS, PIECE_WEIGHTS } from '../src/chess.js';

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
// <chess-board> — 8x8 heatmap board
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
// <chess-controls> — phase tabs, FEN input, presets
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
// <chess-app> — orchestrator
// ---------------------------------------------------------------------------

class ChessApp extends ChessComponent {
    connectedCallback() {
        this.render();
        this._init();
    }

    render() {
        this.shadowRoot.innerHTML = `
<style>
:host { display: block; }
h1 { margin: 0 0 4px; font-size: 22px; font-weight: 600; color: #e0e0e0; }
.subtitle { font-size: 13px; color: #777; margin-bottom: 20px; }
a { color: #667eea; }
</style>
<h1>Chess Integer Reduction</h1>
<div class="subtitle">
  Petri net drain-count analysis recovers strategic square values from pure game topology.
</div>
<chess-controls id="controls"></chess-controls>
<chess-board id="board"></chess-board>`;
    }

    _init() {
        const controls = this.$('#controls');
        const board = this.$('#board');

        const refresh = () => {
            const phase = controls.phase;
            try {
                if (phase === 1) board.update(computePhase1());
                else if (phase === 2) board.update(computePhase2());
                else board.update(computePhase3(controls.fen));
            } catch (e) {
                console.error('Analysis error:', e);
            }
        };

        controls.addEventListener('phase-change', refresh);
        controls.addEventListener('fen-change', refresh);

        refresh();
    }
}

// ---------------------------------------------------------------------------
// Register
// ---------------------------------------------------------------------------

customElements.define('chess-board', ChessBoard);
customElements.define('chess-controls', ChessControls);
customElements.define('chess-app', ChessApp);
