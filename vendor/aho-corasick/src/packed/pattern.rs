use core::{cmp, fmt, mem, u16, usize};

use alloc::{string::String, vec, vec::Vec};

use crate::packed::api::MatchKind;

/// The type used for representing a pattern identifier.
///
/// We don't use `usize` here because our packed searchers don't scale to
/// huge numbers of patterns, so we keep things a bit smaller.
pub type PatternID = u16;

/// A non-empty collection of non-empty patterns to search for.
///
/// This collection of patterns is what is passed around to both execute
/// searches and to construct the searchers themselves. Namely, this permits
/// searches to avoid copying all of the patterns, and allows us to keep only
/// one copy throughout all packed searchers.
///
/// Note that this collection is not a set. The same pattern can appear more
/// than once.
#[derive(Clone, Debug)]
pub struct Patterns {
    /// The match semantics supported by this collection of patterns.
    ///
    /// The match semantics determines the order of the iterator over patterns.
    /// For leftmost-first, patterns are provided in the same order as were
    /// provided by the caller. For leftmost-longest, patterns are provided in
    /// descending order of length, with ties broken by the order in which they
    /// were provided by the caller.
    kind: MatchKind,
    /// The collection of patterns, indexed by their identifier.
    by_id: Vec<Vec<u8>>,
    /// The order of patterns defined for iteration, given by pattern
    /// identifiers. The order of `by_id` and `order` is always the same for
    /// leftmost-first semantics, but may be different for leftmost-longest
    /// semantics.
    order: Vec<PatternID>,
    /// The length of the smallest pattern, in bytes.
    minimum_len: usize,
    /// The largest pattern identifier. This should always be equivalent to
    /// the number of patterns minus one in this collection.
    max_pattern_id: PatternID,
    /// The total number of pattern bytes across the entire collection. This
    /// is used for reporting total heap usage in constant time.
    total_pattern_bytes: usize,
}

impl Patterns {
    /// Create a new collection of patterns for the given match semantics. The
    /// ID of each pattern is the index of the pattern at which it occurs in
    /// the `by_id` slice.
    ///
    /// If any of the patterns in the slice given are empty, then this panics.
    /// Similarly, if the number of patterns given is zero, then this also
    /// panics.
    pub fn new() -> Patterns {
        Patterns {
            kind: MatchKind::default(),
            by_id: vec![],
            order: vec![],
            minimum_len: usize::MAX,
            max_pattern_id: 0,
            total_pattern_bytes: 0,
        }
    }

    /// Add a pattern to this collection.
    ///
    /// This panics if the pattern given is empty.
    pub fn add(&mut self, bytes: &[u8]) {
        assert!(!bytes.is_empty());
        assert!(self.by_id.len() <= u16::MAX as usize);

        let id = self.by_id.len() as u16;
        self.max_pattern_id = id;
        self.order.push(id);
        self.by_id.push(bytes.to_vec());
        self.minimum_len = cmp::min(self.minimum_len, bytes.len());
        self.total_pattern_bytes += bytes.len();
    }

    /// Set the match kind semantics for this collection of patterns.
    ///
    /// If the kind is not set, then the default is leftmost-first.
    pub fn set_match_kind(&mut self, kind: MatchKind) {
        self.kind = kind;
        match self.kind {
            MatchKind::LeftmostFirst => {
                self.order.sort();
            }
            MatchKind::LeftmostLongest => {
                let (order, by_id) = (&mut self.order, &mut self.by_id);
                order.sort_by(|&id1, &id2| {
                    by_id[id1 as usize]
                        .len()
                        .cmp(&by_id[id2 as usize].len())
                        .reverse()
                });
            }
        }
    }

    /// Return the number of patterns in this collection.
    ///
    /// This is guaranteed to be greater than zero.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Returns true if and only if this collection of patterns is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the approximate total amount of heap used by these patterns, in
    /// units of bytes.
    pub fn memory_usage(&self) -> usize {
        self.order.len() * mem::size_of::<PatternID>()
            + self.by_id.len() * mem::size_of::<Vec<u8>>()
            + self.total_pattern_bytes
    }

    /// Clears all heap memory associated with this collection of patterns and
    /// resets all state such that it is a valid empty collection.
    pub fn reset(&mut self) {
        self.kind = MatchKind::default();
        self.by_id.clear();
        self.order.clear();
        self.minimum_len = usize::MAX;
        self.max_pattern_id = 0;
    }

    /// Return the maximum pattern identifier in this collection. This can be
    /// useful in searchers for ensuring that the collection of patterns they
    /// are provided at search time and at build time have the same size.
    pub fn max_pattern_id(&self) -> PatternID {
        assert_eq!((self.max_pattern_id + 1) as usize, self.len());
        self.max_pattern_id
    }

    /// Returns the length, in bytes, of the smallest pattern.
    ///
    /// This is guaranteed to be at least one.
    pub fn minimum_len(&self) -> usize {
        self.minimum_len
    }

    /// Returns the match semantics used by these patterns.
    pub fn match_kind(&self) -> &MatchKind {
        &self.kind
    }

    /// Return the pattern with the given identifier. If such a pattern does
    /// not exist, then this panics.
    pub fn get(&self, id: PatternID) -> Pattern<'_> {
        Pattern(&self.by_id[id as usize])
    }

    /// Return the pattern with the given identifier without performing bounds
    /// checks.
    ///
    /// # Safety
    ///
    /// Callers must ensure that a pattern with the given identifier exists
    /// before using this method.
    #[cfg(all(feature = "std", target_arch = "x86_64"))]
    pub unsafe fn get_unchecked(&self, id: PatternID) -> Pattern<'_> {
        Pattern(self.by_id.get_unchecked(id as usize))
    }

    /// Return an iterator over all the patterns in this collection, in the
    /// order in which they should be matched.
    ///
    /// Specifically, in a naive multi-pattern matcher, the following is
    /// guaranteed to satisfy the match semantics of this collection of
    /// patterns:
    ///
    /// ```ignore
    /// for i in 0..haystack.len():
    ///   for p in patterns.iter():
    ///     if haystack[i..].starts_with(p.bytes()):
    ///       return Match(p.id(), i, i + p.bytes().len())
    /// ```
    ///
    /// Namely, among the patterns in a collection, if they are matched in
    /// the order provided by this iterator, then the result is guaranteed
    /// to satisfy the correct match semantics. (Either leftmost-first or
    /// leftmost-longest.)
    pub fn iter(&self) -> PatternIter<'_> {
        PatternIter { patterns: self, i: 0 }
    }
}

/// An iterator over the patterns in the `Patterns` collection.
///
/// The order of the patterns provided by this iterator is consistent with the
/// match semantics of the originating collection of patterns.
///
/// The lifetime `'p` corresponds to the lifetime of the collection of patterns
/// this is iterating over.
#[derive(Debug)]
pub struct PatternIter<'p> {
    patterns: &'p Patterns,
    i: usize,
}

impl<'p> Iterator for PatternIter<'p> {
    type Item = (PatternID, Pattern<'p>);

    fn next(&mut self) -> Option<(PatternID, Pattern<'p>)> {
        if self.i >= self.patterns.len() {
            return None;
        }
        let id = self.patterns.order[self.i];
        let p = self.patterns.get(id);
        self.i += 1;
        Some((id, p))
    }
}

/// A pattern that is used in packed searching.
#[derive(Clone)]
pub struct Pattern<'a>(&'a [u8]);

impl<'a> fmt::Debug for Pattern<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pattern")
            .field("lit", &String::from_utf8_lossy(&self.0))
            .finish()
    }
}

impl<'p> Pattern<'p> {
    /// Returns the length of this pattern, in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the bytes of this pattern.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the first `len` low nybbles from this pattern. If this pattern
    /// is shorter than `len`, then this panics.
    #[cfg(all(feature = "std", target_arch = "x86_64"))]
    pub fn low_nybbles(&self, len: usize) -> Vec<u8> {
        let mut nybs = vec![];
        for &b in self.bytes().iter().take(len) {
            nybs.push(b & 0xF);
        }
        nybs
    }

    /// Returns true if this pattern is a prefix of the given bytes.
    #[inline(always)]
    pub fn is_prefix(&self, bytes: &[u8]) -> bool {
        self.len() <= bytes.len() && self.equals(&bytes[..self.len()])
    }

    /// Returns true if and only if this pattern equals the given bytes.
    #[inline(always)]
    pub fn equals(&self, bytes: &[u8]) -> bool {
        // Why not just use memcmp for this? Well, memcmp requires calling out
        // to libc, and this routine is called in fairly hot code paths. Other
        // than just calling out to libc, it also seems to result in worse
        // codegen. By rolling our own memcpy in pure Rust, it seems to appear
        // more friendly to the optimizer.
        //
        // This results in an improvement in just about every benchmark. Some
        // smaller than others, but in some cases, up to 30% faster.

        let (x, y) = (self.bytes(), bytes);
        if x.len() != y.len() {
            return false;
        }
        // If we don't have enough bytes to do 4-byte at a time loads, then
        // fall back to the naive slow version.
        if x.len() < 4 {
            for (&b1, &b2) in x.iter().zip(y) {
                if b1 != b2 {
                    return false;
                }
            }
            return true;
        }
        // When we have 4 or more bytes to compare, then proceed in chunks of 4
        // at a time using unaligned loads.
        //
        // Also, why do 4 byte loads instead of, say, 8 byte loads? The reason
        // is that this particular version of memcmp is likely to be called
        // with tiny needles. That means that if we do 8 byte loads, then a
        // higher proportion of memcmp calls will use the slower variant above.
        // With that said, this is a hypothesis and is only loosely supported
        // by benchmarks. There's likely some improvement that could be made
        // here. The main thing here though is to optimize for latency, not
        // throughput.

        // SAFETY: Via the conditional above, we know that both `px` and `py`
        // have the same length, so `px < pxend` implies that `py < pyend`.
        // Thus, derefencing both `px` and `py` in the loop below is safe.
        //
        // Moreover, we set `pxend` and `pyend` to be 4 bytes before the actual
        // end of of `px` and `py`. Thus, the final dereference outside of the
        // loop is guaranteed to be valid. (The final comparison will overlap
        // with the last comparison done in the loop for lengths that aren't
        // multiples of four.)
        //
        // Finally, we needn't worry about alignment here, since we do
        // unaligned loads.
        unsafe {
            let (mut px, mut py) = (x.as_ptr(), y.as_ptr());
            let (pxend, pyend) = (px.add(x.len() - 4), py.add(y.len() - 4));
            while px < pxend {
                let vx = (px as *const u32).read_unaligned();
                let vy = (py as *const u32).read_unaligned();
                if vx != vy {
                    return false;
                }
                px = px.add(4);
                py = py.add(4);
            }
            let vx = (pxend as *const u32).read_unaligned();
            let vy = (pyend as *const u32).read_unaligned();
            vx == vy
        }
    }
}
