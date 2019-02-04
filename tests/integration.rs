#![feature(cell_update)]

use atomiqueue::AtomiQueue;
use std::{
    cell::Cell,
    thread::{self, sleep},
    time::Duration,
    sync::Arc,
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

type Remote<'a> = &'a Cell<usize>;

struct Tracker<'a>(Remote<'a>);

impl<'a> Tracker<'a> {
    fn new(create: Remote<'a>, delete: Remote<'a>) -> Self {
        create.update(|x| x + 1);
        Tracker(delete)
    }
}

impl<'a> Drop for Tracker<'a> {
    fn drop(&mut self) { self.0.update(|x| x + 1); }
}

#[test]
fn correct_drop() {
    let (create, delete) = (Cell::<usize>::new(0), Cell::<usize>::new(0));

    let queue = AtomiQueue::<Tracker>::new();
    assert_eq!((0, 0), (create.get(), delete.get()));

    queue.extend(
        repeat_with(|| Tracker::new(&create, &delete)).take(atomiqueue::SIZE),
        |_| false,
    );
    assert_eq!(
        (atomiqueue::SIZE, 0),
        (create.get(), delete.get())
    );

    let (_, value) = queue.push(Tracker::new(&create, &delete)).expect_err("Queue should be full");
    assert_eq!(
        (atomiqueue::SIZE + 1, 0),
        (create.get(), delete.get())
    );

    drop(value);
    assert_eq!(
        (atomiqueue::SIZE + 1, 1),
        (create.get(), delete.get())
    );

    drop(queue.pop().expect("Should be only one popper").expect("Shouldn't be empty"));
    drop(queue.pop().expect("Should be only one popper").expect("Shouldn't be empty"));
    assert_eq!(
        (atomiqueue::SIZE + 1, 3),
        (create.get(), delete.get())
    );

    drop(queue);
    assert_eq!(
        (atomiqueue::SIZE + 1, atomiqueue::SIZE + 1),
        (create.get(), delete.get())
    );
}
