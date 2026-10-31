def steps(n: int) -> int:
    acc = 0
    while n != 1:
        n = n // 2 if n % 2 == 0 else 3 * n + 1
        acc += 1
    return acc


def main() -> None:
    total = 0
    for n in range(1, 10_001):
        total += steps(n)
    assert total == 849666


main()
