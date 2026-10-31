def bounded(cr: float, ci: float) -> bool:
    zr = 0.0
    zi = 0.0
    for _ in range(100):
        zr2 = zr * zr
        zi2 = zi * zi
        if zr2 + zi2 > 4.0:
            return False
        zi = zr * zi * 2.0 + ci
        zr = zr2 - zi2 + cr
    return True


def main() -> None:
    count = 0
    ci = -1.5
    for _ in range(100):
        cr = -2.0
        for _ in range(100):
            if bounded(cr, ci):
                count += 1
            cr += 0.03
        ci += 0.03
    assert count == 1740


main()
