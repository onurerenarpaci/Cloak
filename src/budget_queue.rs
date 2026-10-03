use std::collections::{BinaryHeap, HashMap, HashSet};
use std::hash::Hash;

/// Stable handle that identifies an element.
/// (index in the slab + generation counter)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    idx: usize,
    gen: u64,
}

#[derive(Debug)]
struct Node<K, T> {
    key: K,
    val: T,
    gen: u64,
}

/// Max-priority queue keyed by `K` with lazy deletion.
///
/// Entries live in a slab; removed entries leave stale heap entries behind that are
/// skipped (and periodically compacted) when popping. A value index allows deleting
/// an entry by its value.
#[derive(Debug)]
pub struct HeapPQ<K: Ord, V> {
    heap: BinaryHeap<(K, usize, u64)>, // (key, idx, gen)
    pool: Vec<Option<Node<K, V>>>,     // slab
    gens: Vec<u64>,
    free: Vec<usize>,
    live: usize,
    stale_skips: usize,

    // map value -> set of handles having that value
    value_ix: HashMap<V, HashSet<Handle>>,
}

impl<K: Ord + Clone, V: Eq + Hash + Clone> Default for HeapPQ<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone, V: Eq + Hash + Clone> HeapPQ<K, V> {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            pool: Vec::new(),
            gens: Vec::new(),
            free: Vec::new(),
            live: 0,
            stale_skips: 0,
            value_ix: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: K, val: V) -> Handle {
        // reuse a free slot (bumping its generation) or grow the slab
        let (idx, gen) = if let Some(i) = self.free.pop() {
            self.gens[i] = self.gens[i].wrapping_add(1);
            (i, self.gens[i])
        } else {
            let i = self.pool.len();
            self.pool.push(None);
            self.gens.push(0);
            (i, 0)
        };
        self.pool[idx] = Some(Node { key: key.clone(), val: val.clone(), gen });
        let h = Handle { idx, gen };

        self.heap.push((key, idx, gen));
        self.live += 1;

        // index by value
        self.value_ix.entry(val).or_default().insert(h);
        h
    }

    /// Removes the live node in slot `idx` and recycles the slot.
    /// Callers are responsible for removing its handle from `value_ix`.
    fn take_alive(&mut self, idx: usize) -> (K, V) {
        let Node { key, val, gen: _ } = self.pool[idx].take().expect("node must be alive");

        // recycle slot + bump gen (invalidates any heap duplicates for this slot)
        self.free.push(idx);
        self.live -= 1;
        self.gens[idx] = self.gens[idx].wrapping_add(1);

        // NOTE: we do NOT need to remove the heap entry here; stale entries will be skipped lazily.
        (key, val)
    }

    /// Removes and returns the entry with the largest key.
    pub fn pop_max(&mut self) -> Option<(K, V)> {
        loop {
            let (_rk, idx, gen) = self.heap.pop()?;
            if self.is_live(idx, gen) {
                // capture value to clean the value index
                let v_clone = self.pool[idx].as_ref().unwrap().val.clone();
                let (k, v) = self.take_alive(idx);
                // remove the specific handle (idx,gen) from value_ix
                let h = Handle { idx, gen };
                if let Some(set) = self.value_ix.get_mut(&v_clone) {
                    set.remove(&h);
                    if set.is_empty() { self.value_ix.remove(&v_clone); }
                }
                return Some((k, v));
            } else {
                self.stale_skips += 1;
                self.maybe_rebuild();
            }
        }
    }

    fn maybe_rebuild(&mut self) {
        if self.stale_skips > self.heap.len() {
            self.rebuild_heap();
        }
    }
    fn rebuild_heap(&mut self) {
        let mut new_heap = BinaryHeap::with_capacity(self.live.max(1));
        for (idx, maybe_node) in self.pool.iter().enumerate() {
            if let Some(n) = maybe_node {
                new_heap.push((n.key.clone(), idx, n.gen));
            }
        }
        self.heap = new_heap;
        self.stale_skips = 0;
    }

    #[inline]
    fn is_live(&self, idx: usize, gen: u64) -> bool {
        idx < self.pool.len()
            && self.pool[idx].as_ref().is_some_and(|n| n.gen == gen)
    }

    /// Deletes the entry identified by `h`, if it is still live.
    pub fn delete(&mut self, h: Handle) -> Option<(K, V)> {
        if !self.is_live(h.idx, h.gen) { return None; }
        let v_clone = self.pool[h.idx].as_ref().unwrap().val.clone();
        let (k, v) = self.take_alive(h.idx);
        if let Some(set) = self.value_ix.get_mut(&v_clone) {
            set.remove(&h);
            if set.is_empty() { self.value_ix.remove(&v_clone); }
        }
        Some((k, v))
    }

    /// Peek at the entry with the largest key without removing it.
    /// O(1) expected, O(log n) if it needs to skip tombstones.
    pub fn peek_max(&mut self) -> Option<(&K, &V)> {
        self.clean_top();
        let &(_, idx, gen) = self.heap.peek()?;
        // safe to unwrap because clean_top guarantees it's live & matching gen
        let node = self.pool[idx].as_ref().expect("heap top must be live");
        debug_assert_eq!(node.gen, gen);
        Some((&node.key, &node.val))
    }

    /// Ensure heap top (if any) is a live entry; pop stale tops.
    fn clean_top(&mut self) {
        while let Some(&(_, idx, gen)) = self.heap.peek() {
            if self.is_live(idx, gen) {
                break;
            } else {
                self.heap.pop();
                self.stale_skips += 1;
            }
        }
        self.maybe_rebuild();
    }

    /// Removes and returns up to `x` entries with the largest keys, in descending key order.
    pub fn pop_first_x_max(&mut self, mut x: usize) -> Vec<(K, V)> {
        let mut out = Vec::with_capacity(x);
        while x > 0 {
            if let Some(ev) = self.pop_max() {
                out.push(ev);
                x -= 1;
            } else {
                break;
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool { self.live == 0 }
    pub fn len(&self) -> usize { self.live }

    /// Delete by value. Removes ONE matching element (arbitrary among duplicates).
    /// Returns (key, value) if any element with that value existed.
    pub fn delete_by_value(&mut self, v: &V) -> Option<(K, V)> {
        use std::collections::hash_map::Entry;
        loop {
            // First, take a handle out of the index without touching `self` elsewhere
            // to avoid borrow conflicts. This scope ends the mutable borrow of `value_ix`.
            let maybe_handle = match self.value_ix.entry(v.clone()) {
                Entry::Occupied(mut occ) => {
                    if let Some(&h) = occ.get().iter().next() {
                        let set = occ.get_mut();
                        set.remove(&h);
                        if set.is_empty() { occ.remove_entry(); }
                        Some(h)
                    } else {
                        // Defensive: empty set under occupied entry; remove it.
                        occ.remove_entry();
                        None
                    }
                }
                Entry::Vacant(_) => None,
            };

            let h = maybe_handle?;

            // Now we can freely borrow `self` to check liveness and delete
            if self.is_live(h.idx, h.gen) {
                let (k, val) = self.take_alive(h.idx);
                return Some((k, val));
            } else {
                // Stale handle cleaned from index; try again if more remain
                continue;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_max_heap_ops() {
        let mut pq = HeapPQ::<i32, &'static str>::new();
        let h5 = pq.insert(5, "five");
        let _h2 = pq.insert(2, "two");
        let _h8 = pq.insert(8, "eight");
        let _h3 = pq.insert(3, "three");

        assert_eq!(*pq.peek_max().unwrap().0, 8);

        // delete arbitrary (O(1) lazy)
        let del = pq.delete(h5).unwrap();
        assert_eq!(del.0, 5);

        // pops in order
        assert_eq!(pq.pop_max().unwrap().0, 8);
        assert_eq!(pq.pop_max().unwrap().0, 3);
        assert_eq!(pq.pop_max().unwrap().0, 2);
        assert!(pq.pop_max().is_none());
        assert!(pq.is_empty());
    }

    #[test]
    fn batch_pop() {
        let mut pq = HeapPQ::<i32, i32>::new();
        for i in (0..20).rev() {
            pq.insert(i, i);
        }
        let first7 = pq.pop_first_x_max(7);
        assert_eq!(first7.iter().map(|(k, _)| *k).collect::<Vec<_>>(), (13..20).rev().collect::<Vec<_>>());
        assert_eq!(pq.len(), 13);
    }

    #[test]
    fn tombstones_and_reuse_are_safe() {
        let mut pq = HeapPQ::<i32, i32>::new();
        let h1 = pq.insert(1, 10);
        let _h2 = pq.insert(2, 20);
        let _ = pq.delete(h1).unwrap();          // tombstone
        let h1b = pq.insert(1, 100);             // reuse same key, new slot or new gen
        // Pop should yield 2, then 1 (new); never return deleted (old) 1.
        assert_eq!(pq.pop_max().unwrap().1, 20);
        assert_eq!(pq.pop_max().unwrap().1, 100);
        assert!(pq.pop_max().is_none());

        // Deleting already-deleted handle returns None
        assert!(pq.delete(h1).is_none());
        // Deleting current handle works
        assert!(pq.delete(h1b).is_none()); // already popped
    }

    #[test]
    fn delete_by_value_basic() {
        let mut pq = HeapPQ::<i32, &'static str>::new();
        pq.insert(5, "five");
        pq.insert(2, "two");
        pq.insert(8, "eight");
        pq.insert(3, "three");

        // Delete existing value
        let result = pq.delete_by_value(&"five");
        assert!(result.is_some());
        let (key, val) = result.unwrap();
        assert_eq!(key, 5);
        assert_eq!(val, "five");
        assert_eq!(pq.len(), 3);

        // Try to delete the same value again - should return None
        assert!(pq.delete_by_value(&"five").is_none());

        // Delete non-existent value
        assert!(pq.delete_by_value(&"nonexistent").is_none());

        // Remaining elements should still work correctly
        assert_eq!(pq.pop_max().unwrap(), (8, "eight"));
        assert_eq!(pq.pop_max().unwrap(), (3, "three"));
        assert_eq!(pq.pop_max().unwrap(), (2, "two"));
        assert!(pq.is_empty());
    }

    #[test]
    fn delete_by_value_with_duplicates() {
        let mut pq = HeapPQ::<i32, i32>::new();
        pq.insert(1, 100);  // same value 100
        pq.insert(2, 100);  // same value 100
        pq.insert(3, 100);  // same value 100
        pq.insert(4, 200);  // different value

        assert_eq!(pq.len(), 4);

        // Delete one instance of value 100 - should remove one arbitrary element
        let result = pq.delete_by_value(&100);
        assert!(result.is_some());
        let (key, val) = result.unwrap();
        assert_eq!(val, 100);
        assert!([1, 2, 3].contains(&key)); // could be any of the three
        assert_eq!(pq.len(), 3);

        // Delete another instance of value 100
        let result = pq.delete_by_value(&100);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, 100);
        assert_eq!(pq.len(), 2);

        // Delete third instance of value 100
        let result = pq.delete_by_value(&100);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, 100);
        assert_eq!(pq.len(), 1);

        // Now no more instances of 100 should exist
        assert!(pq.delete_by_value(&100).is_none());

        // Value 200 should still be there
        let result = pq.delete_by_value(&200);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), (4, 200));
        assert!(pq.is_empty());
    }

    #[test]
    fn delete_by_value_with_stale_handles() {
        let mut pq = HeapPQ::<i32, i32>::new();
        let h1 = pq.insert(1, 100);
        let _h2 = pq.insert(2, 100);
        pq.insert(3, 200);

        // Delete one handle directly, making it stale in value_ix
        let result = pq.delete(h1);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), (1, 100));
        assert_eq!(pq.len(), 2);

        // Now delete_by_value should skip the stale handle and find the live one
        let result = pq.delete_by_value(&100);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), (2, 100));
        assert_eq!(pq.len(), 1);

        // No more instances of 100
        assert!(pq.delete_by_value(&100).is_none());

        // Value 200 should still be accessible
        assert_eq!(pq.pop_max().unwrap(), (3, 200));
        assert!(pq.is_empty());
    }

    #[test]
    fn delete_by_value_empty_heap() {
        let mut pq = HeapPQ::<i32, &'static str>::new();
        assert!(pq.delete_by_value(&"anything").is_none());
        assert!(pq.is_empty());
    }

    #[test]
    fn delete_by_value_after_pop() {
        let mut pq = HeapPQ::<i32, &'static str>::new();
        pq.insert(1, "one");
        pq.insert(2, "two");

        // Pop maximum
        assert_eq!(pq.pop_max().unwrap(), (2, "two"));

        // Try to delete the popped value
        assert!(pq.delete_by_value(&"two").is_none());

        // Delete remaining value
        let result = pq.delete_by_value(&"one");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), (1, "one"));
        assert!(pq.is_empty());
    }

    #[test]
    fn delete_by_value_maintains_heap_property() {
        let mut pq = HeapPQ::<i32, i32>::new();
        pq.insert(10, 1000);
        pq.insert(5, 500);
        pq.insert(15, 1500);
        pq.insert(3, 300);
        pq.insert(7, 700);

        // Delete middle value
        assert!(pq.delete_by_value(&500).is_some());

        // Remaining elements should still pop in order
        let mut results = Vec::new();
        while let Some((key, _)) = pq.pop_max() {
            results.push(key);
        }
        assert_eq!(results, vec![15, 10, 7, 3]);
    }
}
