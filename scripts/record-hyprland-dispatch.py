#!/usr/bin/env python3
# Records how the running Hyprland (Lua config) answers the dispatchers
# novad's tests send, into
# framework/compositor/recorded/hyprland-<version>/dispatch.json.
#
# Nothing is dispatched but `hl.dsp.no_op()`, which does nothing. Every other
# dispatcher is only built, in `repl return type(<dispatcher>)`: Hyprland
# parses it and checks its arguments without running it, so the desktop
# doesn't change (`hl.dsp.exit()` included). A dispatcher is `userdata`;
# anything else answers with its error. Add a dispatcher a test sends to
# `built` (any answer) and run this again.
#
# Run it inside a Hyprland session: scripts/record-hyprland-dispatch.py <version>
import json
import os
import socket
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RECORDED = ROOT / "framework/compositor/recorded"
# The one dispatcher run for real: novad's question whether Hyprland speaks Lua.
PROBE = "hl.dsp.no_op()"


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
    out = RECORDED / f"hyprland-{sys.argv[1]}" / "dispatch.json"
    old = json.loads(out.read_text()) if out.exists() else {}
    # The dispatchers to build: the ones the tests send, kept in the file.
    built = old.get("built", {})
    probe = ask(f"dispatch {PROBE}")
    if probe.strip() != "ok":
        sys.exit(f"this Hyprland doesn't take Lua dispatchers ({probe!r}); run it on one with a Lua config")
    recording = {
        "version": json.loads(ask("j/version")).get("tag", ""),
        "dispatched": {PROBE: probe},
        "built": {d: ask(f"repl return type({d})") for d in built},
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(recording, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(recording, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
