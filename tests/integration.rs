#![feature(cell_update)]

mod tracker;

use atomiqueue::AtomiQueue;
use std::{
    thread::{self, sleep},
    time::Duration,
    iter::repeat_with,
};
use rand::random;

const SIZE: usize = 16;
fn random_sleep(mult: u64) {
    sleep(Duration::from_micros(
        (random::<u64>() % 1024) * mult
    ));
}

#[test]
fn parallel_fuzz() {
    static QUEUE: AtomiQueue<i32, SIZE> = AtomiQueue::<i32, SIZE>::new();

    for _ in 0..50 {
        let prod_a = thread::spawn(move ||
            QUEUE.extend(
                (0..(SIZE*2)).map(|x| { random_sleep(1); x }),
                |_, x| { random_sleep(1); Some(x) },
            )
        );

        let prod_b = thread::spawn(move ||
            QUEUE.extend(
                ((SIZE*2)..(SIZE*4)).map(|x| { random_sleep(1); x }),
                |_, x| { random_sleep(1); Some(x) },
            )
        );

        let mut vec = (0..(SIZE*4))
            .map(|_| loop {
                random_sleep(3);
                if let Ok(Some(value)) = QUEUE.pop() { break value; }
            })
            .collect::<Vec<_>>();

        prod_a.join().unwrap();
        prod_b.join().unwrap();

        vec.sort();
        assert_eq!((0..64).collect::<Vec<_>>(), vec);
        assert_eq!(Ok(None), QUEUE.pop());
    }
}

#[test]
fn correct_drop() {
    let tracker = tracker::Tracker::new();

    // init
    let queue = AtomiQueue::<tracker::Beacon>::new();
    assert_eq!((0, 0), tracker.created_deleted(),);

    // fill
    queue.extend(
        repeat_with(|| tracker.new_beacon()).take(SIZE),
        |_, _| None,
    );
    assert_eq!((SIZE, 0), tracker.created_deleted());

    // fail push
    let (_, value) = queue.push(tracker.new_beacon()).expect_err("Queue should be full");
    assert_eq!((SIZE + 1, 0), tracker.created_deleted());

    // drop failed
    drop(value);
    assert_eq!((SIZE + 1, 1), tracker.created_deleted());

    // pop and drop 2
    drop(queue.pop().expect("Should be only one popper").expect("Shouldn't be empty"));
    drop(queue.pop().expect("Should be only one popper").expect("Shouldn't be empty"));
    assert_eq!((SIZE + 1, 3), tracker.created_deleted());

    // peek and drop 2
    drop(queue.front().expect("Should be only one peeker").expect("Shouldn't be empty"));
    drop(queue.back().expect("Should be only one peeker").expect("Shouldn't be empty"));
    assert_eq!((SIZE + 3, 5), tracker.created_deleted());

    // drop all
    drop(queue);
    assert_eq!((SIZE + 3, SIZE + 3), tracker.created_deleted());
}
