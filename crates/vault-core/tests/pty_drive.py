#!/usr/bin/env python3
"""Runs a command under a pseudo-terminal and answers passphrase prompts.

Usage: pty_drive.py ANSWER [ANSWER ...] -- COMMAND [ARGS ...]
Each ANSWER is sent after output containing "passphrase" appears. A hard
30-second deadline applies; on failure the (synthetic) transcript is printed
to stderr. Exits with the command's exit status. Test helper only.
"""
import os, pty, select, signal, sys, time

sep = sys.argv.index("--")
answers, cmd = sys.argv[1:sep], sys.argv[sep + 1:]
pid, fd = pty.fork()
if pid == 0:
    os.execvp(cmd[0], cmd)

transcript, pending = b"", b""
deadline = time.time() + 30
status = None
while time.time() < deadline:
    r, _, _ = select.select([fd], [], [], 0.2)
    if r:
        try:
            chunk = os.read(fd, 1024)
        except OSError:
            chunk = b""
        if not chunk:
            break
        transcript += chunk
        pending += chunk
        if answers and b"passphrase" in pending.lower():
            time.sleep(0.15)
            os.write(fd, answers.pop(0).encode() + b"\n")
            pending = b""
    else:
        done, st = os.waitpid(pid, os.WNOHANG)
        if done:
            status = st
            break
if status is None:
    if time.time() >= deadline:
        os.kill(pid, signal.SIGKILL)
    _, status = os.waitpid(pid, 0)
code = os.waitstatus_to_exitcode(status)
if code != 0:
    sys.stderr.write("pty_drive: %r exited %d; transcript:\n%s\n" % (cmd[0], code, transcript.decode("utf-8", "replace")))
sys.exit(code)
