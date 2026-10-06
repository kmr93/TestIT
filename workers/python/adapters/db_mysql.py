import re
import time
from typing import Any, Dict, Tuple


def execute_mysql_query(
    config: Dict[str, Any],
    inputs: Dict[str, Any],
    secrets: Dict[str, str],
    emit_progress: Any,
) -> Tuple[str, Dict[str, Any], Dict[str, Any], Dict[str, Any]]:
    """Run one bounded, parameterized SELECT against MySQL or MariaDB."""
    emit_progress("connecting_db", {"connector_version": 1})
    query = config.get("query", "").strip()
    if (
        not re.match(r"^SELECT\b", query, re.IGNORECASE)
        or ";" in query
        or re.search(r"\b(INTO\s+(OUTFILE|DUMPFILE)|FOR\s+UPDATE|LOCK\s+IN\s+SHARE\s+MODE|SLEEP\s*\(|BENCHMARK\s*\()", query, re.IGNORECASE)
    ):
        return (
            "ERROR",
            {"code": "READ_ONLY_QUERY_REQUIRED", "message": "Only one read-only SELECT statement is allowed", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    host = config.get("host")
    username = config.get("user")
    database = config.get("database")
    if not host or not username or not database:
        return (
            "ERROR",
            {"code": "CONNECTION_CONFIG_REQUIRED", "message": "Connection host, user, and database are required", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    started = time.time()
    emit_progress("executing_query", {"query_length": len(query)})
    conn = None
    try:
        import pymysql

        max_rows = min(max(int(config.get("max_rows", 100)), 1), 500)
        timeout_seconds = min(max(int(config.get("timeout_seconds", 30)), 1), 3600)
        conn = pymysql.connect(
            host=host,
            port=int(config.get("port", 3306)),
            user=username,
            password=secrets.get(config.get("password_secret", ""), ""),
            database=database,
            cursorclass=pymysql.cursors.SSDictCursor,
            connect_timeout=min(max(int(config.get("connect_timeout_seconds", 10)), 1), 30),
            read_timeout=timeout_seconds,
            write_timeout=10,
            ssl={"check_hostname": True} if config.get("tls", True) else None,
        )
        with conn.cursor() as cursor:
            cursor.execute(query, config.get("params", []))
            rows = cursor.fetchmany(max_rows + 1)
        truncated = len(rows) > max_rows
        rows = rows[:max_rows]
        row_count = len(rows)
        expected_min = config.get("expected_min_rows")
        duration_ms = (time.time() - started) * 1000
        if expected_min is not None and row_count < int(expected_min):
            return (
                "ASSERTION_FAILED",
                {"code": "ROW_COUNT_MISMATCH", "message": f"Expected at least {expected_min} rows, found {row_count}", "class": "ASSERTION", "details": {"actual_rows": row_count, "expected_min": int(expected_min)}},
                {"row_count": row_count},
                {"duration_ms": duration_ms, "row_count": row_count},
            )

        columns = config.get("output_columns", [])
        selected_rows = []
        if isinstance(columns, list):
            for row in rows:
                selected_rows.append({key: row[key] for key in columns if key in row})
        return (
            "SUCCEEDED",
            {},
            {"row_count": row_count, "rows_truncated": truncated, "selected_rows": selected_rows},
            {"duration_ms": duration_ms, "row_count": row_count},
        )
    except Exception:
        return (
            "ERROR",
            {"code": "DB_QUERY_FAILED", "message": "Unable to query the configured MySQL or MariaDB connection", "class": "TRANSIENT_NETWORK", "details": {}},
            {},
            {"duration_ms": (time.time() - started) * 1000},
        )
    finally:
        if conn is not None:
            try:
                conn.close()
            except Exception:
                pass
