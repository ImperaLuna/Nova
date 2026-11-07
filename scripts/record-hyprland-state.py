#!/usr/bin/env python3
# Records how the running Hyprland describes the desktop right now (the
# questions novad asks: monitors, workspaces, windows, focus, keyboards),
# into crates/novad/src/compositor/recorded/<name>/, for novad's tests.
#
# Set the desktop up first (say, a window in a special workspace shown on
# one screen), then, inside that Hyprland session:
#   scripts/record-hyprland-state.py hyprland-0.57-special
import json
import os
import socket
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RECORDED = ROOT / "crates/novad/src/compositor/recorded"
QUERIES = ["monitors", "workspaces", "clients", "activewindow", "activeworkspace", "devices", "version"]


def ask(request: str) -> str:
    instance = Path(os.environ["XDG_RUNTIME_DIR"]) / "hypr" / os.environ["HYPRLAND_INSTANCE_SIGNATURE"]
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(5)
        sock.connect(str(instance / ".socket.sock"))
        sock.sendall(request.encode())
        sock.shutdown(socket.SHUT_WR)
        answer = b""
        while chunk := sock.recv(65536):
            answer += chunk
    return answer.decode()


def main() -> None:
    out = RECORDED / sys.argv[1]
    out.mkdir(parents=True, exist_ok=True)
    for query in QUERIES:
        answer = ask(f"j/{query}")
        json.loads(answer)  # Hyprland's own text, kept as it is, but checked.
        (out / f"{query}.json").write_text(answer)
    print(f"recorded {', '.join(QUERIES)} in {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
