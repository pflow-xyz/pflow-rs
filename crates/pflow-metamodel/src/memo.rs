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
    /// Words of key compared.
    kw: usize,
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

/// Where a key's hash points: its two buckets' first slots, and its tag.
struct Probe {
    buckets: [usize; 2],
    tag: u8,
}

impl Table {
    /// A table of at most `entries`, comparing `kw` words of key.
    pub fn new(entries: usize, kw: usize, compact: bool) -> Self {
        assert!((1..=KEY).contains(&kw));
        let entries = entries.max(1);
        let ways = WAYS.min(entries);
        let most = entries / ways;
        let buckets = most.min(START / ways).max(1);
        let stride = kw + if compact { 1 } else { 3 };
        Self {
            kw,
            compact,
            stride,
            slots: vec![0; buckets * ways * stride],
            tags: vec![0; buckets * ways],
            buckets,
            most,
            ways,
            len: 0,
            generation: 1,
            dict: vec![],
        }
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Bytes one entry takes, its tag included.
    pub fn entry_bytes(&self) -> usize {
        self.stride * 8 + 1
    }

    /// Bytes the table takes now.
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
    }

    fn probe(&self, k: &Key) -> Probe {
        let h = mix(k);
        let h2 = h.rotate_left(32).wrapping_mul(0x94D0_49BB_1331_11EB);
        Probe {
            buckets: [h, h2]
                .map(|x| ((x as u128 * self.buckets as u128) >> 64) as usize * self.ways),
            tag: (h2 >> 57) as u8 | 0x80,
        }
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

    /// Whether slot `i`, whose tag matches, holds `k`.
    fn holds(&self, i: usize, k: &Key, engine: u64) -> bool {
        let s = &self.slots[i * self.stride..i * self.stride + self.kw + 1];
        s[..self.kw] == k[..self.kw] && s[self.kw] & COUNTS == engine
    }

    /// The slot holding `k` (`Ok`), or else the one `k` would take (`Err`):
    /// an empty slot of its buckets, or failing that the weakest, an entry
    /// of an earlier [`Table::age`] first, then the one found with the
    /// least work. Only a store needs the second; a lookup passes `choose`
    /// false and gets `Err(0)` for a key not held.
    ///
    /// A key goes to its second bucket only when its first is full, and a
    /// slot, once taken, stays taken until the table grows (which puts
    /// every entry back by the same rule): so a first bucket with an empty
    /// slot means the key is not in the second.
    fn locate(&self, k: &Key, counts: bool, choose: bool) -> Slot {
        let p = self.probe(k);
        let engine = Self::engine(counts);
        for b in p.buckets {
            let mut room = None;
            for (j, &t) in self.tags[b..b + self.ways].iter().enumerate() {
                if t == p.tag && self.holds(b + j, k, engine) {
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
            let current = (m >> GEN_SHIFT & GEN_MASK == self.generation) as u64;
            current << 8 | (m >> WORK_SHIFT & 0xFF)
        };
        let ways = self.ways;
        let all = p.buckets.into_iter().flat_map(|b| b..b + ways);
        Err(all.min_by_key(|&i| rank(i)).unwrap())
    }

    pub fn get(&self, k: &Key, counts: bool) -> Option<Entry> {
        self.locate(k, counts, false).ok().map(|i| self.read(i))
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
        let mut at = self.locate(k, counts, true);
        let (mut e, mut work) = (e, (usize::BITS - nodes.leading_zeros()) as u64);
        if let Ok(i) = at {
            let old = self.read(i);
            e.lo = e.lo.max(old.lo);
            e.hi = e.hi.min(old.hi);
            work = work.max(self.meta(i) >> WORK_SHIFT & 0xFF);
        }
        let mut meta = e.best as u64
            | work << WORK_SHIFT
            | self.generation << GEN_SHIFT
            | Self::engine(counts);
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
            at = self.locate(k, counts, true);
        }
        self.put(at, k, meta, wide);
    }

    /// Writes an entry where [`Table::locate`] said.
    fn put(&mut self, at: Slot, k: &Key, meta: u64, wide: [u64; 2]) {
        let (Ok(i) | Err(i)) = at;
        if self.tags[i] == 0 {
            self.len += 1;
        }
        self.tags[i] = self.probe(k).tag;
        let s = &mut self.slots[i * self.stride..(i + 1) * self.stride];
        s[..self.kw].copy_from_slice(&k[..self.kw]);
        s[self.kw] = meta;
        if !self.compact {
            s[self.kw + 1..].copy_from_slice(&wide);
        }
    }

    /// Doubles the buckets, up to the limit, and moves every entry over.
    fn grow(&mut self) {
        let old = std::mem::take(&mut self.slots);
        let tags = std::mem::take(&mut self.tags);
        self.buckets = (self.buckets * 2).min(self.most);
        self.slots = vec![0; self.buckets * self.ways * self.stride];
        self.tags = vec![0; self.buckets * self.ways];
        self.len = 0;
        for (s, _) in old.chunks_exact(self.stride).zip(tags).filter(|x| x.1 != 0) {
            let meta = s[self.kw];
            let mut k = [0; KEY];
            k[..self.kw].copy_from_slice(&s[..self.kw]);
            let wide = if self.compact {
                [0; 2]
            } else {
                [s[self.kw + 1], s[self.kw + 2]]
            };
            let at = self.locate(&k, meta & COUNTS != 0, true);
            self.put(at, &k, meta, wide);
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
