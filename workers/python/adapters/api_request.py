import time
from typing import Any, Dict, Tuple


def execute_api_request(
    config: Dict[str, Any],
    inputs: Dict[str, Any],
    secrets: Dict[str, str],
    emit_progress: Any,
) -> Tuple[str, Dict[str, Any], Dict[str, Any], Dict[str, Any]]:
    """
    Executes an HTTP API node using Playwright APIRequestContext (or requests fallback).
    Asserts status codes, headers, and extracts JSON outputs.
    """
    emit_progress("preparing_request", {"attempt": 1})

    url = config.get("url") or config.get("path") or "https://httpbin.org/get"
    method = config.get("method", "GET").upper()
    headers = config.get("headers", {})
    body = config.get("body")
    timeout_sec = config.get("timeout_seconds", 30)

    # Resolve authorization profile if specified
    if "auth_profile" in config:
        auth = config["auth_profile"]
        if auth.get("type") == "bearer" and "token_secret" in auth:
            sec_name = auth["token_secret"]
            token = secrets.get(sec_name, "")
            headers["Authorization"] = f"Bearer {token}"

    emit_progress("sending_http_request", {"method": method, "url": url})
    start_time = time.time()

    status_code = 200
    resp_body = {}
    duration_ms = 0.0

    try:
        # Use Playwright Python APIRequestContext if available
        try:
            from playwright.sync_api import sync_playwright
            with sync_playwright() as p:
                req_context = p.request.new_context(extra_http_headers=headers)
                resp = req_context.fetch(
                    url,
                    method=method,
                    data=body if isinstance(body, (str, bytes)) else None,
                    timeout=timeout_sec * 1000,
                )
                status_code = resp.status
                duration_ms = (time.time() - start_time) * 1000
                try:
                    resp_body = resp.json()
                except Exception:
                    resp_body = {"raw_text": resp.text()[:2048]}
        except ImportError:
            # Fallback to requests
            import requests
            resp = requests.request(
                method=method,
                url=url,
                headers=headers,
                json=body if isinstance(body, dict) else None,
                data=body if isinstance(body, str) else None,
                timeout=timeout_sec,
            )
            status_code = resp.status_code
            duration_ms = (time.time() - start_time) * 1000
            try:
                resp_body = resp.json()
            except Exception:
                resp_body = {"raw_text": resp.text[:2048]}

        emit_progress("evaluating_assertions", {"status_code": status_code})

        # Evaluate expected status
        expected_status = config.get("expected_status", 200)
        if isinstance(expected_status, int) and status_code != expected_status:
            return (
                "ASSERTION_FAILED",
                {
                    "code": "STATUS_CODE_MISMATCH",
                    "message": f"Expected status {expected_status}, but received {status_code}",
                    "class": "ASSERTION",
                    "details": {"actual_status": status_code, "expected": expected_status},
                },
                {"status_code": status_code, "body": resp_body},
                {"duration_ms": duration_ms, "status_code": status_code},
            )

        # Output extraction
        outputs = {"status_code": status_code, "response": resp_body}
        if isinstance(resp_body, dict):
            for k, v in resp_body.items():
                outputs[k] = v

        return (
            "SUCCEEDED",
            {},
            outputs,
            {"duration_ms": duration_ms, "status_code": status_code},
        )

    except Exception as e:
        duration_ms = (time.time() - start_time) * 1000
        return (
            "ERROR",
            {
                "code": "REQUEST_FAILED",
                "message": str(e),
                "class": "TRANSIENT_NETWORK",
                "details": {},
            },
            {},
            {"duration_ms": duration_ms},
        )
