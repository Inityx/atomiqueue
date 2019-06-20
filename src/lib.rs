#![no_std]
#![feature(cell_update, const_fn, maybe_uninit)]

mod array;

use core::{
    cell::{Cell, UnsafeCell},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    mem::{MaybeUninit, needs_drop},
};

use array::Array;

#[derive(Debug, PartialEq)]
pub enum PushError { Full, Pending }

/// Fixed-size queue with atomic operations.
///
/// Designed for message passing in embedded interrupt service handlers.
pub struct AtomiQueue<A: Array>  {
    start: Cell<usize>,
    end: Cell<usize>,
    size: AtomicUsize,
    storage: UnsafeCell<MaybeUninit<A>>,
    push_pending: AtomicBool,
    pop_pending: AtomicBool,
}

unsafe impl<A> Sync for AtomiQueue<A> where A: Array, A::T: Send {}

impl<A: Array> Drop for AtomiQueue<A> {
    fn drop(&mut self) {
        if !needs_drop::<A::T>() { return; }

        // No need to be atomic because a dropped
        // AtomiQueue can have no references
        let mut size = self.len();

        while size > 0 {
            drop(unsafe { self.element_at(self.start.get()).read() });
            self.inc_start();
            size -= 1;
        }
    }
}

impl<A: Array> Default for AtomiQueue<A> {
    fn default() -> Self { AtomiQueue::new() }
}

impl<A: Array> AtomiQueue<A> {
    /// Creates a new, empty queue.
    pub const fn new() -> Self {
        AtomiQueue {
            start: Cell::new(0),
            end: Cell::new(0),
            size: AtomicUsize::new(0),
            storage: UnsafeCell::new(MaybeUninit::uninitialized()),
            push_pending: AtomicBool::new(false),
            pop_pending: AtomicBool::new(false),
        }
    }

    /// Tries to push a value onto the queue.
    ///
    /// This operation will return `Err` if there's already a
    /// `push` happening somewhere else on the same queue, or the queue is
    /// full; `Err` also returns the failed value.
    pub fn push(&self, value: A::T) -> Result<(), (PushError, A::T)> {
        if self.push_pending.compare_and_swap(false, true, Ordering::Acquire) {
            return Err((PushError::Pending, value));
        }

        if self.size.load(Ordering::Acquire) == A::SIZE {
            self.push_pending.store(false, Ordering::Release);
            return Err((PushError::Full, value));
        }

        unsafe { self.element_at(self.end.get()).write(value); }

        self.inc_end();

        self.size.fetch_add(1, Ordering::Release);
        self.push_pending.store(false, Ordering::Release);

        Ok(())
    }

    /// Pops a value off the queue.
    ///
    /// This operation will return `Err` if there's already a
    /// [`front`](AtomiQueue::front), ['back'](AtomiQueue::back) or `pop` happening
    /// somewhere else on the same queue.
    pub fn pop(&self) -> Result<Option<A::T>, ()> {
        if self.pop_pending.compare_and_swap(false, true, Ordering::Acquire) {
            return Err(());
        }

        if self.size.load(Ordering::Acquire) == 0 {
            self.pop_pending.store(false, Ordering::Release);
            return Ok(None);
        }

        let value = unsafe { self.element_at(self.start.get()).read() };

        self.inc_start();

        self.size.fetch_sub(1, Ordering::Release);
        self.pop_pending.store(false, Ordering::Release);

        Ok(Some(value))
    }

    /// Clones the oldest value in the queue
    ///
    /// This operation returns `Err` if there's already a
    /// `front`, [`back`](AtomiQueue::back), or [`pop`](AtomiQueue::pop)
    /// happening somewhere else on the same queue.
    pub fn front(&self) -> Result<Option<A::T>, ()>
    where A::T: Clone {
        if self.pop_pending.compare_and_swap(false, true, Ordering::Acquire) {
            return Err(());
        }

        if self.size.load(Ordering::Acquire) == 0 {
            self.pop_pending.store(false, Ordering::Release);
            return Ok(None);
        }

        let value = unsafe {
            self.element_at(self.start.get()).as_ref().unwrap()
        }.clone();

        // don't inc_start
        // don't size.fetch_sub(1)

        self.pop_pending.store(false, Ordering::Release);

        Ok(Some(value))
    }

    /// Clones the most recently pushed value in the queue.
    ///
    /// This operation returns `Err` if there's already a
    /// [`front`](AtomiQueue::front), `back`, or [`pop`](AtomiQueue::pop)
    /// happening somewhere else on the same queue.
    pub fn back(&self) -> Result<Option<A::T>, ()>
    where A::T: Clone {
        if self.pop_pending.compare_and_swap(false, true, Ordering::Acquire) {
            return Err(());
        }

        if self.size.load(Ordering::Acquire) == 0 {
            self.pop_pending.store(false, Ordering::Release);
            return Ok(None);
        }

        let value = unsafe {
            if self.end.get() == 0 {
                self.element_at(A::SIZE - 1).as_ref().unwrap()
            } else {
                self.element_at(self.end.get() - 1).as_ref().unwrap()
            }
        }.clone();

        self.pop_pending.store(false, Ordering::Release);

        Ok(Some(value))
    }

    unsafe fn element_at(&self, index: usize) -> *mut A::T {
        let storage: *mut MaybeUninit<_> = self.storage.get();
        let storage: &mut MaybeUninit<_> = storage.as_mut().unwrap();
        let elem_ptr = storage.as_mut_ptr() as *mut A::T;
        elem_ptr.add(index)
    }

    fn inc_start(&self) {
        self.start.update(|start| (start + 1) % A::SIZE);
    }

    fn inc_end(&self) {
        self.end.update(|end| (end + 1) % A::SIZE);
    }

    /// Gets the number of elements currently in the queue.
    ///
    /// A concurrent [`push`](AtomiQueue::push) or [`pop`](AtomiQueue::pop)
    /// will not be counted in the size until it has completed.
    pub fn len(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    /// Extends queue with the contents of an iterable.
    ///
    /// This operation [`push`](AtomiQueue::push)es all items from `iter`
    /// into the queue. If pushing fails, `retry` is called with the [`PushError`]
    /// and the failed value. Returning `Some(value)` attempts to push the
    /// returned value, and returning `None` moves on to the next value in `iter`.
    ///
    /// The `retry` functor can be used to implement strategies like exponential
    /// backoff by sleeping, or realtime prioritization by filtering.
    pub fn extend(
        &self,
        iter: impl IntoIterator<Item=A::T>,
        mut retry: impl FnMut(PushError, A::T) -> Option<A::T>,
    ) {
        for value in iter {
            let mut maybe_value = Some(value);

            while let Some(to_push) = maybe_value.take() {
                if let Err((e, failed)) = self.push(to_push) {
                    maybe_value = retry(e, failed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new() {
        let queue = AtomiQueue::<[i32;16]>::new();
        assert_eq!(Ok(None), queue.pop());
        assert_eq!(Ok(()), queue.push(1));
        assert_eq!(Ok(Some(1)), queue.front());
        assert_eq!(Ok(Some(1)), queue.back());
        assert_eq!(Ok(Some(1)), queue.pop());
    }

    #[test]
    fn fill() {
        let queue = AtomiQueue::<[i32;16]>::new();
        queue.extend((0..).take(SIZE), |_, _| None);

        assert_eq!(
            &[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15],
            unsafe { queue.storage.get().as_ref().unwrap().get_ref() },
        );
        assert_eq!(Err((PushError::Full, 1)), queue.push(1));
        assert_eq!(Ok(Some(0)), queue.front());
        assert_eq!(Ok(Some(0)), queue.pop());
        assert_eq!(Ok(Some(1)), queue.front());
        assert_eq!(Ok(Some(1)), queue.pop());
        assert_eq!(Ok(Some(2)), queue.front());
        assert_eq!(Ok(Some(2)), queue.pop());

        while let Ok(Some(x)) = queue.pop() { core::mem::drop(x); }

        queue.push(123).unwrap();
        queue.push(234).unwrap();
        assert_eq!(Ok(Some(123)), queue.pop());
        assert_eq!(Ok(Some(234)), queue.pop());
    }

    #[test]
    fn peeks() {
        let queue = AtomiQueue::<[i32;16]>::new();
        assert_eq!(Ok(None), queue.pop());
        assert_eq!(Ok(()), queue.push(1));
        assert_eq!(Ok(Some(1)), queue.front());
        assert_eq!(Ok(Some(1)), queue.back());
        assert_eq!(Ok(()), queue.push(2));
        assert_eq!(Ok(Some(1)), queue.front());
        assert_eq!(Ok(Some(2)), queue.back());
        assert_eq!(Ok(()), queue.push(3));
        assert_eq!(Ok(Some(1)), queue.front());
        assert_eq!(Ok(Some(3)), queue.back());
        assert_eq!(Ok(Some(1)), queue.pop());
        assert_eq!(Ok(Some(2)), queue.front());
        assert_eq!(Ok(Some(3)), queue.back());
    }
}
