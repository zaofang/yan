def main() -> None:
    acc = 0
    i = 0
    while i < 1_000_000:
        acc += i % 7
        i += 1
    assert acc == 2999997


main()
