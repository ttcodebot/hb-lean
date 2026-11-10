#!/bin/bash
# Records the environment LibreLane passes to each OpenROAD step, then runs the real binary.
if [ -n "$STEP_DIR" ] && [ ! -f "$STEP_DIR/_proc_env" ]; then env -0 > "$STEP_DIR/_proc_env"; fi
exec "$REAL_OPENROAD" "$@"
