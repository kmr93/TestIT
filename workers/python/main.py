#!/usr/bin/env python3
"""
TestIT Isolated Node Worker Entrypoint
Implements the strict NDJSON invocation and result protocol.
Workers execute exactly one node instance and terminate.
"""

import datetime
import json
import os
import sys
import time

from adapters.api_request import execute_api_request
from adapters.data_tabular import execute_tabular_check
from adapters.db_mongodb import execute_mongodb_query
from adapters.db_mysql import execute_mysql_query
from adapters.script_runner import execute_approved_script


def emit_progress_frame(step_id: str, phase: str, counters: dict):
    """Emits an NDJSON progress frame to stdout (coalesced, non-sensitive)."""
    frame = {
        "frame_type": "progress",
        "step_id": step_id,
        "phase": phase,
        "counters": counters,
        "timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }
    sys.stdout.write(json.dumps(frame) + "\n")
    sys.stdout.flush()


def main():
    try:
        # 1. Read Invocation Envelope from stdin
        raw_input = sys.stdin.read().strip()
        if not raw_input:
            if len(sys.argv) > 1 and os.path.exists(sys.argv[1]):
                with open(sys.argv[1], "r", encoding="utf-8") as f:
                    raw_input = f.read().strip()

        if not raw_input:
            sys.stderr.write("Worker error: No invocation envelope received on stdin\n")
            sys.exit(1)

        envelope = json.loads(raw_input)
        step_id = envelope.get("step_id", "00000000-0000-0000-0000-000000000000")
        node_type = envelope.get("node_type", "api.request")
        config = envelope.get("config", {})
        inputs = envelope.get("inputs", {})
        secrets = envelope.get("secrets", {})

        def progress_callback(phase: str, counters: dict = None):
            emit_progress_frame(step_id, phase, counters or {})

        progress_callback("worker_initialized", {"node_type": node_type})

        # 2. Dispatch to specific adapter
        if node_type == "api.request":
            status, error, outputs, metrics = execute_api_request(
                config, inputs, secrets, progress_callback
            )
        elif node_type == "db.mysql":
            status, error, outputs, metrics = execute_mysql_query(
                config, inputs, secrets, progress_callback
            )
        elif node_type == "db.mongodb":
            status, error, outputs, metrics = execute_mongodb_query(
                config, inputs, secrets, progress_callback
            )
        elif node_type == "data.tabular":
            status, error, outputs, metrics = execute_tabular_check(
                config, inputs, secrets, progress_callback
            )
        elif node_type in ("script.python", "script.shell"):
            status, error, outputs, metrics = execute_approved_script(
                config, inputs, secrets, progress_callback
            )
        elif node_type == "sleep.wait":
            sec = min(config.get("duration_seconds", 1), 60)
            progress_callback("sleeping", {"duration_sec": sec})
            time.sleep(sec)
            status, error, outputs, metrics = (
                "SUCCEEDED",
                {},
                {"slept_seconds": sec},
                {"duration_ms": sec * 1000},
            )
        else:
            status, error, outputs, metrics = (
                "ERROR",
                {
                    "code": "UNKNOWN_NODE_TYPE",
                    "message": f"Adapter for node type '{node_type}' is not recognized",
                    "class": "INTERNAL",
                    "details": {},
                },
                {},
                {"duration_ms": 0.0},
            )

        # 3. Emit final Result Envelope
        result_envelope = {
            "frame_type": "result",
            "step_id": step_id,
            "status": status,
            "error": error if error else None,
            "outputs": outputs,
            "metrics": metrics,
            "artifacts": [],
            "redacted_keys": list(secrets.keys()),
        }

        sys.stdout.write(json.dumps(result_envelope) + "\n")
        sys.stdout.flush()

    except Exception as e:
        # Crash resilience: Emit error result envelope so coordinator is never hung
        sys.stderr.write(f"Fatal unhandled worker error: {str(e)}\n")
        crash_result = {
            "frame_type": "result",
            "step_id": "00000000-0000-0000-0000-000000000000",
            "status": "ERROR",
            "error": {
                "code": "WORKER_CRASH",
                "message": str(e),
                "class": "INTERNAL",
                "details": {},
            },
            "outputs": {},
            "metrics": {"duration_ms": 0.0},
            "artifacts": [],
            "redacted_keys": [],
        }
        sys.stdout.write(json.dumps(crash_result) + "\n")
        sys.stdout.flush()
        sys.exit(0)


if __name__ == "__main__":
    main()
