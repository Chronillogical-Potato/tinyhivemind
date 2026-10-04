#!/usr/bin/env python3
"""A scripted OpenAI-compatible chat-completions server for offline runs.

It needs no network and no key. It answers by reading the request: the seat's
role from the system prompt, and how far along it is from the messages. The
canned task is "write `hi` to /tmp/hello.txt", so the same script drives the
single agent and the four-seat hive to a completed episode.

    python3 mock_llm.py --port-file /path/to/port

Binds 127.0.0.1 on an ephemeral port and writes it to --port-file.
"""

import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer


def tool_call(name, args, call_id="call_1"):
    return {
        "role": "assistant",
        "content": None,
        "tool_calls": [
            {
                "id": call_id,
                "type": "function",
                "function": {"name": name, "arguments": json.dumps(args)},
            }
        ],
    }


LONG = {"step": 0}


def decide(messages):
    system = messages[0]["content"]
    user = messages[1]["content"] if len(messages) > 1 else ""
    tool_results = [m for m in messages if m.get("role") == "tool"]
    done = len(tool_results)
    if "You condense" in system:
        return {"role": "assistant", "content": "wrote /tmp/hello.txt; then ran filler steps"}
    if "working alone" in system and "LONGSESSION" in system:
        # A long scripted session: 24 commands with ~2.5 KB of output each. The
        # step is counted here, not read off the transcript, because the context
        # policies rewrite the transcript.
        LONG["step"] = 0 if len(messages) == 2 else LONG["step"] + 1
        done = LONG["step"]
        if done == 0:
            return tool_call("bash", {"cmd": "echo hi > /tmp/hello.txt"}, "call_0")
        if done < 24:
            cmd = f"echo step {done}; head -c 2500 /dev/zero | tr '\\0' x"
            return tool_call("bash", {"cmd": cmd}, f"call_{done}")
        return tool_call("complete_episode", {"message": "long session finished"}, "call_end")
    if "You are the lead" in system:
        if "reported" in user:
            return tool_call("complete_episode", {"message": "hello.txt written and checked"})
        return tool_call(
            "broadcast", {"message": "implement: write hi to /tmp/hello.txt and verify it"}
        )
    if "You are the implementer" in system:
        if done == 0:
            return tool_call("bash", {"cmd": "echo hi > /tmp/hello.txt && cat /tmp/hello.txt"})
        return tool_call("post", {"message": "wrote /tmp/hello.txt containing hi"})
    if "working alone" in system:
        if done == 0:
            return tool_call("bash", {"cmd": "echo hi > /tmp/hello.txt"})
        if done == 1:
            return tool_call("bash", {"cmd": "cat /tmp/hello.txt"})
        return tool_call("complete_episode", {"message": "wrote /tmp/hello.txt"})
    return tool_call("post", {"message": "nothing to add"})


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length))
        message = decide(body["messages"])
        prompt = len(json.dumps(body["messages"])) // 4
        reply = {
            "id": "mock",
            "choices": [{"index": 0, "message": message, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": prompt, "completion_tokens": 12},
        }
        data = json.dumps(reply).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, fmt, *args):
        print(fmt % args, file=sys.stderr)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port-file", required=True)
    args = parser.parse_args()
    server = HTTPServer(("127.0.0.1", 0), Handler)
    with open(args.port_file, "w") as handle:
        handle.write(str(server.server_address[1]))
    server.serve_forever()


if __name__ == "__main__":
    main()
