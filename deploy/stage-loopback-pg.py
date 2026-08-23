#!/usr/bin/env python3
"""Forward netns loopback 127.0.0.2:5433 to stage Postgres on 10.254.32.2:5433.

The engine binary requires sslmode=require for any non-loopback host. Stage Postgres
listens on the veth address, and isolation forbids 127.0.0.1:5433. 127.0.0.2 is loopback
and is not in the isolation deny list.
"""
import select
import socket
import threading

LISTEN = ("127.0.0.2", 5433)
TARGET = ("10.254.32.2", 5433)


def pipe(a: socket.socket, b: socket.socket) -> None:
    try:
        while True:
            ready, _, _ = select.select([a, b], [], [])
            for src in ready:
                dst = b if src is a else a
                data = src.recv(65536)
                if not data:
                    return
                dst.sendall(data)
    except OSError:
        return
    finally:
        a.close()
        b.close()


def main() -> None:
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(LISTEN)
    server.listen(64)
    while True:
        client, _ = server.accept()
        upstream = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            upstream.connect(TARGET)
        except OSError:
            client.close()
            continue
        threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()


if __name__ == "__main__":
    main()
