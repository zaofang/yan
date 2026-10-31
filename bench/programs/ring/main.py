def main() -> None:
    buf = [0] * 64
    head = 0
    length = 0
    total = 0
    for n in range(500_000):
        if length == 64:
            total += buf[head]
            head = (head + 1) % 64
            length -= 1
        buf[(head + length) % 64] = n
        length += 1
    assert total == 124967752080


main()
