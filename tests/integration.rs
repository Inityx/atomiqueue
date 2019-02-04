#![feature(cell_update)]

use atomiqueue::AtomiQueue;
use std::{
    thread::{self, sleep},
    time::Duration,
    sync::{Arc, atomic::{AtomicUsize, Ordering::Relaxed}},
    iter::repeat_with,
};
use rand::random;

fn random_sleep(mult: u64) {
    sleep(Duration::from_micros(
        (random::<u64>() % 1024) * mult
    ));
}

#[test]
fn parallel_fuzz() {
    for _ in 0..50 {
        let queue = Arc::new(AtomiQueue::<i32>::new());

        let queue_handle = Arc::clone(&queue);
        let prod_a = thread::spawn(move ||
            queue_handle.extend(
                (0..32).map(|x| { random_sleep(1); x }),
                |_| { random_sleep(1); true },
            )
        );

        let queue_handle = Arc::clone(&queue);
        let prod_b = thread::spawn(move ||
            queue_handle.extend(
                (32..64).map(|x| { random_sleep(1); x }),
                |_| { random_sleep(1); true },
            )
        );

        let mut vec = (0..64)
            .map(|_| loop {
                random_sleep(3);
                if let Ok(Some(value)) = queue.pop() { break value; }
            })
            .collect::<Vec<_>>();

        prod_a.join().unwrap();
        prod_b.join().unwrap();


        vec.sort();
        assert_eq!((0..64).collect::<Vec<_>>(), vec);
    }
}

type Remote<'a> = &'a AtomicUsize;

struct Tracker<'a> {
    create: Remote<'a>,
    delete: Remote<'a>,
}

impl<'a> Tracker<'a> {
    fn new(create: Remote<'a>, delete: Remote<'a>) -> Self {
        create.fetch_add(1, Relaxed);
        Tracker { create, delete }
    }
}

impl<'a> Clone for Tracker<'a> {
    fn clone(&self) -> Self { Tracker::new(self.create, self.delete) }
}

impl<'a> Drop for Tracker<'a> {
    fn drop(&mut self) { self.delete.fetch_add(1, Relaxed); }
}

#[test]
fn correct_drop() {
    let (create, delete) = (AtomicUsize::new(0), AtomicUsize::new(0));

    // init
    let queue = AtomiQueue::<Tracker>::new();
    assert_eq!((0, 0), (create.load(Relaxed), delete.load(Relaxed)));

    // fill
    queue.extend(
        repeat_with(|| Tracker::new(&create, &delete)).take(atomiqueue::SIZE),
        |_| false,
    );
    assert_eq!(
        (atomiqueue::SIZE, 0),
        (create.load(Relaxed), delete.load(Relaxed))
    );

    // fail push
    let (_, value) = queue.push(Tracker::new(&create, &delete)).expect_err("Queue should be full");
    assert_eq!(
        (atomiqueue::SIZE + 1, 0),
        (create.load(Relaxed), delete.load(Relaxed))
    );

    // drop failed
    drop(value);
    assert_eq!(
        (atomiqueue::SIZE + 1, 1),
        (create.load(Relaxed), delete.load(Relaxed))
    );

    // pop and drop 2
    drop(queue.pop().expect("Should be only one popper").expect("Shouldn't be empty"));
    drop(queue.pop().expect("Should be only one popper").expect("Shouldn't be empty"));
    assert_eq!(
        (atomiqueue::SIZE + 1, 3),
        (create.load(Relaxed), delete.load(Relaxed))
    );

    // peek and drop 2
    drop(queue.peek().expect("Should be only one peeker").expect("Shouldn't be empty"));
    drop(queue.peek().expect("Should be only one peeker").expect("Shouldn't be empty"));
    assert_eq!(
        (atomiqueue::SIZE + 3, 5),
        (create.load(Relaxed), delete.load(Relaxed))
    );

    // drop all
    drop(queue);
    assert_eq!(
        (atomiqueue::SIZE + 3, atomiqueue::SIZE + 3),
        (create.load(Relaxed), delete.load(Relaxed))
    );
}
