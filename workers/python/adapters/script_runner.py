import subprocess
import time
from typing import Any, Dict, Tuple


def execute_approved_script(
    config: Dict[str, Any],
    inputs: Dict[str, Any],
    secrets: Dict[str, str],
    emit_progress: Any,
) -> Tuple[str, Dict[str, Any], Dict[str, Any], Dict[str, Any]]:
    """
    Executes an approved, admin-reviewed script asset within bounded limits.
    Arbitrary inline script execution is prohibited by design.
    """
    script_content = config.get("script_content", "")
    args = config.get("args", [])
    timeout_sec = min(config.get("timeout_seconds", 30), 60)
    max_output_bytes = 262144  # 256KB output cap

    emit_progress("preparing_script_sandbox", {"timeout": timeout_sec})
    start_time = time.time()

    if not script_content:
        return (
            "ERROR",
            {
                "code": "EMPTY_SCRIPT",
                "message": "Script asset content cannot be empty",
                "class": "INTERNAL",
                "details": {},
            },
            {},
            {"duration_ms": 0.0},
        )

    try:
        emit_progress("executing_script", {})
        proc = subprocess.run(
            ["python", "-c", script_content] + [str(a) for a in args],
            capture_output=True,
            text=True,
            timeout=timeout_sec,
        )

        duration_ms = (time.time() - start_time) * 1000
        stdout_capped = proc.stdout[:max_output_bytes]

        if proc.returncode != 0:
            return (
                "ERROR",
                {
                    "code": "SCRIPT_NON_ZERO_EXIT",
                    "message": f"Script failed with exit code {proc.returncode}: {proc.stderr[:1024]}",
                    "class": "INTERNAL",
                    "details": {"exit_code": proc.returncode},
                },
                {"stdout": stdout_capped, "exit_code": proc.returncode},
                {"duration_ms": duration_ms, "exit_code": proc.returncode},
            )

        return (
            "SUCCEEDED",
            {},
            {"stdout": stdout_capped, "exit_code": 0},
            {"duration_ms": duration_ms, "exit_code": 0},
        )

    except subprocess.TimeoutExpired:
        duration_ms = (time.time() - start_time) * 1000
        return (
            "TIMED_OUT",
            {
                "code": "SCRIPT_TIMED_OUT",
                "message": f"Script exceeded wall-clock timeout of {timeout_sec}s",
                "class": "TIMEOUT",
                "details": {},
            },
            {},
            {"duration_ms": duration_ms},
        )
    except Exception as e:
        duration_ms = (time.time() - start_time) * 1000
        return (
            "ERROR",
            {
                "code": "SCRIPT_EXECUTION_FAILED",
                "message": str(e),
                "class": "INTERNAL",
                "details": {},
            },
            {},
            {"duration_ms": duration_ms},
        )
