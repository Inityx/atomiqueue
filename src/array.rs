pub trait Array {
    type T;
    const SIZE: usize;
}

macro_rules! is_array {
    ($n:expr,$($ns:expr),+) => {
        is_array!($n);
        is_array!($($ns),*);
    };
    ($n:expr) => {
        impl<U> Array for [U;$n] {
            type T = U;
            const SIZE: usize = $n;
        }
    };
    () => {};
}

is_array!(
    1,2,3,4,5,6,7,8,
    9,10,11,12,13,14,15,16,
    17,18,19,20,21,22,23,24,
    25,26,27,28,29,30,31,32,
);
