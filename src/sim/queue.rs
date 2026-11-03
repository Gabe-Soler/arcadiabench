//! Deterministic event queue: earliest time first, ties broken by insertion order.

use super::time::SimMs;
use std::collections::BTreeMap;

pub struct Queue<E> {
    items: BTreeMap<(SimMs, u64), E>,
    next_seq: u64,
}

impl<E> Default for Queue<E> {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
            next_seq: 0,
        }
    }
}

impl<E> Queue<E> {
    pub fn push(&mut self, at: SimMs, event: E) {
        self.items.insert((at, self.next_seq), event);
        self.next_seq += 1;
    }

    pub fn pop(&mut self) -> Option<(SimMs, E)> {
        self.items.pop_first().map(|((t, _), e)| (t, e))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_by_time_then_insertion() {
        let mut q = Queue::default();
        q.push(5, "c");
        q.push(1, "a");
        q.push(5, "d");
        q.push(1, "b");
        let order: Vec<_> = std::iter::from_fn(|| q.pop()).collect();
        assert_eq!(order, vec![(1, "a"), (1, "b"), (5, "c"), (5, "d")]);
        assert!(q.is_empty());
    }
}
