"""Real POSIX terminal checks; run after cargo build: python3 tests/terminal_cli.py.

Uses only Python's standard library, local fixtures, and disposable workspaces.
"""
import errno
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer


BINARY = str(Path(sys.argv.pop(1) if len(sys.argv) > 1 else "target/debug/rein").resolve())


class Terminal:
    def __init__(self, args, env=None):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.execve(BINARY, [BINARY, *map(str, args)], {**os.environ, "TERM": "xterm", **(env or {})})
        self.buffer = b""

    def send(self, text):
        os.write(self.fd, text if isinstance(text, bytes) else text.encode())

    def until(self, marker, timeout=10):
        marker = marker.encode()
        deadline = time.monotonic() + timeout
        while marker not in self.buffer:
            if time.monotonic() > deadline:
                raise AssertionError(f"Timed out waiting for {marker!r}: {self.buffer!r}")
            if not select.select([self.fd], [], [], 0.1)[0]:
                continue
            try:
                chunk = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    raise AssertionError(f"Terminal exited before {marker!r}: {self.buffer!r}") from error
                raise
            if not chunk:
                raise AssertionError(f"Terminal EOF before {marker!r}")
            if b"\x1b[6n" in chunk:
                self.send(b"\x1b[1;1R")
            self.buffer += chunk
        end = self.buffer.index(marker) + len(marker)
        result, self.buffer = self.buffer[:end], self.buffer[end:]
        return result

    def wait(self):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.pid = None
                if os.waitstatus_to_exitcode(status) != 0:
                    raise AssertionError(f"CLI exited unsuccessfully: {status}")
                return
            time.sleep(0.02)
        raise AssertionError("CLI did not exit")

    def close(self):
        os.close(self.fd)
        if self.pid:
            try:
                os.kill(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(self.pid, 0)
            self.pid = None


class TerminalTests(unittest.TestCase):
    def test_shell_requires_fresh_approval_and_returns_verified_results(self):
        requests = []
        commands = {
            "deny": "touch denied",
            "approve": (
                'test -z "$OPENAI_API_KEY" && test -z "$OPENCODE_GO_API_KEY" || exit 90; '
                'pwd; printf "%09000d" 1; printf "test failed" >&2; touch approved; exit 7'
            ),
            "cancel approval": "touch cancelled_before_start",
            "cancel command": "sleep 30 & echo $! > child.pid; wait",
            "test crate": "cargo test --offline --quiet",
            "disabled": "touch disabled",
        }

        class Provider(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                requests.append(body)
                last = body["messages"][-1]
                if last["role"] == "tool":
                    delta = {"role": "assistant", "content": "Tool result received"}
                    finish = "stop"
                else:
                    name = last["content"]
                    delta = {"role": "assistant", "tool_calls": [{
                        "index": 0, "id": f"call-{len(requests)}", "type": "function",
                        "function": {"name": "run_command", "arguments": json.dumps({"command": commands[name]})},
                    }]}
                    finish = "tool_calls"
                chunks = [
                    {"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                    {"choices": [{"index": 0, "delta": {}, "finish_reason": finish}]},
                    {"choices": [], "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}},
                ]
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for chunk in chunks:
                    chunk.update({"id": "fixture", "model": "fixture"})
                    self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
                self.wfile.write(b"data: [DONE]\n\n")

        with tempfile.TemporaryDirectory(prefix="rein-shell-") as directory:
            root = Path(directory).resolve()
            (root / "Cargo.toml").write_text(
                '[package]\nname = "rein-shell-fixture"\nversion = "0.1.0"\nedition = "2021"\n'
            )
            (root / "src").mkdir()
            (root / "src/lib.rs").write_text(
                "#[test] fn addition() { assert_eq!(2 + 2, 4); }\n"
            )
            session = root / ".session.json"
            server = HTTPServer(("127.0.0.1", 0), Provider)
            env = {
                "OPENAI_API_KEY": "fixture-not-a-secret",
                "OPENCODE_GO_API_KEY": "other-fixture-not-a-secret",
                "OPENAI_BASE_URL": f"http://127.0.0.1:{server.server_port}/v1",
            }
            args = ["chat-openai", session, "fixture", root]
            terminal = Terminal([*args, "--allow-shell"], env)
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                def request(name):
                    terminal.until("you > ")
                    terminal.send(name + "\r")
                    prompt = terminal.until("or Enter to deny: ")
                    self.assertIn(commands[name].encode().split(b";")[0].replace(b'"', b'\\"'), prompt)
                    return re.search(rb"Type (run [a-f0-9]{8}) to approve", prompt).group(1)

                denied = request("deny")
                self.assertFalse((root / "denied").exists())
                terminal.send(b"\r")
                terminal.until("Tool result received")
                self.assertIn("approval denied", requests[-1]["messages"][-1]["content"])
                self.assertFalse((root / "denied").exists())

                approved = request("approve")
                self.assertNotEqual(denied, approved)
                self.assertFalse((root / "approved").exists())
                terminal.send(approved + b"\r")
                terminal.until("exit status: 7")
                terminal.until("Tool result received")
                result = json.loads(requests[-1]["messages"][-1]["content"])
                self.assertTrue((root / "approved").exists())
                self.assertEqual(result["exit_code"], 7)
                self.assertFalse(result["success"])
                self.assertEqual(result["stderr"], "test failed")
                self.assertTrue(result["stdout"].startswith(str(root) + "\n"))
                self.assertEqual(len(result["stdout"]), 4096)
                self.assertTrue(result["stdout_truncated"])
                self.assertFalse(result["stderr_truncated"])

                request("cancel approval")
                terminal.send(b"\x03")
                terminal.until("turn stopped: Cancelled")
                self.assertFalse((root / "cancelled_before_start").exists())

                terminal.send(request("cancel command") + b"\r")
                deadline = time.monotonic() + 5
                child = root / "child.pid"
                while not child.exists() or not child.read_text().strip():
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.02)
                pid = child.read_text().strip()
                terminal.send(b"\x03")
                terminal.until("turn stopped: Cancelled")
                # A killed child may briefly remain a zombie until its new parent reaps it.
                deadline = time.monotonic() + 5
                while True:
                    state = subprocess.run(
                        ["ps", "-o", "stat=", "-p", pid], capture_output=True, text=True,
                    ).stdout.strip()
                    if not state or state.startswith("Z"):
                        break
                    self.assertLess(time.monotonic(), deadline, f"child {pid} survived cancellation")
                    time.sleep(0.02)

                terminal.send(request("test crate") + b"\r")
                terminal.until("exit status: 0", timeout=30)
                terminal.until("Tool result received")
                result = json.loads(requests[-1]["messages"][-1]["content"])
                self.assertTrue(result["success"])
                self.assertIn("1 passed; 0 failed", result["stdout"])
                terminal.until("you > ")
                terminal.send("/exit\r")
                terminal.until("Goodbye.")
                terminal.wait()
            finally:
                terminal.close()
                # Do not fork the next PTY while the server thread is active.
                server.shutdown()
                worker.join(5)
            terminal = Terminal(args, env)
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                terminal.until("no shell")
                terminal.until("you > ")
                terminal.send("disabled\r")
                terminal.until("Tool result received")
                self.assertNotIn("run_command", [t["function"]["name"] for t in requests[-2]["tools"]])
                self.assertIn("not authorized", requests[-1]["messages"][-1]["content"])
                self.assertFalse((root / "disabled").exists())
                terminal.until("you > ")
                terminal.send("/exit\r")
                terminal.until("Goodbye.")
                terminal.wait()
            finally:
                terminal.close()
                server.shutdown()
                server.server_close()
                worker.join(5)

    def test_edit_history_paste_interrupt_eof_and_resume(self):
        with tempfile.TemporaryDirectory(prefix="rein-terminal-") as directory:
            path = Path(directory) / "session.json"
            terminal = Terminal(["chat-demo", path])
            try:
                terminal.until("you > ")
                terminal.send(b"hellp\x7fo\r")  # Backspace edits hellp to hello.
                terminal.until("session saved]")
                terminal.until("you > ")
                terminal.send(b"\x1b[A\r")  # Up recalls the preceding prompt.
                terminal.until("session saved]")
                terminal.until("you > ")
                terminal.send(b"\x03")
                terminal.until("Input cleared")
                terminal.until("you > ")
                terminal.send("//help\r")
                terminal.until("session saved]")
                terminal.until("you > ")
                terminal.send("/paste\r")
                terminal.until("... > ")
                terminal.send("discard this\r")
                terminal.until("... > ")
                terminal.send(b"\x03")
                terminal.until("Paste discarded")
                terminal.until("you > ")
                terminal.send(b"\x04")
                terminal.until("Goodbye.")
                terminal.wait()
            finally:
                terminal.close()
            snapshot = json.loads(path.read_text())
            prompts = [entry["message"]["content"][0]["text"] for entry in snapshot["transcript"] if entry["source"] == "User"]
            self.assertEqual(prompts, ["hello", "hello", "/help"])
            terminal = Terminal(["chat-demo", path])
            try:
                terminal.until("you > ")
                terminal.send(b"\x1b[A\r")
                terminal.until("session saved]")
                terminal.until("you > ")
                terminal.send("/exit\r")
                terminal.until("Goodbye.")
                terminal.wait()
            finally:
                terminal.close()
            snapshot = json.loads(path.read_text())
            self.assertEqual(snapshot["transcript"][-4]["message"]["content"][0]["text"], "/help")

    def test_ctrl_c_cancels_model_and_next_turn_works(self):
        release = threading.Event()

        class Provider(BaseHTTPRequestHandler):
            calls = 0

            def log_message(self, *_):
                pass

            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                Provider.calls += 1
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()

                def event(delta, finish=None):
                    value = {"id": "fixture", "model": "fixture", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
                    self.wfile.write(f"data: {json.dumps(value)}\n\n".encode())
                    self.wfile.flush()

                if Provider.calls == 1:
                    event({"role": "assistant", "content": "unfinished draft"})
                    release.wait(10)
                else:
                    event({"role": "assistant", "content": "Recovered answer"})
                    event({}, "stop")
                    self.wfile.write(b'data: {"id":"fixture","model":"fixture","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}\n\ndata: [DONE]\n\n')
                    self.wfile.flush()

        with tempfile.TemporaryDirectory(prefix="rein-cancel-") as directory:
            path = Path(directory) / ".session.json"
            server = HTTPServer(("127.0.0.1", 0), Provider)
            terminal = Terminal(["chat-openai", path, "fixture", directory], {
                "OPENAI_API_KEY": "fixture-not-a-secret",
                "OPENAI_BASE_URL": f"http://127.0.0.1:{server.server_port}/v1",
            })
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                terminal.until("you > ")
                terminal.send("first task\r")
                terminal.until("unfinished draft")
                terminal.send(b"\x03")
                terminal.until("turn stopped: Cancelled")
                terminal.until("you > ")
                snapshot = json.loads(path.read_text())
                self.assertEqual(len(snapshot["transcript"]), 1)
                self.assertNotIn("unfinished draft", path.read_text())
                release.set()
                terminal.send("second task\r")
                terminal.until("Recovered answer")
                terminal.until("session saved]")
                terminal.until("you > ")
                terminal.send("/exit\r")
                terminal.until("Goodbye.")
                terminal.wait()
                self.assertEqual(Provider.calls, 2)
                self.assertEqual(len(json.loads(path.read_text())["transcript"]), 3)
            finally:
                release.set()
                terminal.close()
                server.shutdown()
                server.server_close()
                worker.join(5)


if __name__ == "__main__":
    unittest.main()
