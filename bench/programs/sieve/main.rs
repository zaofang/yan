fn main() {
    let n: usize = std::hint::black_box(500_000);
    let mut xs = vec![true; n + 1];
    let mut count: i64 = 0;
    for p in 2..=n {
        if xs[p] {
            count += 1;
            let mut m = p * p;
            while m <= n {
                xs[m] = false;
                m += p;
            }
        }
    }
    assert_eq!(count, 41538);
}
