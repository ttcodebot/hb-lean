"""Prefix each stdin line with seconds since start (line-buffered)."""
import sys, time
t0 = time.monotonic()
for line in sys.stdin.buffer:
    sys.stdout.buffer.write(b"%9.3f " % (time.monotonic() - t0) + line)
    sys.stdout.buffer.flush()
