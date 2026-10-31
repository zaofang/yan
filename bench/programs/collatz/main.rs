fn steps(mut n: i64) -> i64 {
    let mut acc: i64 = 0;
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        acc += 1;
    }
    acc
}

fn main() {
    let limit: i64 = std::hint::black_box(10_000);
    let total: i64 = (1..=limit).map(steps).sum();
    assert_eq!(total, 849666);
}
