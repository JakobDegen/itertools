use alloc::vec::Vec;
use std::fmt;
use std::iter::once;

use super::lazy_buffer::LazyBuffer;
use crate::size_hint::{self, SizeHint};

/// An iterator adaptor that iterates through all the `k`-permutations of the
/// elements from an iterator.
///
/// See [`.permutations()`](crate::Itertools::permutations) for
/// more information.
#[must_use = "iterator adaptors are lazy and do nothing unless consumed"]
pub struct Permutations<I: Iterator> {
    vals: LazyBuffer<I>,
    state: PermutationState,
}

impl<I> Clone for Permutations<I>
where
    I: Clone + Iterator,
    I::Item: Clone,
{
    clone_fields!(vals, state);
}

#[derive(Clone, Debug)]
enum PermutationState {
    /// No permutation generated yet.
    Start { k: usize },
    /// Values from the iterator are not fully loaded yet so `n` is still unknown.
    Buffered { k: usize, min_n: usize },
    /// All values from the iterator are known so `n` is known.
    Loaded(CompleteState),
    /// No permutation left to generate.
    End,
}

#[derive(Clone, Debug)]
enum CompleteState {
    Start {
        n: usize,
        k: usize,
    },
    Ongoing {
        indices: Vec<usize>,
        cycles: Vec<usize>,
    },
}

impl<I> fmt::Debug for Permutations<I>
where
    I: Iterator + fmt::Debug,
    I::Item: fmt::Debug,
{
    debug_fmt_fields!(Permutations, vals, state);
}

pub fn permutations<I: Iterator>(iter: I, k: usize) -> Permutations<I> {
    let mut vals = LazyBuffer::new(iter);

    if k == 0 {
        // Special case, yields single empty vec; `n` is irrelevant
        let state = PermutationState::Loaded(CompleteState::Start { n: 0, k: 0 });

        return Permutations { vals, state };
    }

    vals.prefill(k);
    let enough_vals = vals.len() == k;

    let state = if enough_vals {
        PermutationState::Start { k }
    } else {
        PermutationState::End
    };

    Permutations { vals, state }
}

impl<I> Iterator for Permutations<I>
where
    I: Iterator,
    I::Item: Clone,
{
    type Item = Vec<I::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        {
            let Self { vals, state } = self;
            match state {
                &mut PermutationState::Start { k } => {
                    *state = PermutationState::Buffered { k, min_n: k };
                }
                PermutationState::Buffered { ref k, min_n } => {
                    if vals.get_next() {
                        *min_n += 1;
                    } else {
                        let n = *min_n;
                        let prev_iteration_count = n - *k + 1;
                        let mut complete_state = CompleteState::Start { n, k: *k };

                        // Advance the complete-state iterator to the correct point
                        for _ in 0..(prev_iteration_count + 1) {
                            complete_state.advance();
                        }

                        *state = PermutationState::Loaded(complete_state);
                    }
                }
                PermutationState::Loaded(state) => {
                    state.advance();
                }
                PermutationState::End => {}
            };
        }
        let Self { vals, state } = &self;
        match state {
            PermutationState::Start { .. } => panic!("unexpected iterator state"),
            PermutationState::Buffered { ref k, min_n } => {
                let latest_idx = *min_n - 1;
                let indices = (0..(*k - 1)).chain(once(latest_idx));

                Some(indices.map(|i| vals[i].clone()).collect())
            }
            PermutationState::Loaded(CompleteState::Ongoing {
                ref indices,
                ref cycles,
            }) => {
                let k = cycles.len();
                Some(indices[0..k].iter().map(|&i| vals[i].clone()).collect())
            }
            PermutationState::Loaded(CompleteState::Start { .. }) | PermutationState::End => None,
        }
    }

    fn count(self) -> usize {
        fn from_complete(complete_state: CompleteState) -> usize {
            complete_state
                .remaining()
                .expect("Iterator count greater than usize::MAX")
        }

        let Permutations { vals, state } = self;
        match state {
            PermutationState::Start { k } => {
                let n = vals.count();
                let complete_state = CompleteState::Start { n, k };

                from_complete(complete_state)
            }
            PermutationState::Buffered { k, min_n } => {
                let prev_iteration_count = min_n - k + 1;
                let n = vals.count();
                let complete_state = CompleteState::Start { n, k };

                from_complete(complete_state) - prev_iteration_count
            }
            PermutationState::Loaded(state) => from_complete(state),
            PermutationState::End => 0,
        }
    }

    fn size_hint(&self) -> SizeHint {
        let at_start = |k| {
            // At the beginning, there are `n!/(n-k)!` items to come (see `remaining`) but `n` might be unknown.
            let (mut low, mut upp) = self.vals.size_hint();
            low = CompleteState::Start { n: low, k }
                .remaining()
                .unwrap_or(usize::MAX);
            upp = upp.and_then(|n| CompleteState::Start { n, k }.remaining());
            (low, upp)
        };
        match self.state {
            PermutationState::Start { k } => at_start(k),
            PermutationState::Buffered { k, min_n } => {
                // Same as `Start` minus the previously generated items.
                size_hint::sub_scalar(at_start(k), min_n - k + 1)
            }
            PermutationState::Loaded(ref state) => match state.remaining() {
                Some(count) => (count, Some(count)),
                None => (::std::usize::MAX, None),
            },
            PermutationState::End => (0, Some(0)),
        }
    }
}

fn advance(indices: &mut [usize], cycles: &mut [usize]) -> bool {
    let n = indices.len();
    let k = cycles.len();
    // NOTE: if `cycles` are only zeros, then we reached the last permutation.
    for i in (0..k).rev() {
        if cycles[i] == 0 {
            cycles[i] = n - i - 1;
            indices[i..].rotate_left(1);
        } else {
            let swap_index = n - cycles[i];
            indices.swap(i, swap_index);
            cycles[i] -= 1;
            return false;
        }
    }
    true
}

impl CompleteState {
    fn advance(&mut self) {
        match self {
            &mut CompleteState::Start { n, k } => {
                let indices = (0..n).collect();
                let cycles = ((n - k)..n).rev().collect();
                *self = CompleteState::Ongoing { cycles, indices };
            }
            CompleteState::Ongoing { indices, cycles } => {
                if advance(indices, cycles) {
                    *self = CompleteState::Start {
                        n: indices.len(),
                        k: cycles.len(),
                    };
                }
            }
        }
    }

    /// Returns the count of remaining permutations, or None if it would overflow.
    fn remaining(&self) -> Option<usize> {
        match self {
            &CompleteState::Start { n, k } => {
                if n < k {
                    return Some(0);
                }
                (n - k + 1..=n).try_fold(1usize, |acc, i| acc.checked_mul(i))
            }
            CompleteState::Ongoing { indices, cycles } => {
                cycles.iter().enumerate().try_fold(0usize, |acc, (i, &c)| {
                    acc.checked_mul(indices.len() - i)
                        .and_then(|count| count.checked_add(c))
                })
            }
        }
    }
}
