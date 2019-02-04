#![no_std]
#![feature(cell_update, const_fn, maybe_uninit)]

use core::{
    cell::{Cell, UnsafeCell},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    mem::{MaybeUninit, needs_drop},
};

#[derive(Debug, PartialEq)]
pub enum PushError { Full, Pending }

pub const SIZE: usize = 16;

/// Fixed-size queue with atomic operations
///
/// Designed for message passing in embedded interrupt service handlers.
pub struct AtomiQueue<T>  {
    start: Cell<usize>,
    end: Cell<usize>,
    size: AtomicUsize,
    storage: UnsafeCell<MaybeUninit<[T; SIZE]>>,
    push_pending: AtomicBool,
    pop_pending: AtomicBool,
}

unsafe impl<T> Sync for AtomiQueue<T> {}

impl<T> Drop for AtomiQueue<T> {
    fn drop(&mut self) {
        if needs_drop::<T>() == false { return; }

        // No need to be atomic because a dropped
        // AtomiQueue can have no references
        while let Ok(Some(value)) = self.pop() {
            drop(value);
        }
    }
}

impl<T> AtomiQueue<T> {
    /// Creates a new, empty AtomiQueue
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

    /// Push a value onto the queue
    ///
    /// Returns `Err((PushError::Pending, T))` if there's already a
    /// push happening somewhere else, and `Err((PushError::Full, T))`
    /// if the queue is full.
    pub fn push(&self, value: T) -> Result<(), (PushError, T)> {
        if self.push_pending.compare_and_swap(false, true, Ordering::Acquire) {
            return Err((PushError::Pending, value));
        }

        if self.size.load(Ordering::Acquire) == SIZE {
            self.push_pending.store(false, Ordering::Release);
            return Err((PushError::Full, value));
        }

        unsafe { self.element_at(self.end.get()).write(value); }

        self.inc_end();

        self.size.fetch_add(1, Ordering::Release);
        self.push_pending.store(false, Ordering::Release);

        Ok(())
    }

    /// Pop a value off the queue
    ///
    /// Returns `Err(())` if there's already a pop happening
    /// somewhere else, and `Ok(None)` if the queue is empty.
    pub fn pop(&self) -> Result<Option<T>, ()> {
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

    unsafe fn element_at(&self, index: usize) -> *mut T {
        let storage: *mut MaybeUninit<_> = self.storage.get();
        let storage: &mut MaybeUninit<_> = storage.as_mut().unwrap();
        let elem_ptr = storage.as_mut_ptr() as *mut T;
        elem_ptr.add(index)
    }

    fn inc_start(&self) {
        self.start.update(|start|(start + 1) % SIZE);
    }

    fn inc_end(&self) {
        self.end.update(|end| (end + 1) % SIZE);
    }

    /// Extend queue with the contents of an iterable.
    ///
    /// Move all items from `iter` into the queue, retrying based
    /// on `retry` when pushing results in an error. If `retry`
    /// returns `false`, the value that failed to push is dropped.
    ///
    /// `retry` can also be used to implement strategies like
    /// exponential backoff using delay/sleep.
    pub fn extend(
        &self,
        iter: impl IntoIterator<Item=T>,
        mut retry: impl FnMut(PushError) -> bool,
    ) {
        for value in iter {
            let mut value = Some(value);
            while let Err((e, value_again)) = self.push(value.take().unwrap()) {
                if !retry(e) { break; }
                value = Some(value_again);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new() {
        let queue = AtomiQueue::<i32>::new();
        assert_eq!(Ok(None), queue.pop());
        assert_eq!(Ok(()), queue.push(1));
        assert_eq!(Ok(Some(1)), queue.pop());
    }

    #[test]
    fn fill() {
        let queue = AtomiQueue::<i32>::new();
        queue.extend((0..).take(SIZE), |_| false);
        assert_eq!(
            &[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15],
            unsafe { queue.storage.get().as_ref().unwrap().get_ref() },
        );
        assert_eq!(Ok(Some(0)), queue.pop());
        assert_eq!(Ok(Some(1)), queue.pop());
        assert_eq!(Ok(Some(2)), queue.pop());
    }

    #[test]
    fn overfill() {
        let queue = AtomiQueue::<i32>::new();
        queue.extend((0..).take(SIZE), |_| false);
        assert_eq!(Err((PushError::Full, 1)), queue.push(1));
        assert_eq!(Ok(Some(0)), queue.pop());
        assert!(queue.push(1).is_ok());
    }
}
