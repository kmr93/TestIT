import json
import time
from urllib.parse import urlsplit
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

    url = config.get("url") or config.get("path")
    if not url:
        return ("ERROR", {"code": "API_URL_REQUIRED", "message": "An explicit request URL is required", "class": "INTERNAL", "details": {}}, {}, {"duration_ms": 0.0})
    method = config.get("method", "GET").upper()
    if method not in {"GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"}:
        return ("ERROR", {"code": "HTTP_METHOD_INVALID", "message": "The HTTP method is not supported", "class": "INTERNAL", "details": {}}, {}, {"duration_ms": 0.0})
    headers = dict(config.get("headers", {}))
    body = config.get("body")
    timeout_sec = config.get("timeout_seconds", 30)
    max_response_bytes = min(max(int(config.get("max_response_bytes", 1_048_576)), 1), 2 * 1024 * 1024)
    request_body = body
    if isinstance(body, (dict, list)):
        request_body = json.dumps(body, separators=(",", ":"))
        if not any(key.lower() == "content-type" for key in headers):
            headers["Content-Type"] = "application/json"

    if any(any(marker in key.lower() for marker in ("authorization", "cookie", "token", "api-key", "api_key", "password", "credential")) for key in headers):
        return ("ERROR", {"code": "INLINE_CREDENTIAL_DISALLOWED", "message": "Credentials must use a secret reference, not inline request headers", "class": "AUTH", "details": {}}, {}, {"duration_ms": 0.0})

    response_headers = {}
    # Resolve authorization profile if specified
    if "auth_profile" in config:
        auth = config["auth_profile"]
        if auth.get("type") == "bearer" and "token_secret" in auth:
            sec_name = auth["token_secret"]
            token = secrets.get(sec_name, "")
            if not token:
                return ("ERROR", {"code": "AUTH_SECRET_UNAVAILABLE", "message": "The referenced API credential is unavailable", "class": "AUTH", "details": {}}, {}, {"duration_ms": 0.0})
            headers["Authorization"] = f"Bearer {token}"

    host = urlsplit(url).hostname or ""
    emit_progress("sending_http_request", {"attempt": 1})
    start_time = time.time()

    status_code = 200
    resp_body = {}
    duration_ms = 0.0

    try:
        # Use Playwright Python APIRequestContext if available
        try:
            from playwright.sync_api import sync_playwright
            with sync_playwright() as p:
                req_context = p.request.new_context(extra_http_headers=headers, timeout=timeout_sec * 1000)
                resp = req_context.fetch(
                    url,
                    method=method,
                    data=request_body if isinstance(request_body, (str, bytes)) else None,
                    timeout=timeout_sec * 1000,
                    max_redirects=0,
                )
                status_code = resp.status
                response_headers = resp.headers
                duration_ms = (time.time() - start_time) * 1000
                try:
                    raw_content = resp.body()
                    if len(raw_content) > max_response_bytes:
                        resp.dispose()
                        req_context.dispose()
                        return ("ERROR", {"code": "RESPONSE_TOO_LARGE", "message": "Response exceeded the configured inspection limit", "class": "CLIENT_ERROR", "details": {}}, {}, {"duration_ms": duration_ms})
                    resp_body = json.loads(raw_content)
                except Exception:
                    resp_body = {}
                resp.dispose()
                req_context.dispose()
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
                allow_redirects=False,
                stream=True,
            )
            status_code = resp.status_code
            response_headers = resp.headers
            duration_ms = (time.time() - start_time) * 1000
            content = bytearray()
            for chunk in resp.iter_content(chunk_size=65536):
                content.extend(chunk)
                if len(content) > max_response_bytes:
                    resp.close()
                    raise ValueError("Response exceeded the 2 MiB inspection limit")
            try:
                resp_body = json.loads(content)
            except Exception:
                resp_body = {}

        emit_progress("evaluating_assertions", {"status_code": status_code})

        # Evaluate expected status
        expected_status = config.get("expected_status", 200)
        allowed_statuses = config.get("allowed_status_codes")
        status_ok = status_code in allowed_statuses if isinstance(allowed_statuses, list) else status_code == expected_status
        if not status_ok:
            return (
                "ASSERTION_FAILED",
                {
                    "code": "STATUS_CODE_MISMATCH",
                    "message": f"Expected status {expected_status}, but received {status_code}",
                    "class": "ASSERTION",
                    "details": {"actual_status": status_code, "expected": expected_status},
                },
                {"status_code": status_code},
                {"duration_ms": duration_ms, "status_code": status_code},
            )

        assertions = config.get("assertions", [])
        if not isinstance(assertions, list) or len(assertions) > 100:
            return ("ERROR", {"code": "ASSERTION_CONFIG_INVALID", "message": "The request has an invalid assertion list", "class": "INTERNAL", "details": {}}, {}, {"duration_ms": duration_ms})
        for assertion in assertions:
            if not isinstance(assertion, dict) or not evaluate_assertion(assertion, resp_body, response_headers):
                return (
                    "ASSERTION_FAILED",
                    {"code": "RESPONSE_ASSERTION_FAILED", "message": "A configured response assertion failed", "class": "ASSERTION", "details": {"target": assertion.get("target") if isinstance(assertion, dict) else "unknown", "path": assertion.get("path") if isinstance(assertion, dict) else None}},
                    {"status_code": status_code},
                    {"duration_ms": duration_ms, "status_code": status_code},
                )

        max_response_time = config.get("max_response_time_ms")
        if max_response_time is not None and duration_ms > float(max_response_time):
            return (
                "ASSERTION_FAILED",
                {"code": "RESPONSE_TIME_EXCEEDED", "message": "The response exceeded the configured time limit", "class": "ASSERTION", "details": {}},
                {"status_code": status_code},
                {"duration_ms": duration_ms, "status_code": status_code},
            )

        # Persist only explicitly selected response fields; raw response bodies are not run outputs.
        outputs = {"status_code": status_code}
        extract = config.get("extract", {})
        if isinstance(extract, dict) and isinstance(resp_body, dict):
            for output_name, path in extract.items():
                selected = resp_body
                for part in str(path).split("."):
                    if isinstance(selected, dict):
                        selected = selected.get(part)
                    elif isinstance(selected, list) and part.isdigit() and int(part) < len(selected):
                        selected = selected[int(part)]
                    else:
                        selected = None
                        break
                if selected is not None:
                    outputs[str(output_name)] = selected

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
                "message": f"API request to {host or 'configured host'} failed",
                "class": "TRANSIENT_NETWORK",
                "details": {},
            },
            {},
            {"duration_ms": duration_ms},
        )


def evaluate_assertion(assertion: Dict[str, Any], body: Any, headers: Dict[str, Any]) -> bool:
    target = assertion.get("target")
    operator = assertion.get("operator")
    expected = assertion.get("expected")
    if target == "json":
        actual = body
        for part in str(assertion.get("path", "")).split("."):
            if isinstance(actual, dict):
                actual = actual.get(part)
            elif isinstance(actual, list) and part.isdigit() and int(part) < len(actual):
                actual = actual[int(part)]
            else:
                actual = None
                break
    elif target == "header":
        actual = next((value for name, value in headers.items() if str(name).lower() == str(assertion.get("name", "")).lower()), None)
    else:
        return False

    if operator == "exists":
        return actual is not None
    if operator == "equals":
        return actual == expected
    if operator == "not_equals":
        return actual != expected
    if operator == "contains":
        try:
            return expected in actual
        except TypeError:
            return False
    if operator in {"greater_than", "less_than"}:
        try:
            return float(actual) > float(expected) if operator == "greater_than" else float(actual) < float(expected)
        except (TypeError, ValueError):
            return False
    return False


def execute_wait_until(
    config: Dict[str, Any],
    inputs: Dict[str, Any],
    secrets: Dict[str, str],
    emit_progress: Any,
) -> Tuple[str, Dict[str, Any], Dict[str, Any], Dict[str, Any]]:
    """Poll an API or read-only database condition until it passes or expires."""
    target = str(config.get("target", "api")).lower()
    method = str(config.get("method", "GET")).upper()
    if target == "api" and method not in {"GET", "HEAD", "OPTIONS"}:
        return (
            "ERROR",
            {"code": "WAIT_METHOD_UNSAFE", "message": "Wait-until only permits idempotent read methods", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )
    if target not in {"api", "mysql", "mongodb"}:
        return (
            "ERROR",
            {"code": "WAIT_TARGET_UNSUPPORTED", "message": "Wait-until target must be API, MySQL, or MongoDB", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )
    try:
        deadline_seconds = min(max(int(config.get("timeout_seconds", 30)), 1), 3600)
        interval_seconds = min(max(int(config.get("poll_interval_seconds", 2)), 1), 60)
        request_timeout = min(max(int(config.get("request_timeout_seconds", 10)), 1), 60)
        max_attempts = min(max(int(config.get("max_attempts", deadline_seconds)), 1), 3600)
    except (TypeError, ValueError):
        return (
            "ERROR",
            {"code": "WAIT_CONFIG_INVALID", "message": "Wait timing settings must be bounded integers", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    request_config = {key: value for key, value in config.items() if key not in {"poll_interval_seconds", "request_timeout_seconds", "max_attempts"}}
    request_config.pop("target", None)
    started = time.monotonic()
    deadline = started + deadline_seconds
    attempts = 0
    last_error: Dict[str, Any] = {}
    last_metrics: Dict[str, Any] = {}
    while attempts < max_attempts and time.monotonic() < deadline:
        attempts += 1
        remaining = max(1, int(deadline - time.monotonic()))
        request_config["timeout_seconds"] = min(request_timeout, remaining)
        request_config["connect_timeout_seconds"] = min(request_timeout, remaining, 30)
        request_config["connect_timeout_ms"] = min(request_timeout, remaining, 30) * 1000
        emit_progress("polling_condition", {"attempt": attempts, "max_attempts": max_attempts})
        if target == "api":
            status, error, outputs, metrics = execute_api_request(request_config, inputs, secrets, lambda *_: None)
        elif target == "mysql":
            from adapters.db_mysql import execute_mysql_query

            request_config.setdefault("expected_min_rows", 1)
            status, error, outputs, metrics = execute_mysql_query(request_config, inputs, secrets, lambda *_: None)
            # A wait step observes only a count and never carries query rows forward.
            outputs = {"row_count": outputs.get("row_count")} if isinstance(outputs, dict) and "row_count" in outputs else {}
        else:
            from adapters.db_mongodb import execute_mongodb_query

            request_config.setdefault("expected_min_count", 1)
            status, error, outputs, metrics = execute_mongodb_query(request_config, inputs, secrets, lambda *_: None)
            outputs = {"count": outputs.get("count")} if isinstance(outputs, dict) and "count" in outputs else {}
        last_error = error or {}
        last_metrics = metrics or {}
        if status == "SUCCEEDED":
            elapsed_ms = (time.monotonic() - started) * 1000
            return (
                "SUCCEEDED",
                {},
                {**outputs, "attempts": attempts},
                {**last_metrics, "duration_ms": elapsed_ms, "attempts": attempts, "condition_met": True},
            )
        retryable = status == "ASSERTION_FAILED" or (
            status == "ERROR" and last_error.get("class") == "TRANSIENT_NETWORK"
        )
        if not retryable:
            return (
                status,
                last_error,
                outputs or {},
                {**last_metrics, "attempts": attempts, "condition_met": False},
            )
        wait_seconds = min(interval_seconds, max(0.0, deadline - time.monotonic()))
        if wait_seconds:
            time.sleep(wait_seconds)

    elapsed_ms = (time.monotonic() - started) * 1000
    return (
        "TIMED_OUT",
        {
            "code": "WAIT_CONDITION_TIMEOUT",
            "message": "The condition did not pass before its deadline",
            "class": "TIMEOUT",
            "details": {
                "attempts": attempts,
                "last_observation": {
                    key: last_metrics[key]
                    for key in ("status_code", "row_count", "doc_count")
                    if key in last_metrics
                },
            },
        },
        {},
        {"duration_ms": elapsed_ms, "attempts": attempts, "condition_met": False},
    )
