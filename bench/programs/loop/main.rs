fn main() {
    let n: i64 = std::hint::black_box(1_000_000);
    let mut acc: i64 = 0;
    let mut i: i64 = 0;
    while i < n {
        acc += i.rem_euclid(7);
        i += 1;
    }
    assert_eq!(acc, 2999997);
}
