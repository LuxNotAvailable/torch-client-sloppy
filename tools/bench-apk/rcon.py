"""Sends commands to the bench server over RCON (localhost only).

    python -I tools/bench-apk/rcon.py "time set noon" "weather clear"

The port and password come from target/bench-server/server.properties.
"""

import pathlib
import socket
import struct
import sys

PROPS = pathlib.Path(__file__).resolve().parents[2] / "target" / "bench-server" / "server.properties"


def props():
    out = {}
    for line in PROPS.read_text().splitlines():
        if "=" in line and not line.startswith("#"):
            k, v = line.split("=", 1)
            out[k.strip()] = v.strip()
    return out


def packet(sock, rid, kind, body):
    data = struct.pack("<ii", rid, kind) + body.encode() + b"\0\0"
    sock.sendall(struct.pack("<i", len(data)) + data)
    size = struct.unpack("<i", sock.recv(4))[0]
    reply = b""
    while len(reply) < size:
        reply += sock.recv(size - len(reply))
    rid_back = struct.unpack("<i", reply[:4])[0]
    return rid_back, reply[8:-2].decode(errors="replace")


def main():
    p = props()
    with socket.create_connection(("127.0.0.1", int(p["rcon.port"])), timeout=10) as s:
        if packet(s, 1, 3, p["rcon.password"])[0] == -1:
            sys.exit("rcon: wrong password")
        for i, command in enumerate(sys.argv[1:], start=2):
            print(f"> {command}: {packet(s, i, 2, command)[1]}")


if __name__ == "__main__":
    main()
