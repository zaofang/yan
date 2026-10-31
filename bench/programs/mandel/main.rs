fn bounded(cr: f64, ci: f64) -> bool {
    let (mut zr, mut zi) = (0.0f64, 0.0f64);
    for _ in 0..100 {
        let zr2 = zr * zr;
        let zi2 = zi * zi;
        if zr2 + zi2 > 4.0 {
            return false;
        }
        zi = zr * zi * 2.0 + ci;
        zr = zr2 - zi2 + cr;
    }
    true
}

fn main() {
    let step: f64 = std::hint::black_box(0.03);
    let mut count: i64 = 0;
    let mut ci = -1.5;
    for _ in 0..100 {
        let mut cr = -2.0;
        for _ in 0..100 {
            if bounded(cr, ci) {
                count += 1;
            }
            cr += step;
        }
        ci += step;
    }
    assert_eq!(count, 1740);
}
