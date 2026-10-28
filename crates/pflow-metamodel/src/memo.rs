//! The game search's transposition table: bounded, flat, and exact on bit
//! markings.
//!
//! An entry is a fixed run of words in one `Vec<u64>`: the key, then a meta
//! word (best move, the value's bounds, how much work found them, and when),
//! so storing one allocates nothing. Over the bit engine the key is the
//! packed marking itself ([`crate::bits::BitModel::marking`]), so two
//! positions never share an entry; over counts it is a 128-bit hash. When a
//! game's rewards are few ([`crate::game::Game`] steps through them), the
//! bounds are one-byte indices into the values seen so far; otherwise they
//! take a word each. Beside the entries, one byte per slot holds seven bits
//! of the key's hash (zero when empty), so a probe reads a few bytes and
//! compares a key only where they match.
//!
//! Entries sit in buckets of [`WAYS`], and a key may sit in either of two
//! buckets its hash picks, which makes a full one rare until the table is.
//! A new key takes an empty slot of the two, or else the weakest: one left
//! by an earlier [`Table::age`] first, then the one found with the least
//! work (log2 of the nodes under it). The table starts small and doubles
//! when a new key finds both its buckets full and a quarter of the slots are
//! taken (below that they fill by chance, not for want of room), up to the
//! entries it was given, and never grows past them: a search that outruns
//! it forgets, and searches again, but its memory stays put. Forgetting
//! never changes a value, only what it costs.
//!
//! With the `partial_key` layout ([`Table::partial`]) an entry does not
//! store its whole key. The marking (a bit key of `nbits` bits) is scrambled
//! by a bijection of `nbits`-bit numbers, then cut by the bucket count `B`:
//! `v = q * B + r`. The bucket is `r`, so the slot already says which `r`,
//! and the entry keeps `q` alone, plus one bit saying whether it sits in
//! its first bucket (`r`) or its second (`r` moved by an offset that `q`
//! decides, which `q` and the bucket together undo). `(q, r)` determines
//! `v` and so the marking: two positions never share an entry, as with the
//! whole key, and an entry takes `ceil((bits of q + 1) / 64)` words where the
//! whole key took `ceil(nbits / 64)`: Connect-4's 131 places drop from three
//! words to two at 2^23 entries. This is Pons's trick with a table size of
//! any kind (a division, not a prime) and a scramble in front, since the low
//! cells of a board are a poor bucket. Counts keys (hashes, after the bit
//! engine meets a marking it cannot hold) go in a plain side table made on
//! first use. The table doubles by decoding each entry back to `v` and
//! splitting it again, so its width shrinks as it grows.
//!
//! A pinned entry ([`Table::pin`], an opening book's) is exact and ranks
//! above every other, so a new key takes its slot only when all the slots
//! it could take are pinned. Pinned entries carry generation 0, which
//! [`Table::age`] never reaches.

/// Slots per bucket.
pub(crate) const WAYS: usize = 4;

/// Words of key at most (the bit engine's inline marking).
pub(crate) const KEY: usize = 4;

/// The slots a table starts with, at most.
const START: usize = 1024;

/// Distinct bounds a compact table indexes, at most.
const DICT: usize = 255;

const BEST: u64 = 0xFFFF_FFFF;
const LO_SHIFT: u32 = 32;
const HI_SHIFT: u32 = 40;
const WORK_SHIFT: u32 = 48;
const GEN_SHIFT: u32 = 56;
const GEN_MASK: u64 = 0x7F;
/// The generation of a pinned entry.
const PINNED: u64 = 0;
const COUNTS: u64 = 1 << 63;

/// What a table holds for one position: the value lies in `[lo, hi]` (equal
/// when exact), and `best` did best.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Entry {
    pub lo: f64,
    pub hi: f64,
    pub best: u32,
}

/// A key: `kw` words of a marking (bits) or a hash (counts), the rest zero.
pub(crate) type Key = [u64; KEY];

/// Where a key is, or would go: see [`Table::locate`].
type Slot = std::result::Result<usize, usize>;

/// See the module docs.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    /// Words of key (or, with `nbits`, of `q` and its bit) compared.
    kw: usize,
    /// Partial keys: the bits of the marking, else 0. See the module docs.
    nbits: usize,
    /// Partial keys: the most bits `q` takes at this size.
    qbits: usize,
    /// Partial keys: a plain table for counts keys, made when one is met.
    side: Option<Box<Table>>,
    /// The entries the table was given.
    entries: usize,
    /// Bounds as indices into `dict` (else two words after the meta word).
    compact: bool,
    stride: usize,
    slots: Vec<u64>,
    /// Per slot: seven bits of its key's hash, with the eighth set; zero
    /// when the slot is empty.
    tags: Vec<u8>,
    buckets: usize,
    /// The most buckets it grows to, and the slots in each.
    most: usize,
    ways: usize,
    len: usize,
    generation: u64,
    /// Bounds seen, as `f64` bits, for compact entries.
    dict: Vec<u64>,
}

fn mix(k: &Key) -> u64 {
    // One multiply per word, independent of the others, then a finish.
    let mut h = k[0].wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ k[1].wrapping_mul(0xC2B2_AE3D_27D4_EB4F).rotate_left(21)
        ^ k[2].wrapping_mul(0x1656_67B1_9E37_79F9).rotate_left(42)
        ^ k[3].wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^ h >> 32
}

/// Where a key points: its two buckets' first slots, what is stored in
/// each (the key, or with `partial_key` the rest of it and a bit), and its
/// tag.
struct Probe {
    buckets: [usize; 2],
    stored: [Key; 2],
    tag: u8,
}

/// The parts of a table's shape an entry's layout depends on: they change
/// as it grows.
#[derive(Clone, Copy)]
struct Geo {
    kw: usize,
    stride: usize,
    buckets: usize,
    qbits: usize,
}

const C1: Key = [
    0x9E37_79B9_7F4A_7C15,
    0xD1B5_4A32_D192_ED03,
    0xA24B_AED4_963E_E407,
    0x9FB2_1C65_1E98_DF25,
];
const C2: Key = [
    0xC2B2_AE3D_27D4_EB4F,
    0x1656_67B1_9E37_79F9,
    0xFF51_AFD7_ED55_8CCD,
    0xC4CE_B9FE_1A85_EC53,
];

/// `a * b` mod 2^(64 `nw`), the words above `nw` zero.
fn mul(a: &Key, b: &Key, nw: usize) -> Key {
    let mut r = [0u64; KEY];
    for i in 0..nw {
        let mut carry = 0u128;
        for j in 0..nw - i {
            let t = a[i] as u128 * b[j] as u128 + r[i + j] as u128 + carry;
            r[i + j] = t as u64;
            carry = t >> 64;
        }
    }
    r
}

/// `x >> s`.
fn shr(x: &Key, s: usize) -> Key {
    let (w, b) = (s / 64, s % 64);
    let mut r = [0u64; KEY];
    for i in 0..KEY {
        if i + w < KEY {
            r[i] = x[i + w] >> b;
            if b > 0 && i + w + 1 < KEY {
                r[i] |= x[i + w + 1] << (64 - b);
            }
        }
    }
    r
}

/// `x` cut to its low `nbits` bits.
fn mask(x: &mut Key, nbits: usize) {
    for (i, w) in x.iter_mut().enumerate() {
        let lo = i * 64;
        if nbits <= lo {
            *w = 0;
        } else if nbits < lo + 64 {
            *w &= (1u64 << (nbits - lo)) - 1;
        }
    }
}

/// A bijection of the numbers below 2^`nbits`: multiply by an odd number,
/// fold the high half onto the low, twice. The low bits of the result,
/// which pick the bucket, depend on every bit of `k`.
fn scramble(k: &Key, nbits: usize) -> Key {
    let mut x = *k;
    mask(&mut x, nbits);
    let s = nbits.div_ceil(2).max(1);
    let nw = nbits.div_ceil(64);
    for c in [&C1, &C2] {
        x = mul(&x, c, nw);
        mask(&mut x, nbits);
        let h = shr(&x, s);
        for i in 0..nw {
            x[i] ^= h[i];
        }
    }
    x
}

/// `v` as `(q, r)` with `v = q * buckets + r`.
fn split(v: &Key, buckets: usize) -> (Key, usize) {
    if buckets.is_power_of_two() {
        let s = buckets.trailing_zeros() as usize;
        return (shr(v, s), (v[0] as usize) & (buckets - 1));
    }
    let (mut q, mut rem) = ([0u64; KEY], 0u128);
    for i in (0..KEY).rev() {
        let cur = rem << 64 | v[i] as u128;
        q[i] = (cur / buckets as u128) as u64;
        rem = cur % buckets as u128;
    }
    (q, rem as usize)
}

/// `q * buckets + r`.
fn join(q: &Key, buckets: usize, r: usize) -> Key {
    let mut v = [0u64; KEY];
    let mut carry = r as u128;
    for i in 0..KEY {
        let t = q[i] as u128 * buckets as u128 + carry;
        v[i] = t as u64;
        carry = t >> 64;
    }
    v
}

/// The bits of `x`.
fn bit_len(x: &Key) -> usize {
    (0..KEY)
        .rev()
        .find(|&i| x[i] != 0)
        .map_or(0, |i| i * 64 + 64 - x[i].leading_zeros() as usize)
}

/// Bits `q` takes at most for `nbits`-bit keys cut by `buckets`.
fn q_bits(nbits: usize, buckets: usize) -> usize {
    let mut top = [u64::MAX; KEY];
    mask(&mut top, nbits);
    bit_len(&split(&top, buckets).0)
}

/// The words of key an entry keeps under `partial_key`, for `nbits`-bit keys
/// in a table of `entries` at most (the width it has grown to), or `None`
/// when `q` and its bit cannot fit in [`KEY`] words.
pub(crate) fn partial_words(nbits: usize, entries: usize) -> Option<usize> {
    if nbits == 0 || nbits >= KEY * 64 {
        return None;
    }
    let ways = WAYS.min(entries.max(1));
    let most = (entries.max(1) / ways).max(1);
    Some((q_bits(nbits, most) + 1).div_ceil(64))
}

impl Table {
    /// A table of at most `entries`, comparing `kw` words of key.
    pub fn new(entries: usize, kw: usize, compact: bool) -> Self {
        assert!((1..=KEY).contains(&kw));
        Self::build(entries, kw, compact, 0)
    }

    /// A table of at most `entries` over `nbits`-bit keys (a bit marking of
    /// that many places), each entry keeping only the part of its key the
    /// bucket does not say: see the module docs. `nbits` is at most
    /// `64 * KEY - 1`, and counts keys wait in a plain table of their own.
    pub fn partial(entries: usize, nbits: usize, compact: bool) -> Self {
        assert!((1..KEY * 64).contains(&nbits));
        Self::build(entries, 0, compact, nbits)
    }

    fn build(entries: usize, kw: usize, compact: bool, nbits: usize) -> Self {
        let entries = entries.max(1);
        let ways = WAYS.min(entries);
        let most = entries / ways;
        let buckets = most.min(START / ways).max(1);
        let mut t = Self {
            kw,
            nbits,
            qbits: 0,
            side: None,
            entries,
            compact,
            stride: 0,
            slots: vec![],
            tags: vec![0; buckets * ways],
            buckets,
            most,
            ways,
            len: 0,
            generation: 1,
            dict: vec![],
        };
        let g = t.geo_at(buckets);
        t.set_geo(g);
        t.slots = vec![0; buckets * ways * t.stride];
        t
    }

    /// The shape at `buckets` buckets.
    fn geo_at(&self, buckets: usize) -> Geo {
        let (kw, qbits) = if self.nbits > 0 {
            let q = q_bits(self.nbits, buckets);
            ((q + 1).div_ceil(64), q)
        } else {
            (self.kw, 0)
        };
        Geo {
            kw,
            stride: kw + if self.compact { 1 } else { 3 },
            buckets,
            qbits,
        }
    }

    fn geo(&self) -> Geo {
        Geo {
            kw: self.kw,
            stride: self.stride,
            buckets: self.buckets,
            qbits: self.qbits,
        }
    }

    fn set_geo(&mut self, g: Geo) {
        self.kw = g.kw;
        self.stride = g.stride;
        self.buckets = g.buckets;
        self.qbits = g.qbits;
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.len + self.side.as_ref().map_or(0, |s| s.len())
    }

    /// Bytes one entry takes once the table has grown to its limit, its tag
    /// included (fewer than at the start, with `partial_key`).
    pub fn entry_bytes(&self) -> usize {
        self.geo_at(self.most).stride * 8 + 1
    }

    /// Bytes the table takes now (counts keys' side table not included).
    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.slots.len() * 8 + self.tags.len()
    }

    /// Bytes the table takes once grown to its limit.
    pub fn most_bytes(&self) -> usize {
        self.most * self.ways * self.entry_bytes()
    }

    /// Entries from here on are newer than those before, which a new key
    /// replaces first.
    pub fn age(&mut self) {
        self.generation = self.generation % GEN_MASK + 1;
        if let Some(s) = &mut self.side {
            s.age();
        }
    }

    /// The probe for identity `v`: the scrambled key, with `partial_key`.
    fn probe_ident(&self, v: &Key) -> Probe {
        if self.nbits == 0 {
            let h = mix(v);
            let h2 = h.rotate_left(32).wrapping_mul(0x94D0_49BB_1331_11EB);
            return Probe {
                buckets: [h, h2]
                    .map(|x| ((x as u128 * self.buckets as u128) >> 64) as usize * self.ways),
                stored: [*v, *v],
                tag: (h2 >> 57) as u8 | 0x80,
            };
        }
        let (q, r) = split(v, self.buckets);
        let h = mix(&q);
        let d = if self.buckets > 1 {
            1 + ((h as u128 * (self.buckets as u128 - 1)) >> 64) as usize
        } else {
            0
        };
        let r2 = (r + d) % self.buckets;
        let mut flagged = q;
        flagged[self.qbits / 64] |= 1 << (self.qbits % 64);
        Probe {
            buckets: [r * self.ways, r2 * self.ways],
            stored: [q, flagged],
            tag: (h.rotate_left(32).wrapping_mul(0x94D0_49BB_1331_11EB) >> 57) as u8 | 0x80,
        }
    }

    fn probe(&self, k: &Key) -> Probe {
        if self.nbits == 0 {
            self.probe_ident(k)
        } else {
            self.probe_ident(&scramble(k, self.nbits))
        }
    }

    /// The identity of the entry in slot `i` of a table shaped `geo`
    /// (its `slots` given): what [`Table::probe_ident`] was handed.
    fn ident(&self, geo: &Geo, slots: &[u64], i: usize) -> Key {
        let mut k = [0u64; KEY];
        k[..geo.kw].copy_from_slice(&slots[i * geo.stride..i * geo.stride + geo.kw]);
        if self.nbits == 0 {
            return k;
        }
        let bucket = i / self.ways;
        let flag = k[geo.qbits / 64] >> (geo.qbits % 64) & 1 == 1;
        k[geo.qbits / 64] &= !(1 << (geo.qbits % 64));
        let r = if flag {
            let h = mix(&k);
            let d = 1 + ((h as u128 * (geo.buckets as u128 - 1)) >> 64) as usize;
            (bucket + geo.buckets - d) % geo.buckets
        } else {
            bucket
        };
        join(&k, geo.buckets, r)
    }

    fn meta(&self, i: usize) -> u64 {
        self.slots[i * self.stride + self.kw]
    }

    fn engine(counts: bool) -> u64 {
        if counts {
            COUNTS
        } else {
            0
        }
    }

    /// Whether slot `i`, whose tag matches, holds `stored`.
    fn holds(&self, i: usize, stored: &Key, engine: u64) -> bool {
        let s = &self.slots[i * self.stride..i * self.stride + self.kw + 1];
        s[..self.kw] == stored[..self.kw] && s[self.kw] & COUNTS == engine
    }

    /// The slot holding the probed key (`Ok`), or else the one it would
    /// take (`Err`): an empty slot of its buckets, or failing that the
    /// weakest, an entry of an earlier [`Table::age`] first, then the one
    /// found with the least work. Only a store needs the second; a lookup
    /// passes `choose` false and gets `Err(0)` for a key not held.
    ///
    /// A key goes to its second bucket only when its first is full, and a
    /// slot, once taken, stays taken until the table grows (which puts
    /// every entry back by the same rule): so a first bucket with an empty
    /// slot means the key is not in the second.
    fn locate(&self, p: &Probe, counts: bool, choose: bool) -> Slot {
        let engine = Self::engine(counts);
        for (n, b) in p.buckets.into_iter().enumerate() {
            let mut room = None;
            for (j, &t) in self.tags[b..b + self.ways].iter().enumerate() {
                if t == p.tag && self.holds(b + j, &p.stored[n], engine) {
                    return Ok(b + j);
                }
                if t == 0 && room.is_none() {
                    room = Some(b + j);
                }
            }
            if let Some(i) = room {
                return Err(i);
            }
        }
        if !choose {
            return Err(0);
        }
        let rank = |i: usize| {
            let m = self.meta(i);
            let gen = m >> GEN_SHIFT & GEN_MASK;
            if gen == PINNED {
                return u64::MAX;
            }
            ((gen == self.generation) as u64) << 8 | (m >> WORK_SHIFT & 0xFF)
        };
        let ways = self.ways;
        let all = p.buckets.into_iter().flat_map(|b| b..b + ways);
        Err(all.min_by_key(|&i| rank(i)).unwrap())
    }

    /// The plain table counts keys use under `partial_key`.
    fn plain(&mut self) -> &mut Table {
        let (entries, compact, generation) = (self.entries, self.compact, self.generation);
        self.side.get_or_insert_with(|| {
            let mut t = Table::new(entries, 2, compact);
            t.generation = generation;
            Box::new(t)
        })
    }

    pub fn get(&self, k: &Key, counts: bool) -> Option<Entry> {
        if counts && self.nbits > 0 {
            return self.side.as_ref()?.get(k, true);
        }
        let p = self.probe(k);
        self.locate(&p, counts, false).ok().map(|i| self.read(i))
    }

    /// The entry in slot `i`.
    fn read(&self, i: usize) -> Entry {
        let s = &self.slots[i * self.stride..(i + 1) * self.stride];
        let meta = s[self.kw];
        let (lo, hi) = if self.compact {
            let at = |shift: u32| f64::from_bits(self.dict[(meta >> shift & 0xFF) as usize]);
            (at(LO_SHIFT), at(HI_SHIFT))
        } else {
            (
                f64::from_bits(s[self.kw + 1]),
                f64::from_bits(s[self.kw + 2]),
            )
        };
        Entry {
            lo,
            hi,
            best: (meta & BEST) as u32,
        }
    }

    /// The index of bound `x` in the dictionary, added if new and room.
    fn index(&mut self, x: f64) -> Option<u64> {
        let b = x.to_bits();
        match self.dict.iter().position(|&d| d == b) {
            Some(i) => Some(i as u64),
            None if self.dict.len() < DICT => {
                self.dict.push(b);
                Some(self.dict.len() as u64 - 1)
            }
            None => None,
        }
    }

    /// Remembers `e` for `k`, found by `nodes` nodes of search, narrowed by
    /// what the table knew of `k` before.
    pub fn store(&mut self, k: &Key, counts: bool, e: Entry, nodes: usize) {
        let work = (usize::BITS - nodes.leading_zeros()) as u64;
        self.keep(k, counts, e, work, self.generation);
    }

    /// Remembers that `k` is worth exactly `value`, `best` doing best (or
    /// [`u32::MAX`] for none known), pinned: see the module docs.
    pub fn pin(&mut self, k: &Key, counts: bool, value: f64, best: u32) {
        let e = Entry {
            lo: value,
            hi: value,
            best,
        };
        self.keep(k, counts, e, 0xFF, PINNED);
    }

    /// [`Table::store`], with the work and generation given; a pinned entry
    /// stays pinned.
    fn keep(&mut self, k: &Key, counts: bool, e: Entry, work: u64, gen: u64) {
        if counts && self.nbits > 0 {
            return self.plain().keep(k, true, e, work, gen);
        }
        let mut p = self.probe(k);
        let mut at = self.locate(&p, counts, true);
        let (mut e, mut work, mut gen) = (e, work, gen);
        if let Ok(i) = at {
            let old = self.read(i);
            e.lo = e.lo.max(old.lo);
            e.hi = e.hi.min(old.hi);
            if gen == PINNED && e.best == BEST as u32 {
                e.best = old.best;
            }
            let m = self.meta(i);
            work = work.max(m >> WORK_SHIFT & 0xFF);
            if m >> GEN_SHIFT & GEN_MASK == PINNED {
                gen = PINNED;
            }
        }
        let mut meta = e.best as u64 | work << WORK_SHIFT | gen << GEN_SHIFT | Self::engine(counts);
        let mut wide = [0u64; 2];
        if self.compact {
            let (Some(lo), Some(hi)) = (self.index(e.lo), self.index(e.hi)) else {
                // More distinct bounds than a byte indexes: not kept.
                return;
            };
            meta |= lo << LO_SHIFT | hi << HI_SHIFT;
        } else {
            wide = [e.lo.to_bits(), e.hi.to_bits()];
        }
        // Both buckets full: room is wanted, if the table may still grow.
        while let Err(i) = at {
            if self.tags[i] == 0
                || self.buckets == self.most
                || self.len * 4 < self.buckets * self.ways
            {
                break;
            }
            self.grow();
            p = self.probe(k);
            at = self.locate(&p, counts, true);
        }
        self.put(at, &p, meta, wide);
    }

    /// Writes an entry where [`Table::locate`] said.
    fn put(&mut self, at: Slot, p: &Probe, meta: u64, wide: [u64; 2]) {
        let (Ok(i) | Err(i)) = at;
        if self.tags[i] == 0 {
            self.len += 1;
        }
        self.tags[i] = p.tag;
        let n = (i / self.ways * self.ways != p.buckets[0]) as usize;
        let s = &mut self.slots[i * self.stride..(i + 1) * self.stride];
        s[..self.kw].copy_from_slice(&p.stored[n][..self.kw]);
        s[self.kw] = meta;
        if !self.compact {
            s[self.kw + 1..].copy_from_slice(&wide);
        }
    }

    /// Doubles the buckets, up to the limit, and moves every entry over.
    fn grow(&mut self) {
        let old = std::mem::take(&mut self.slots);
        let tags = std::mem::take(&mut self.tags);
        let was = self.geo();
        let now = self.geo_at((self.buckets * 2).min(self.most));
        self.set_geo(now);
        self.slots = vec![0; self.buckets * self.ways * self.stride];
        self.tags = vec![0; self.buckets * self.ways];
        self.len = 0;
        for (i, _) in tags.iter().enumerate().filter(|x| *x.1 != 0) {
            let s = &old[i * was.stride..(i + 1) * was.stride];
            let meta = s[was.kw];
            let v = self.ident(&was, &old, i);
            let wide = if self.compact {
                [0; 2]
            } else {
                [s[was.kw + 1], s[was.kw + 2]]
            };
            let p = self.probe_ident(&v);
            let at = self.locate(&p, meta & COUNTS != 0, true);
            self.put(at, &p, meta, wide);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(i: u64) -> Key {
        [i, i.wrapping_mul(31), 0, 0]
    }

    fn entry(i: u64) -> Entry {
        Entry {
            lo: -(i as f64 % 3.0),
            hi: 1.0,
            best: i as u32,
        }
    }

    #[test]
    fn holds_what_it_was_given_up_to_its_limit() {
        for compact in [true, false] {
            let mut t = Table::new(10_000, 2, compact);
            for i in 0..5000 {
                t.store(&key(i), false, entry(i), 1);
            }
            // Below its limit the table grows rather than forget, and with
            // two buckets to choose from a key rarely finds both full.
            let kept = (0..5000)
                .filter(|&i| t.get(&key(i), false) == Some(entry(i)))
                .count();
            assert!(kept > 4950, "{kept}");
            for i in 0..100_000 {
                t.store(&key(i), false, entry(i), 1);
            }
            assert!(t.len() <= 10_000, "{}", t.len());
            assert_eq!(t.bytes(), t.most_bytes());
        }
    }

    #[test]
    fn a_tiny_table_keeps_to_its_entries() {
        for n in 1..12 {
            let mut t = Table::new(n, 3, true);
            for i in 0..100 {
                t.store(&key(i), false, entry(i), 1);
                assert!(t.len() <= n);
            }
        }
    }

    #[test]
    fn engines_and_keys_do_not_share_entries() {
        let mut t = Table::new(100, 2, true);
        t.store(&key(7), false, entry(7), 1);
        assert_eq!(t.get(&key(7), true), None);
        assert_eq!(t.get(&key(8), false), None);
        t.store(&key(7), true, entry(8), 1);
        assert_eq!(t.get(&key(7), false), Some(entry(7)));
        assert_eq!(t.get(&key(7), true), Some(entry(8)));
    }

    #[test]
    fn a_store_narrows_what_was_known() {
        let mut t = Table::new(100, 2, true);
        let at = |lo, hi| Entry { lo, hi, best: 1 };
        t.store(&key(1), false, at(-1.0, 1.0), 1);
        t.store(&key(1), false, at(0.0, 1.0), 1);
        t.store(&key(1), false, at(-1.0, 0.0), 1);
        assert_eq!(t.get(&key(1), false), Some(at(0.0, 0.0)));
    }

    #[test]
    fn deep_and_current_entries_outlast_shallow_and_old_ones() {
        // One bucket of four.
        let mut t = Table::new(4, 1, true);
        for i in 0..4 {
            t.store(
                &[i, 0, 0, 0],
                false,
                entry(i),
                if i == 0 { 1 << 20 } else { 1 },
            );
        }
        t.store(&[9, 0, 0, 0], false, entry(9), 1);
        assert!(t.get(&[0, 0, 0, 0], false).is_some(), "the deep one stays");
        // Three newer entries take the shallow old slots, and the next one
        // takes the deep old one rather than a newer shallow one.
        t.age();
        for i in 10..13 {
            t.store(&[i, 0, 0, 0], false, entry(i), 1);
        }
        assert!(t.get(&[0, 0, 0, 0], false).is_some());
        t.store(&[13, 0, 0, 0], false, entry(13), 1);
        assert!(t.get(&[0, 0, 0, 0], false).is_none(), "old goes first");
        assert!((10..14).all(|i| t.get(&[i, 0, 0, 0], false).is_some()));
    }

    #[test]
    fn pinned_entries_outlast_everything_else() {
        // Two buckets of four, whatever the keys.
        let mut t = Table::new(8, 2, true);
        for i in 0..4 {
            t.pin(&key(i), false, 1.0, i as u32);
        }
        for round in 0..50 {
            t.age();
            for i in 100 + round * 20..120 + round * 20 {
                t.store(&key(i), false, entry(i), 1 << 30);
            }
        }
        for i in 0..4 {
            let e = t.get(&key(i), false).expect("pinned");
            assert_eq!((e.lo, e.hi, e.best), (1.0, 1.0, i as u32));
        }
        // A search's store on a pinned key leaves it pinned, and a pin with
        // no best move keeps the one known.
        t.store(&key(0), false, entry(0), 1);
        t.pin(&key(1), false, 1.0, BEST as u32);
        assert_eq!(t.get(&key(1), false).unwrap().best, 1);
        for i in 0..400 {
            t.store(&key(1000 + i), false, entry(i), 1 << 30);
        }
        assert!((0..4).all(|i| t.get(&key(i), false).is_some()));
    }

    /// Law B1 (docs/search-composition-laws.md): storing on a key held is
    /// the meet of intervals in lo and hi: commutative, associative,
    /// idempotent, the whole range its unit, an empty interval absorbing.
    /// The best move is not met: the last store's wins.
    #[test]
    fn stores_meet_in_lo_and_hi_and_the_last_best_wins() {
        let vals = [-1.0, 0.0, 1.0];
        let mut all = vec![];
        for lo in vals {
            for hi in vals {
                all.push((lo, hi));
            }
        }
        let ivs: Vec<(f64, f64)> = all.iter().copied().filter(|x| x.0 <= x.1).collect();
        // The entry after storing `xs`, in turn, on one key.
        let meet = |xs: &[(f64, f64, u32)]| {
            let mut t = Table::new(16, 1, true);
            for &(lo, hi, best) in xs {
                t.store(&[1, 0, 0, 0], false, Entry { lo, hi, best }, 1);
            }
            t.get(&[1, 0, 0, 0], false).unwrap()
        };
        let iv = |e: Entry| (e.lo, e.hi);
        for &a in &ivs {
            assert_eq!(iv(meet(&[(a.0, a.1, 1), (a.0, a.1, 1)])), a, "idempotent");
            let top = meet(&[(a.0, a.1, 1), (-1.0, 1.0, 2)]);
            assert_eq!(iv(top), a, "the whole range is the unit");
            assert_eq!(top.best, 2, "but its best move is kept");
            for e in [
                meet(&[(1.0, -1.0, 1), (a.0, a.1, 2)]),
                meet(&[(a.0, a.1, 2), (1.0, -1.0, 1)]),
            ] {
                assert!(e.lo > e.hi, "an empty interval stays empty");
            }
            for &b in &ivs {
                let ab = meet(&[(a.0, a.1, 1), (b.0, b.1, 2)]);
                let ba = meet(&[(b.0, b.1, 2), (a.0, a.1, 1)]);
                assert_eq!(iv(ab), (a.0.max(b.0), a.1.min(b.1)));
                assert_eq!(iv(ab), iv(ba), "commutative");
                assert_eq!((ab.best, ba.best), (2, 1), "the last best wins");
                for &c in &ivs {
                    let bc = meet(&[(b.0, b.1, 2), (c.0, c.1, 3)]);
                    let left = meet(&[(a.0, a.1, 1), (b.0, b.1, 2), (c.0, c.1, 3)]);
                    let right = meet(&[(a.0, a.1, 1), (bc.lo, bc.hi, 3)]);
                    assert_eq!(iv(left), iv(right), "associative");
                }
            }
        }
    }

    /// Law B2: a pin absorbs a later store, and meets an earlier one; it
    /// stays pinned either way. A store outside a pin's value (a wrong
    /// book) leaves an empty interval, unnoticed.
    #[test]
    fn a_pin_absorbs_stores_and_stays_pinned() {
        let k = [5, 0, 0, 0];
        let pinned = |t: &Table| {
            let i = t.locate(&t.probe(&k), false, false).unwrap();
            t.meta(i) >> GEN_SHIFT & GEN_MASK == PINNED
        };
        let at = |lo, hi| Entry { lo, hi, best: 3 };
        let mut t = Table::new(16, 1, true);
        t.pin(&k, false, 0.0, 1);
        t.store(&k, false, at(-1.0, 1.0), 1);
        let e = t.get(&k, false).unwrap();
        assert_eq!((e.lo, e.hi), (0.0, 0.0));
        assert!(pinned(&t));
        let mut t = Table::new(16, 1, true);
        t.store(&k, false, at(-1.0, 0.0), 1);
        t.pin(&k, false, 0.0, 1);
        let e = t.get(&k, false).unwrap();
        assert_eq!((e.lo, e.hi, e.best), (0.0, 0.0, 1));
        assert!(pinned(&t));
        let mut t = Table::new(16, 1, true);
        t.pin(&k, false, 1.0, 1);
        t.store(&k, false, at(-1.0, 0.0), 1);
        let e = t.get(&k, false).unwrap();
        assert!(e.lo > e.hi, "{e:?}");
    }

    /// A random `nbits`-bit key.
    fn bits_key(r: &mut u64, nbits: usize) -> Key {
        let mut k = [0u64; KEY];
        for w in &mut k {
            *r = r
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *w = *r ^ *r >> 29;
        }
        mask(&mut k, nbits);
        k
    }

    #[test]
    fn the_scramble_is_a_bijection_and_split_joins_back() {
        // Every 12-bit key goes to its own number, and q, r make it again.
        for nbits in [1, 5, 12] {
            let mut seen = std::collections::HashSet::new();
            for i in 0..1u64 << nbits {
                let v = scramble(&[i, 0, 0, 0], nbits);
                assert!(bit_len(&v) <= nbits);
                assert!(seen.insert(v), "{nbits} bits: {i} collides");
                for buckets in [1, 2, 3, 7, 64, 100] {
                    let (q, r) = split(&v, buckets);
                    assert!(r < buckets);
                    assert_eq!(join(&q, buckets, r), v);
                }
            }
        }
        // The same at 131 and 255 bits, on random keys.
        let mut r = 7;
        for nbits in [131, 200, 255] {
            for _ in 0..200 {
                let v = scramble(&bits_key(&mut r, nbits), nbits);
                for buckets in [1, 4, 1000, 1 << 21, 3_000_001] {
                    let (q, rem) = split(&v, buckets);
                    assert_eq!(join(&q, buckets, rem), v);
                    assert!(bit_len(&q) <= q_bits(nbits, buckets));
                }
            }
        }
    }

    #[test]
    fn a_partial_table_holds_what_a_whole_one_holds() {
        // The same stores into both: a key found reads back what was stored
        // for it, through every doubling, on a table size that is a power
        // of two and on one that is not, and nearly all are found (a store
        // may evict when both its buckets are full, as in a whole table).
        for (nbits, entries) in [
            (27usize, 1usize << 16),
            (131, 1 << 16),
            (131, 100_000),
            (255, 50_000),
        ] {
            for compact in [true, false] {
                let mut whole = Table::new(entries, nbits.div_ceil(64).max(2), compact);
                let mut part = Table::partial(entries, nbits, compact);
                let mut r = 11;
                let mut keys: Vec<Key> = (0..10_000).map(|_| bits_key(&mut r, nbits)).collect();
                keys.sort();
                keys.dedup();
                for (i, k) in keys.iter().enumerate() {
                    let e = entry(i as u64);
                    whole.store(k, false, e, 1 + i % 7);
                    part.store(k, false, e, 1 + i % 7);
                }
                let (mut kept_whole, mut kept_part) = (0, 0);
                for (i, k) in keys.iter().enumerate() {
                    kept_whole += whole.get(k, false).is_some() as usize;
                    if let Some(e) = part.get(k, false) {
                        assert_eq!(e, entry(i as u64), "{nbits} bits, key {i}");
                        kept_part += 1;
                    }
                }
                assert!(
                    kept_part + 100 >= kept_whole,
                    "{kept_part} against {kept_whole}"
                );
                assert!(kept_part > keys.len() * 98 / 100, "{kept_part}");
                assert!(part.entry_bytes() <= whole.entry_bytes());
                // A key never stored is not there.
                for _ in 0..2000 {
                    let k = bits_key(&mut r, nbits);
                    assert_eq!(part.get(&k, false), None);
                }
            }
        }
    }

    #[test]
    fn a_partial_table_never_confuses_two_keys_and_keeps_to_its_entries() {
        // Far more keys than slots, so many are evicted: what is found is
        // always what was stored for that key, never another's.
        for entries in [8, 64, 1000, 5000] {
            let mut t = Table::partial(entries, 100, true);
            let mut r = 3;
            let keys: Vec<Key> = (0..4 * entries + 50)
                .map(|_| bits_key(&mut r, 100))
                .collect();
            for (i, k) in keys.iter().enumerate() {
                t.store(k, false, entry(i as u64), 1 + i % 5);
                assert!(t.len() <= entries);
            }
            let mut found = 0;
            for (i, k) in keys.iter().enumerate() {
                if let Some(e) = t.get(k, false) {
                    assert_eq!(e, entry(i as u64), "{entries} entries, key {i}");
                    found += 1;
                }
            }
            assert!(found > entries / 2, "{found} of {entries}");
        }
    }

    #[test]
    fn a_partial_table_keeps_counts_keys_apart_and_pins() {
        let mut t = Table::partial(1000, 60, true);
        let k = [5, 0, 0, 0];
        t.store(&k, false, entry(1), 1);
        assert_eq!(t.get(&k, true), None);
        t.store(&k, true, entry(2), 1);
        assert_eq!(t.get(&k, false), Some(entry(1)));
        assert_eq!(t.get(&k, true), Some(entry(2)));
        t.pin(&k, false, 1.0, 4);
        t.pin(&k, true, 1.0, 5);
        for round in 0..20 {
            t.age();
            for i in 0..400u64 {
                t.store(&[1000 + i + round * 400, 0, 0, 0], false, entry(i), 1 << 30);
            }
        }
        let e = t.get(&k, false).expect("pinned");
        assert_eq!((e.lo, e.hi, e.best), (1.0, 1.0, 4));
        let e = t.get(&k, true).expect("pinned");
        assert_eq!((e.lo, e.hi, e.best), (1.0, 1.0, 5));
    }

    #[test]
    fn a_partial_entry_keeps_fewer_words_as_the_table_grows() {
        // 131 places (Connect-4): three words whole, two at 2^23 entries.
        let whole = Table::new(1 << 23, 3, true);
        let part = Table::partial(1 << 23, 131, true);
        assert_eq!(whole.entry_bytes(), 33);
        assert_eq!(part.entry_bytes(), 25);
        assert_eq!(partial_words(131, 1 << 23), Some(2));
        // At the start of growth (256 buckets) it keeps 123 + 1 bits: two.
        assert_eq!(part.kw, 2);
        assert_eq!(partial_words(300, 1 << 23), None);
    }

    #[test]
    fn bounds_keep_their_bits() {
        let mut t = Table::new(8, 1, true);
        let e = Entry {
            lo: -0.0,
            hi: 0.0,
            best: 3,
        };
        t.store(&[1, 0, 0, 0], false, e, 1);
        let got = t.get(&[1, 0, 0, 0], false).unwrap();
        assert!(got.lo.is_sign_negative() && got.hi.is_sign_positive());
    }
}
