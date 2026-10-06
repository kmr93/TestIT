"""Authenticated, fixed-policy launcher for one-shot Docker node workers."""

import datetime as dt
import hmac
import http.client
import ipaddress
import json
import os
import socket
import tarfile
import threading
import time
import urllib.parse
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO


API_VERSION = os.getenv("DOCKER_API_VERSION", "v1.43")
DOCKER_SOCKET = os.getenv("DOCKER_SOCKET", "/var/run/docker.sock")
WORKER_IMAGE = os.getenv("WORKER_IMAGE", "testit-worker-runtime:local")
WORKER_NETWORK = os.getenv("WORKER_NETWORK", "testit-worker-net")
WORKER_ALLOWED_HOSTS = tuple(
    item.strip().lower()
    for item in os.getenv("WORKER_ALLOWED_HOSTS", "").split(",")
    if item.strip()
)
MANAGER_TOKEN = os.getenv("WORKER_MANAGER_TOKEN", "")
MAX_REQUEST_BYTES = 1_048_576
ALLOWED_NODE_TYPES = {"api.request", "wait.until", "db.mysql", "db.mongodb", "data.tabular", "sleep.wait"}
ACTIVE_INVOCATIONS = threading.BoundedSemaphore(4)
CANCELLED_RUNS = {}
CANCEL_LOCK = threading.Lock()


class InvocationError(Exception):
    def __init__(self, status, code, message):
        super().__init__(message)
        self.status = status
        self.code = code


class UnixHTTPConnection(http.client.HTTPConnection):
    def __init__(self, path, timeout):
        super().__init__("localhost", timeout=timeout)
        self.unix_path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.unix_path)


def docker_request(method, path, body=None, headers=None, timeout=20):
    connection = UnixHTTPConnection(DOCKER_SOCKET, timeout)
    request_headers = dict(headers or {})
    if body is not None and "Content-Type" not in request_headers:
        request_headers["Content-Type"] = "application/json"
    connection.request(method, f"/{API_VERSION}{path}", body=body, headers=request_headers)
    response = connection.getresponse()
    response_body = response.read()
    status = response.status
    connection.close()
    if status >= 300:
        detail = response_body[:512].decode("utf-8", errors="replace")
        raise InvocationError(502, "CONTAINER_ENGINE_ERROR", f"Container engine request failed ({status}): {detail}")
    return status, response_body


def target_host(envelope):
    node_type = envelope["node_type"]
    config = envelope.get("config") or {}
    if node_type == "wait.until" and config.get("target") in {"mysql", "mongodb"}:
        if config.get("tls") is False:
            raise InvocationError(403, "TLS_REQUIRED", "Database connector TLS must remain enabled")
        if config.get("target") == "mysql":
            host = config.get("host")
            if not host:
                raise InvocationError(422, "DB_HOST_REQUIRED", "MySQL waits need an explicit connection host")
        else:
            raw_uri = config.get("uri")
            if raw_uri:
                parsed = urllib.parse.urlsplit(raw_uri)
                if parsed.username or parsed.password:
                    raise InvocationError(422, "DB_CREDENTIALS_IN_URI", "MongoDB credentials must be supplied through a secret reference, not a connection URL")
                query_options = urllib.parse.parse_qs(parsed.query)
                if query_options.get("tlsInsecure", ["false"])[0].lower() == "true" or query_options.get("tlsAllowInvalidCertificates", ["false"])[0].lower() == "true":
                    raise InvocationError(403, "TLS_VERIFICATION_REQUIRED", "MongoDB TLS certificate verification cannot be disabled")
                host = parsed.hostname
            else:
                host = config.get("host")
            if not host:
                raise InvocationError(422, "DB_HOST_REQUIRED", "MongoDB waits need an explicit connection host")
    elif node_type in {"api.request", "wait.until"}:
        raw_url = config.get("url") or config.get("path")
        if not raw_url:
            raise InvocationError(422, "API_URL_REQUIRED", "API request nodes need an explicit URL")
        parsed = urllib.parse.urlsplit(raw_url)
        if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password:
            raise InvocationError(422, "API_URL_INVALID", "API URL must use HTTP(S), include a host, and omit embedded credentials")
        if any(any(marker in key.lower() for marker in ("token", "key", "password", "secret", "credential", "authorization", "cookie")) for key, _ in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)):
            raise InvocationError(422, "API_URL_CREDENTIAL_DISALLOWED", "Credentials must use an encrypted secret reference, not URL query parameters")
        host = parsed.hostname
    elif node_type == "db.mysql":
        if config.get("tls") is False:
            raise InvocationError(403, "TLS_REQUIRED", "Database connector TLS must remain enabled")
        host = config.get("host")
        if not host:
            raise InvocationError(422, "DB_HOST_REQUIRED", "MySQL nodes need an explicit connection host")
    elif node_type == "db.mongodb":
        if config.get("tls") is False:
            raise InvocationError(403, "TLS_REQUIRED", "Database connector TLS must remain enabled")
        raw_uri = config.get("uri")
        if raw_uri:
            parsed = urllib.parse.urlsplit(raw_uri)
            if parsed.username or parsed.password:
                raise InvocationError(422, "DB_CREDENTIALS_IN_URI", "MongoDB credentials must be supplied through a secret reference, not a connection URL")
            query_options = urllib.parse.parse_qs(parsed.query)
            if query_options.get("tlsInsecure", ["false"])[0].lower() == "true" or query_options.get("tlsAllowInvalidCertificates", ["false"])[0].lower() == "true":
                raise InvocationError(403, "TLS_VERIFICATION_REQUIRED", "MongoDB TLS certificate verification cannot be disabled")
            host = parsed.hostname
        else:
            host = config.get("host")
        if not host:
            raise InvocationError(422, "DB_HOST_REQUIRED", "MongoDB nodes need an explicit connection host")
    elif node_type == "data.tabular":
        path = config.get("path", "")
        parsed = urllib.parse.urlsplit(path)
        if parsed.scheme in {"s3", "http", "https"}:
            host = parsed.hostname or parsed.netloc
        else:
            raise InvocationError(422, "DATA_PATH_UNAVAILABLE", "Local data-file mounts are not configured for isolated workers")
    else:
        return None, []

    host = str(host).strip("[]").lower().rstrip(".")
    if not host or not is_allowed_host(host):
        raise InvocationError(403, "EGRESS_DESTINATION_DENIED", "The destination host is not on the worker egress allow-list")
    try:
        addresses = sorted({item[4][0] for item in socket.getaddrinfo(host, None, type=socket.SOCK_STREAM)})
    except OSError:
        raise InvocationError(422, "DESTINATION_DNS_FAILED", "The destination host did not resolve")
    if not addresses:
        raise InvocationError(422, "DESTINATION_DNS_FAILED", "The destination host did not resolve")
    for value in addresses:
        try:
            address = ipaddress.ip_address(value)
        except ValueError:
            raise InvocationError(422, "DESTINATION_DNS_INVALID", "The destination resolved to an invalid address")
        if address.is_loopback or address.is_link_local or value == "169.254.169.254":
            raise InvocationError(403, "DESTINATION_BLOCKED", "Loopback and link-local destinations are blocked")
    return host, addresses


def is_allowed_host(host):
    for allowed in WORKER_ALLOWED_HOSTS:
        if allowed.startswith("*."):
            suffix = allowed[1:]
            if host.endswith(suffix) and host != suffix[1:]:
                return True
        elif hmac.compare_digest(host, allowed):
            return True
    return False


def validate_envelope(envelope):
    if not isinstance(envelope, dict) or envelope.get("schema_version") != 1:
        raise InvocationError(400, "INVALID_ENVELOPE", "Invocation schema version 1 is required")
    for key in ("run_id", "case_id", "step_id"):
        try:
            uuid.UUID(envelope.get(key, ""))
        except (ValueError, TypeError, AttributeError):
            raise InvocationError(400, "INVALID_ENVELOPE", f"{key} must be a UUID")
    if envelope.get("node_type") not in ALLOWED_NODE_TYPES:
        raise InvocationError(422, "NODE_TYPE_UNAVAILABLE", "This node type is not enabled in the isolated worker runtime")
    timeout = (envelope.get("limits") or {}).get("timeout_seconds", 30)
    if not isinstance(timeout, int) or timeout < 1 or timeout > 3600:
        raise InvocationError(400, "INVALID_LIMIT", "Worker timeout must be between 1 and 3600 seconds")
    config = envelope.get("config", {})
    inputs = envelope.get("inputs", {})
    secrets = envelope.get("secrets", {})
    if not isinstance(config, dict) or not isinstance(inputs, dict) or not isinstance(secrets, dict):
        raise InvocationError(400, "INVALID_ENVELOPE", "config, inputs, and secrets must be objects")
    if envelope.get("node_type") == "wait.until" and config.get("target", "api") not in {"api", "mysql", "mongodb"}:
        raise InvocationError(422, "WAIT_TARGET_UNSUPPORTED", "Wait-until target must be API, MySQL, or MongoDB")
    if has_inline_credentials(config):
        raise InvocationError(422, "INLINE_CREDENTIAL_DISALLOWED", "Credentials must use encrypted secret references")
    if len(json.dumps(envelope, separators=(",", ":")).encode("utf-8")) > MAX_REQUEST_BYTES:
        raise InvocationError(413, "INVOCATION_TOO_LARGE", "Invocation payload exceeds the configured limit")
    deadline = envelope.get("deadline_utc")
    if not isinstance(deadline, str):
        raise InvocationError(400, "INVALID_ENVELOPE", "deadline_utc is required")
    try:
        parsed_deadline = dt.datetime.fromisoformat(deadline.replace("Z", "+00:00"))
        remaining = (parsed_deadline - dt.datetime.now(dt.timezone.utc)).total_seconds()
    except ValueError:
        raise InvocationError(400, "INVALID_ENVELOPE", "deadline_utc must be an ISO-8601 timestamp")
    if remaining <= 0:
        raise InvocationError(408, "DEADLINE_EXPIRED", "The node deadline elapsed before worker startup")
    host, addresses = target_host(envelope)
    return min(timeout, max(1, int(remaining))), host, addresses


def has_inline_credentials(value):
    markers = ("password", "token", "authorization", "cookie", "api_key", "apikey", "credential")
    if isinstance(value, dict):
        for key, nested in value.items():
            name = str(key).lower()
            is_reference = name.endswith("_secret") or name in {"secret_ref", "secret_refs"}
            if not is_reference and any(marker in name for marker in markers) and nested is not None:
                return True
            if name in {"url", "uri", "base_url"} and isinstance(nested, str):
                parsed = urllib.parse.urlsplit(nested)
                query_has_secret = any(
                    any(marker in query_key.lower() for marker in markers)
                    for query_key, _ in urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
                )
                if parsed.username or parsed.password or query_has_secret:
                    return True
            if has_inline_credentials(nested):
                return True
    elif isinstance(value, list):
        return any(has_inline_credentials(item) for item in value)
    return False


def make_tar(envelope):
    encoded = json.dumps(envelope, separators=(",", ":")).encode("utf-8")
    archive = BytesIO()
    with tarfile.open(fileobj=archive, mode="w") as tar:
        info = tarfile.TarInfo("testit-envelope.json")
        info.size = len(encoded)
        info.mode = 0o400
        info.uid = 10001
        info.gid = 10001
        info.uname = "testit"
        info.gname = "testit"
        tar.addfile(info, BytesIO(encoded))
    return archive.getvalue()


def create_worker(envelope, timeout, host, addresses):
    step_id = envelope["step_id"]
    name = f"testit-step-{step_id}"
    extra_hosts = [f"{host}:{address}" for address in addresses] if host else []
    config = {
        "Image": WORKER_IMAGE,
        "Entrypoint": ["python", "/app/main.py", "/tmp/testit-envelope.json"],
        "User": "10001:10001",
        "WorkingDir": "/tmp",
        "AttachStdout": True,
        "AttachStderr": True,
        "Tty": False,
        "OpenStdin": False,
        "Labels": {"com.testit.managed": "true", "com.testit.step_id": step_id, "com.testit.run_id": envelope["run_id"]},
        "HostConfig": {
            "AutoRemove": False,
            "ReadonlyRootfs": True,
            "CapDrop": ["ALL"],
            "SecurityOpt": ["no-new-privileges:true"],
            "PidsLimit": 128,
            "Memory": 536870912,
            "NanoCpus": 1000000000,
            "NetworkMode": WORKER_NETWORK,
            "Tmpfs": {"/tmp": "rw,noexec,nosuid,nodev,size=16777216,uid=10001,gid=10001"},
            "ExtraHosts": extra_hosts,
            "LogConfig": {"Type": "json-file", "Config": {"max-size": "1m", "max-file": "1"}},
        },
    }
    path = "/containers/create?" + urllib.parse.urlencode({"name": name})
    _, body = docker_request("POST", path, json.dumps(config).encode("utf-8"), timeout=20)
    container_id = json.loads(body)["Id"]
    return container_id


def docker_logs(container_id, max_bytes, timeout):
    _, data = docker_request(
        "GET",
        f"/containers/{container_id}/logs?stdout=1&stderr=1&follow=0&timestamps=0",
        timeout=timeout + 15,
    )
    if len(data) > max_bytes:
        raise InvocationError(413, "WORKER_OUTPUT_LIMIT", "Worker output exceeded the configured size limit")
    stdout = bytearray()
    cursor = 0
    while cursor + 8 <= len(data):
        stream_id = data[cursor]
        size = int.from_bytes(data[cursor + 4 : cursor + 8], "big")
        cursor += 8
        if size < 0 or cursor + size > len(data):
            raise InvocationError(502, "WORKER_PROTOCOL_ERROR", "Container log frame was incomplete")
        if stream_id == 1:
            stdout.extend(data[cursor : cursor + size])
        cursor += size
    return stdout.decode("utf-8", errors="replace")


def invoke(envelope):
    timeout, host, addresses = validate_envelope(envelope)
    cleanup = envelope.get("cleanup") is True
    if not ACTIVE_INVOCATIONS.acquire(blocking=False):
        raise InvocationError(429, "WORKER_CAPACITY", "All isolated worker slots are busy")
    container_id = None
    started = time.monotonic()
    try:
        if not cleanup and is_run_cancelled(envelope["run_id"]):
            return cancelled_result(envelope["step_id"], started)
        container_id = create_worker(envelope, timeout, host, addresses)
        archive = make_tar(envelope)
        docker_request(
            "PUT",
            f"/containers/{container_id}/archive?path=%2Ftmp",
            archive,
            headers={"Content-Type": "application/x-tar"},
        )
        docker_request("POST", f"/containers/{container_id}/start", timeout=20)
        try:
            docker_request(
                "POST",
                f"/containers/{container_id}/wait?condition=not-running",
                timeout=timeout + 15,
            )
        except (TimeoutError, socket.timeout):
            return {
                "result": {
                    "frame_type": "result",
                    "step_id": envelope["step_id"],
                    "status": "TIMED_OUT",
                    "error": {
                        "code": "WORKER_TIMED_OUT",
                        "message": f"Node exceeded its {timeout}s wall-clock deadline",
                        "class": "TIMEOUT",
                        "details": {},
                    },
                    "outputs": {},
                    "metrics": {"duration_ms": (time.monotonic() - started) * 1000},
                    "artifacts": [],
                    "redacted_keys": [],
                }
            }
        if not cleanup and is_run_cancelled(envelope["run_id"]):
            return cancelled_result(envelope["step_id"], started)
        limit = min((envelope.get("limits") or {}).get("max_output_bytes", 1048576), 4 * 1024 * 1024)
        log_limit = min((envelope.get("limits") or {}).get("max_log_bytes", 524288), 2 * 1024 * 1024)
        text = docker_logs(container_id, limit + log_limit, timeout)
        progress = []
        results = []
        for line in text.splitlines():
            if not line.strip():
                continue
            try:
                frame = json.loads(line)
            except json.JSONDecodeError:
                continue
            if frame.get("frame_type") == "progress":
                if frame.get("step_id") == envelope["step_id"]:
                    progress.append(frame)
            elif frame.get("frame_type") == "result":
                results.append(frame)
        if len(results) != 1 or results[0].get("step_id") != envelope["step_id"]:
            raise InvocationError(502, "WORKER_PROTOCOL_ERROR", "Worker must emit exactly one result frame for the invoked step")
        return {"result": results[0], "progress": progress}
    finally:
        if container_id:
            try:
                docker_request("DELETE", f"/containers/{container_id}?force=1&v=1", timeout=10)
            except Exception:
                pass
        ACTIVE_INVOCATIONS.release()


def cancelled_result(step_id, started):
    return {
        "result": {
            "frame_type": "result",
            "step_id": step_id,
            "status": "CANCELED",
            "error": {"code": "RUN_CANCELED", "message": "The run was canceled by an authorized user", "class": "INTERNAL", "details": {}},
            "outputs": {},
            "metrics": {"duration_ms": (time.monotonic() - started) * 1000},
            "artifacts": [],
            "redacted_keys": [],
        }
    }


def is_run_cancelled(run_id):
    now = time.monotonic()
    with CANCEL_LOCK:
        expired = [key for key, expires in CANCELLED_RUNS.items() if expires <= now]
        for key in expired:
            CANCELLED_RUNS.pop(key, None)
        return CANCELLED_RUNS.get(run_id, 0) > now


def cancel_run(run_id):
    try:
        uuid.UUID(run_id)
    except (ValueError, TypeError, AttributeError):
        raise InvocationError(400, "INVALID_RUN_ID", "run_id must be a UUID")
    with CANCEL_LOCK:
        CANCELLED_RUNS[run_id] = time.monotonic() + 300
    filters = urllib.parse.urlencode({"filters": json.dumps({"label": [f"com.testit.run_id={run_id}"]})})
    _, response = docker_request("GET", f"/containers/json?all=0&{filters}", timeout=10)
    containers = json.loads(response)
    stopped = 0
    failures = 0
    for container in containers:
        container_id = container.get("Id")
        if not container_id:
            continue
        try:
            docker_request("POST", f"/containers/{container_id}/stop?t=2", timeout=5)
            stopped += 1
        except Exception:
            failures += 1
    if failures:
        raise InvocationError(502, "CANCELLATION_FAILED", "The container engine could not stop every worker for this run")
    return {"status": "cancel_requested", "stopped_workers": stopped}


def remove_orphaned_workers():
    """Remove containers left behind if the manager process was interrupted."""
    filters = urllib.parse.urlencode({"filters": json.dumps({"label": ["com.testit.managed=true"]})})
    _, response = docker_request("GET", f"/containers/json?all=1&{filters}", timeout=20)
    containers = json.loads(response)
    removed = 0
    for container in containers:
        container_id = container.get("Id")
        if not container_id:
            continue
        docker_request("DELETE", f"/containers/{container_id}?force=1&v=1", timeout=20)
        removed += 1
    print(f"worker-manager removed {removed} orphaned worker container(s)", flush=True)


class Handler(BaseHTTPRequestHandler):
    server_version = "TestITWorkerManager/1"

    def log_message(self, format_string, *args):
        # Never log request bodies, authorization headers, or connector details.
        print(f"worker-manager {self.client_address[0]} {self.command} {self.path.split('?')[0]}", flush=True)

    def send_json(self, status, value):
        encoded = json.dumps(value, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(encoded)

    def do_GET(self):
        if self.path == "/health/live":
            self.send_json(200, {"status": "ok", "service": "testit-worker-manager"})
        else:
            self.send_json(404, {"error": "not_found"})

    def do_POST(self):
        if self.path not in {"/v1/invoke", "/v1/cancel"}:
            self.send_json(404, {"error": "not_found"})
            return
        authorization = self.headers.get("Authorization", "")
        provided = authorization.removeprefix("Bearer ")
        if not MANAGER_TOKEN or not hmac.compare_digest(provided, MANAGER_TOKEN):
            self.send_json(401, {"error": "unauthorized"})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if length <= 0 or length > MAX_REQUEST_BYTES:
                raise InvocationError(413, "INVOCATION_TOO_LARGE", "Invocation payload exceeds the configured limit")
            payload = json.loads(self.rfile.read(length))
            result = invoke(payload) if self.path == "/v1/invoke" else cancel_run(payload.get("run_id"))
            self.send_json(200, result)
        except InvocationError as error:
            self.send_json(error.status, {"error": error.code, "message": str(error)})
        except (json.JSONDecodeError, UnicodeDecodeError):
            self.send_json(400, {"error": "INVALID_JSON", "message": "Invocation payload must be valid JSON"})
        except Exception:
            self.send_json(502, {"error": "WORKER_LAUNCH_FAILED", "message": "Isolated worker launch failed"})


if __name__ == "__main__":
    if len(MANAGER_TOKEN) < 64:
        raise SystemExit("WORKER_MANAGER_TOKEN must be at least 64 characters")
    remove_orphaned_workers()
    server = ThreadingHTTPServer(("0.0.0.0", int(os.getenv("WORKER_MANAGER_PORT", "8081"))), Handler)
    server.daemon_threads = True
    server.serve_forever()
