#!/usr/bin/env python3
"""Lights a virtio-gpu screen that QEMU keeps dark.

QEMU reports a virtio-gpu's second connector as connected only once a
display asks it for a size. Without a window (`-display none`), the VM's
VNC server on that head is the display: this asks it, as a VNC client
would, for a desktop of the given size (RFB's SetDesktopSize), which QEMU
passes to the GPU, and the VM sees a screen plugged in.

    light-head.py SOCKET WIDTH HEIGHT

SOCKET is the VNC server's Unix socket (`-vnc unix:SOCKET,display=…,head=1`).
Only the standard library: it runs on any host with Python 3.
"""

import socket
import struct
import sys

# RFB pseudo-encoding: the client understands desktop size changes, and
# may ask for one.
EXTENDED_DESKTOP_SIZE = -308
SET_ENCODINGS = 2
SET_DESKTOP_SIZE = 251


def read_exactly(sock, n):
    data = b""
    while len(data) < n:
        chunk = sock.recv(n - len(data))
        if not chunk:
            raise ConnectionError("the VNC server closed the connection")
        data += chunk
    return data


def light(path, width, height):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(5)
        sock.connect(path)
        version = read_exactly(sock, 12)
        if not version.startswith(b"RFB 003."):
            raise ConnectionError(f"not a VNC server: {version!r}")
        sock.sendall(b"RFB 003.008\n")
        (count,) = struct.unpack("B", read_exactly(sock, 1))
        if count == 0:
            (length,) = struct.unpack(">I", read_exactly(sock, 4))
            raise ConnectionError(read_exactly(sock, length).decode(errors="replace"))
        types = read_exactly(sock, count)
        # 1: no authentication, the only one a VM's own socket offers.
        if 1 not in types:
            raise ConnectionError(f"the VNC server wants authentication: {list(types)}")
        sock.sendall(b"\x01")
        (result,) = struct.unpack(">I", read_exactly(sock, 4))
        if result != 0:
            raise ConnectionError("the VNC server refused the connection")
        # ClientInit, shared; then ServerInit: size, pixel format, name.
        sock.sendall(b"\x01")
        server_init = read_exactly(sock, 24)
        (name_length,) = struct.unpack(">I", server_init[20:24])
        read_exactly(sock, name_length)
        sock.sendall(struct.pack(">BxHi", SET_ENCODINGS, 1, EXTENDED_DESKTOP_SIZE))
        # One screen covering the desktop.
        sock.sendall(
            struct.pack(">BxHHBx", SET_DESKTOP_SIZE, width, height, 1)
            + struct.pack(">IHHHHI", 0, 0, 0, width, height, 0)
        )
        # QEMU answers once it has passed the size on; the answer itself does
        # not matter, only that the request was read before the socket
        # closes.
        try:
            sock.recv(64)
        except socket.timeout:
            pass


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__.split("\n\n")[2].strip())
    try:
        light(sys.argv[1], int(sys.argv[2]), int(sys.argv[3]))
    except (OSError, ValueError) as error:
        sys.exit(f"light-head: {error}")


if __name__ == "__main__":
    main()
