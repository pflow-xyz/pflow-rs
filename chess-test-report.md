# Pressure Engine vs Stockfish — Test Report

## Round 2: Quiescence Search + Phase-Dependent Evaluation

**Date:** 2026-02-24
**Opponent:** Stockfish (Easy — depth 5, skill 3)
**Evaluation:** Material + phase-dependent pressure features (king zone, territory, center, mobility, hanging pieces, convergence, passed pawns, pawn shield, mating drive)
**Search:** Alpha-beta + quiescence search (captures/promotions to qDepth 8) + check extensions (max 2/path)

### Performance Timing

| Depth | Time from starting position |
|-------|----------------------------|
| 2 (Easy) | 18ms |
| 3 (Medium) | 94ms |
| 4 (Hard) | 734ms |

### Head-to-Head Results

| Engine | Color | Result | Moves | Notes |
|--------|-------|--------|-------|-------|
| Pressure Easy (d2) | White | **Stockfish wins** | 184 | Much longer survival than Round 1 (48→184) |
| Pressure Easy (d2) | Black | **Stockfish wins** | 61 | Still too shallow |
| Pressure Medium (d3) | White | **Pressure wins** | 47 | Qg7# — decisive attacking win |
| Pressure Medium (d3) | Black | **Stockfish wins** | 75 | Fought well but tactical loss |
| Pressure Hard (d4) | White | **Pressure wins** | 75 | Won exchange, ground down endgame |
| Pressure Hard (d4) | Black | **Pressure wins** | 68 | Won material from opening, converted cleanly |

### Summary

| Metric | d2 | d3 | d4 |
|--------|----|----|-----|
| Wins | 0 | 1 | **2** |
| Draws | 0 | 0 | 0 |
| Losses | 2 | 1 | 0 |

### Comparison: Round 1 → Round 2

| Depth | Round 1 (W/D/L) | Round 2 (W/D/L) | Improvement |
|-------|-----------------|-----------------|-------------|
| d2 | 0/0/2 | 0/0/2 | Longer survival (48,47 → 184,61) |
| d3 | 0/1/1 | **1/0/1** | Converted draw → win (as white) |
| d4 | 0/1/1 | **2/0/0** | Converted both to wins |
| **Total** | **0/2/4** | **3/0/3** | +3 wins, eliminated draws |

### Analysis

**What changed:**

1. **Quiescence search** — The biggest single improvement. Round 1's horizon effect caused Pressure to "walk into" captures at depth 0. Now captures are resolved, so the engine sees tactics clearly. The d3-as-white game shows a clean 47-move checkmate with accurate forcing play.

2. **Check extensions** — Extends search by 1 ply when a move gives check (max 2 per path). This lets the engine see longer forcing sequences without increasing nominal depth.

3. **Phase-dependent evaluation** — Opening weights emphasize center + pawn shield; middlegame weights emphasize king zone + convergence; endgame weights emphasize mobility + passed pawns + mating drive. This replaced the fixed-weight evaluation.

4. **Hanging piece penalty** — Undefended pieces attacked by the opponent get penalized at 0.5× their material value. This prevents casual piece drops.

5. **Mating drive** — In endgames with ≥300cp advantage, rewards pushing the opponent king to the corner and bringing the winning king closer. This directly addresses Round 1's failure to convert material advantage.

6. **Convergence** — Rewards squares where 2+ pieces attack the same target, encouraging piece coordination.

**What's working:**
- The d4-as-black game shows the full pipeline: won material from the opening (Stockfish played h4?), maintained advantage through middlegame, and the mating drive + passed pawn features converted the endgame cleanly.
- The d3-as-white game demonstrates real attacking chess: queen infiltration on the kingside, piece coordination, and a forced checkmate.
- Zero draws means the endgame features (mating drive, passed pawns) solved the Round 1 conversion problem.

**Remaining weakness:**
- d2 still loses — 2 plies of search is fundamentally too shallow even with quiescence
- d3-as-black lost in 75 moves — responding to Stockfish's initiative requires more search depth

### Game PGNs

#### Pressure Easy (d2) as White vs Stockfish Easy
```
d4 d5 Qd3 Nc6 Bf4 Nb4 Qc3 e5 Bxe5 Qg5 f4 Qe7 a3 Nc6 Nd2 Bf5 Ngf3
O-O-O Ng5 f6 h4 fxe5 fxe5 h6 Ngf3 Kb8 Kd1 Qe6 Qe3 Nf6 Qb3 Ng4 Rg1
Be7 g3 Na5 Qc3 Nc4 Nxc4 dxc4 e4 Bxe4 Bxc4 Qb6 Be2 c5 Nd2 cxd4 Qb3 Bxc2+
... (184 ply, Stockfish wins)
```

#### Pressure Easy (d2) as Black vs Stockfish Easy
```
Nf3 d5 d4 Qd6 Nc3 Qe6 Ng5 Qf5 g4 Qxg4 Nxd5 Qd7 e4 Qc6 Bb5 Na6 Qf3
Qxb5 a4 Qc4 b3 Qxd4 c3 Qc5 b4 Qc4 Qxf7+ Kd8 Qxf8+ Kd7 Bf4 h6 O-O-O
Nf6 Nxe7+ Nd5 Qxg7 hxg5 Ng6+ Ke8 exd5 Rh7 Rde1+ Be6 Qg8+ Kd7 Ne5+
... (61 ply, Stockfish wins)
```

#### Pressure Medium (d3) as White vs Stockfish Easy
```
e3 Nf6 Qf3 e5 Nc3 d5 Bb5+ c6 Be2 h5 Nh3 Bb4 Qg3 Bxc3 bxc3 g6 Qxe5+
Qe7 Qd4 O-O Ba3 Qxa3 Qxf6 Qb2 O-O Bf5 Rfc1 Bxh3 gxh3 Nd7 Qd6 Nb8
Rab1 Qxc1+ Rxc1 b6 Qe7 g5 Qxg5+ Kh8 Qf6+ Kh7 Kh1 Re8 Rg1 b5 Qg7#
```

#### Pressure Medium (d3) as Black vs Stockfish Easy
```
e4 e6 e5 Qh4 Nf3 Qe4+ Qe2 Qxc2 Nc3 Qf5 Nd4 Qf4 Nc2 Nc6 Nb5 Kd8 d4
Qf5 h4 Nb4 Ne3 Qe4 f3 Qf4 Qf2 c6 g3 Qh6 Nf5 Qg6 a3 Qxf5 axb4 cxb5
Kd1 a6 g4 Qg6 Qd2 Ke8 h5 Qh6 f4 f6 g5 Bxb4 gxh6 Bf8 Bg2 Nxh6 d5 Nf5
... (75 ply, Stockfish wins)
```

#### Pressure Hard (d4) as White vs Stockfish Easy
```
e3 d5 Qf3 e5 Nc3 c6 Bd3 Nd7 Bf5 Ne7 Bg4 d4 exd4 g6 Ne4 Nf5 Bxf5 Qe7
Bxd7+ Bxd7 d5 cxd5 Nf6+ Kd8 Nxd7 e4 Qf6 Rg8 Qxe7+ Bxe7 Ne5 Ke8 Ng4
Rc8 c3 d4 cxd4 a6 Ne2 f5 Ne5 Bf6 f4 Rc2 Nc3 Ke7 Kd1 Rxd2+ Bxd2 Ra8
... (75 ply, Pressure wins)
```

#### Pressure Hard (d4) as Black vs Stockfish Easy
```
h4 e6 d4 Qf6 e4 Nc6 Be3 e5 Bg5 Qg6 Nd2 h6 Bd8 Kxd8 a3 Nxd4 h5 Qd6
c3 Nc6 Ngf3 Nf6 Qb3 Qe6 Bc4 Qe7 Bxf7 Qc5 O-O d6 Qa2 Ne7 Rad1 b5 Nb3
Qc6 Rfe1 Bd7 Na5 Qb6 Nb3 Nxh5 Nxe5 dxe5 a4 Nf6 a5 Qc6 f3 Kc8 Re3 Kb8
... (68 ply, Pressure wins)
```

---

## Round 1: Spatial Pressure Features (Baseline)

**Date:** 2026-02-24
**Opponent:** Stockfish (Easy — depth 1, skill 0)
**Evaluation:** Material + pressure-derived spatial features (king zone, territory, center, mobility)

### Performance Timing

| Depth | Time from starting position |
|-------|----------------------------|
| 2 (Easy) | 18ms |
| 3 (Medium) | 68ms |
| 4 (Hard) | 459ms |

### Head-to-Head Results

| Engine | Color | Result | Moves | Notes |
|--------|-------|--------|-------|-------|
| Pressure Easy (d2) | White | **Stockfish wins** | 48 | Bxc3# — blunders pieces early |
| Pressure Easy (d2) | Black | **Stockfish wins** | 47 | Qa6# — can't defend at low depth |
| Pressure Medium (d3) | White | **Stockfish wins** | 76 | Qxf1# — longer resistance, tactical errors |
| Pressure Medium (d3) | Black | **Draw** | 200 | Won material (rook+bishop vs bare king area), couldn't convert |
| Pressure Hard (d4) | White | **Stockfish wins** | 114 | Rd2# — lengthy fight, pressure-driven play visible |
| Pressure Hard (d4) | Black | **Draw** | 200 | Won heavy material, promoted pawn, but couldn't deliver checkmate |

### Summary

| Metric | d2 | d3 | d4 |
|--------|----|----|-----|
| Wins | 0 | 0 | 0 |
| Draws | 0 | 1 | 1 |
| Losses | 2 | 1 | 1 |
| Avg moves survived | 47.5 | 138 | 157 |

---

## Previous Report (2026-02-23): Random Play vs All Engines

Headless browser tests playing random legal moves (as White) against each engine preset.

| # | Engine | Result | Moves | Time | Winner |
|---|--------|--------|-------|------|--------|
| 1 | Pressure (Easy) | Checkmate | 82 | 6.3s | Black |
| 2 | Pressure (Medium) | Checkmate | 36 | 2.8s | Black |
| 3 | Stockfish (Easy) | Checkmate | 32 | 2.2s | Black |
| 4 | Stockfish (Medium) | Checkmate | 32 | 5.3s | Black |
| 5 | Stockfish (Hard) | Checkmate | 26 | 10.4s | Black |
