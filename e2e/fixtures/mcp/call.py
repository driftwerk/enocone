#!/usr/bin/env python3
"""call.py BIN PROJECT TOOL [ARGS_JSON]: call one tool of `enocone mcp`, as an AI tool would.

Starts `BIN --project PROJECT mcp`, does the MCP handshake over stdio, calls
TOOL with ARGS_JSON (default {}), and prints the text the tool returned.
Tools that answer in JSON print JSON, so a flow can check it with `json:`.
A tool error prints its text too, after "tool error: ". Standard library only.
"""
import json
import subprocess
import sys


def main() -> int:
    bin_, project, tool = sys.argv[1:4]
    args = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}
    proc = subprocess.Popen(
        [bin_, "--project", project, "mcp"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
    )

    def send(msg):
        proc.stdin.write(json.dumps(msg) + "\n")
        proc.stdin.flush()

    def reply(id_):
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            msg = json.loads(line)
            if msg.get("id") == id_:
                return msg
        raise SystemExit(f"enocone mcp exited before answering request {id_}")

    try:
        send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "enocone-e2e", "version": "1"}}})
        reply(1)
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": args}})
        res = reply(2)
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
    if "error" in res:
        print(f"protocol error: {res['error'].get('message')}")
        return 1
    result = res["result"]
    text = "\n".join(c.get("text", "") for c in result.get("content", []) if c.get("type") == "text")
    print(f"tool error: {text}" if result.get("isError") else text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
