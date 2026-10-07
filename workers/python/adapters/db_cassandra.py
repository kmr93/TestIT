import json
import re
import ssl
import time
from typing import Any, Dict, Tuple


def execute_cassandra_query(
    config: Dict[str, Any],
    inputs: Dict[str, Any],
    secrets: Dict[str, str],
    emit_progress: Any,
) -> Tuple[str, Dict[str, Any], Dict[str, Any], Dict[str, Any]]:
    """Execute a bounded, parameterized Cassandra SELECT over verified TLS."""
    query = config.get("query", "").strip()
    if (
        not re.match(r"^SELECT\b", query, re.IGNORECASE)
        or ";" in query
        or re.search(r"\bALLOW\s+FILTERING\b", query, re.IGNORECASE)
    ):
        return (
            "ERROR",
            {"code": "READ_ONLY_QUERY_REQUIRED", "message": "Only one bounded read-only Cassandra SELECT is allowed", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    host = str(config.get("host", "")).strip()
    username = str(config.get("user", "")).strip()
    keyspace = str(config.get("keyspace", "")).strip()
    table_match = re.search(r"\bFROM\s+([A-Za-z_][A-Za-z0-9_.]*)", query, re.IGNORECASE)
    table_name = table_match.group(1) if table_match else ""
    if not host or len(query) > 16_384 or not table_name or ("." not in table_name and not keyspace):
        return (
            "ERROR",
            {"code": "CONNECTION_CONFIG_REQUIRED", "message": "Cassandra host, keyspace, and read-only query are required", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )
    if config.get("tls", True) is not True:
        return (
            "ERROR",
            {"code": "TLS_REQUIRED", "message": "Cassandra connections require TLS certificate verification", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    params = config.get("params", [])
    columns = config.get("output_columns", [])
    try:
        if not isinstance(params, list) or len(params) > 100:
            raise ValueError("invalid bound parameter list")
        if not isinstance(columns, list) or len(columns) > 32 or any(not isinstance(item, str) for item in columns):
            raise ValueError("invalid output column list")
        timeout_seconds = min(max(int(config.get("timeout_seconds", 30)), 1), 3600)
        max_rows = min(max(int(config.get("max_rows", 100)), 1), 500)
        max_output_bytes = min(max(int(config.get("max_output_bytes", 262144)), 1024), 1_048_576)
        expected_min = min(max(int(config.get("expected_min_rows", 0)), 0), 500)
    except (TypeError, ValueError):
        return (
            "ERROR",
            {"code": "CASSANDRA_CONFIG_INVALID", "message": "Cassandra query limits, parameters, or output columns are invalid", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    started = time.time()
    cluster = None
    session = None
    try:
        from cassandra.auth import PlainTextAuthProvider
        from cassandra.cluster import Cluster
        from cassandra.query import SimpleStatement

        ca_certificate = secrets.get(str(config.get("ca_certificate_secret", "")), "")
        tls_context = ssl.create_default_context(cadata=ca_certificate or None)
        auth_provider = None
        if username:
            password = secrets.get(str(config.get("password_secret", "")), "")
            if not password:
                raise ValueError("referenced Cassandra credential unavailable")
            auth_provider = PlainTextAuthProvider(username=username, password=password)

        emit_progress("connecting_db", {"connector_version": 1})
        cluster = Cluster(
            contact_points=[host],
            port=int(config.get("port", 9042)),
            auth_provider=auth_provider,
            ssl_context=tls_context,
            connect_timeout=min(timeout_seconds, 30),
            control_connection_timeout=min(timeout_seconds, 30),
        )
        session = cluster.connect(keyspace or None)
        session.default_timeout = timeout_seconds
        emit_progress("executing_query", {"query_length": len(query)})
        statement = SimpleStatement(query, fetch_size=max_rows + 1)
        result = session.execute(statement, params, timeout=timeout_seconds)
        selected_rows = []
        row_count = 0
        output_size = 0
        for row in result:
            row_count += 1
            if columns:
                row_map = row._asdict()
                selected = {name: row_map[name] for name in columns if name in row_map}
                normalized = json.loads(json.dumps(selected, default=str))
                output_size += len(json.dumps(normalized, separators=(",", ":")).encode("utf-8"))
                if output_size > max_output_bytes:
                    raise OverflowError("selected result exceeded the output bound")
                selected_rows.append(normalized)
            if row_count > max_rows:
                break

        duration_ms = (time.time() - started) * 1000
        truncated = row_count > max_rows
        observed_rows = min(row_count, max_rows)
        outputs: Dict[str, Any] = {"row_count": observed_rows, "rows_truncated": truncated}
        if columns:
            outputs["selected_rows"] = selected_rows[:max_rows]
        metrics = {"duration_ms": duration_ms, "row_count": observed_rows}
        if observed_rows < expected_min:
            return (
                "ASSERTION_FAILED",
                {"code": "ROW_COUNT_MISMATCH", "message": f"Expected at least {expected_min} rows, found {observed_rows}", "class": "ASSERTION", "details": {"actual_rows": observed_rows, "expected_min": expected_min}},
                outputs,
                metrics,
            )
        return "SUCCEEDED", {}, outputs, metrics
    except OverflowError:
        return (
            "ERROR",
            {"code": "CASSANDRA_OUTPUT_TOO_LARGE", "message": "Selected Cassandra output exceeded the configured size limit", "class": "CLIENT_ERROR", "details": {}},
            {},
            {"duration_ms": (time.time() - started) * 1000},
        )
    except Exception:
        return (
            "ERROR",
            {"code": "CASSANDRA_QUERY_FAILED", "message": "Unable to query the configured Cassandra connection", "class": "TRANSIENT_NETWORK", "details": {}},
            {},
            {"duration_ms": (time.time() - started) * 1000},
        )
    finally:
        if session is not None:
            try:
                session.shutdown()
            except Exception:
                pass
        if cluster is not None:
            try:
                cluster.shutdown()
            except Exception:
                pass
