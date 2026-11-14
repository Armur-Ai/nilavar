#!/usr/bin/env python3
"""Runs a command under a pseudo-terminal and answers passphrase prompts.

Usage: pty_drive.py ANSWER [ANSWER ...] -- COMMAND [ARGS ...]
Each ANSWER is sent after a prompt containing "passphrase" appears.
Exits with the command's exit status. Test helper only (synthetic data).
"""
import os, pty, sys, time

sep = sys.argv.index("--")
answers, cmd = sys.argv[1:sep], sys.argv[sep + 1:]
pid, fd = pty.fork()
if pid == 0:
    os.execvp(cmd[0], cmd)

buf = b""
deadline = time.time() + 60
while True:
    try:
        chunk = os.read(fd, 1024)
    except OSError:
        break
    if not chunk:
        break
    buf += chunk
    if answers and b"passphrase" in buf.lower():
        time.sleep(0.1)
        os.write(fd, answers.pop(0).encode() + b"\n")
        buf = b""
    if time.time() > deadline:
        os.kill(pid, 9)
        break
_, status = os.waitpid(pid, 0)
sys.exit(os.waitstatus_to_exitcode(status))
