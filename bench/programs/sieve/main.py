def main() -> None:
    n = 10_000
    xs = [True] * (n + 1)
    count = 0
    for p in range(2, n + 1):
        if xs[p]:
            count += 1
            for m in range(p * p, n + 1, p):
                xs[m] = False
    assert count == 1229


main()
