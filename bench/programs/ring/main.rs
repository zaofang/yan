fn main() {
    let rounds: i64 = std::hint::black_box(500_000);
    let mut buf = [0i64; 64];
    let (mut head, mut len, mut total) = (0usize, 0usize, 0i64);
    for n in 0..rounds {
        if len == 64 {
            total += buf[head];
            head = (head + 1) % 64;
            len -= 1;
        }
        buf[(head + len) % 64] = n;
        len += 1;
    }
    assert_eq!(total, 124967752080);
}
