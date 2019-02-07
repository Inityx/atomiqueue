use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Beacon<'a> {
    create: &'a AtomicUsize,
    delete: &'a AtomicUsize,
}

impl<'a> Beacon<'a> {
    fn new(
        create: &'a AtomicUsize,
        delete: &'a AtomicUsize,
    ) -> Self {
        create.fetch_add(1, Ordering::Relaxed);

        Beacon {
            create,
            delete,
        }
    }
}

impl<'a> Clone for Beacon<'a> {
    fn clone(&self) -> Self { Beacon::new(self.create, self.delete) }
}

impl<'a> Drop for Beacon<'a> {
    fn drop(&mut self) { self.delete.fetch_add(1, Ordering::Relaxed); }
}

#[derive(Default)]
pub struct Tracker {
    create: AtomicUsize,
    delete: AtomicUsize,
}

impl Tracker {
    pub fn new() -> Self { Tracker::default() }

    pub fn new_beacon<'a>(&'a self) -> Beacon<'a> {
        Beacon::new(&self.create, &self.delete)
    }

    pub fn created_deleted(&self) -> (usize, usize) {
        (
            self.create.load(Ordering::Acquire),
            self.delete.load(Ordering::Acquire),
        )
    }
}
