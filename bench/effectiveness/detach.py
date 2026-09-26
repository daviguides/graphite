"""Launch a command in its own session so no parent shell, waiter or monitor
can signal it. Usage: python3 detach.py <logfile> <cmd> [args...]
Prints the child PID."""

import subprocess
import sys

log = open(sys.argv[1], "a")
proc = subprocess.Popen(sys.argv[2:], stdout=log, stderr=subprocess.STDOUT,
                        stdin=subprocess.DEVNULL, start_new_session=True)
print(proc.pid)
