import time
from typing import Any, Dict, Tuple


def execute_mongodb_query(
    config: Dict[str, Any],
    inputs: Dict[str, Any],
    secrets: Dict[str, str],
    emit_progress: Any,
) -> Tuple[str, Dict[str, Any], Dict[str, Any], Dict[str, Any]]:
    """Run a bounded find operation and report counts without persisting documents."""
    emit_progress("connecting_db", {"connector_version": 1})
    collection_name = config.get("collection", "")
    database_name = config.get("database", "")
    uri = config.get("uri")
    host = config.get("host")
    health_check = config.get("health_check") is True
    if not (uri or host) or (not health_check and (not collection_name or not database_name)):
        return (
            "ERROR",
            {"code": "CONNECTION_CONFIG_REQUIRED", "message": "MongoDB host, database, and collection are required", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    started = time.time()
    client = None
    try:
        from pymongo import MongoClient

        timeout_ms = min(max(int(config.get("timeout_seconds", 30)) * 1000, 1000), 3600000)
        max_docs = min(max(int(config.get("max_documents", 100)), 1), 500)
        options = {
            "username": config.get("user") or None,
            "password": secrets.get(config.get("password_secret", ""), "") or None,
            "tls": True,
            "serverSelectionTimeoutMS": min(max(int(config.get("connect_timeout_ms", 5000)), 1000), 30000),
            "socketTimeoutMS": timeout_ms,
            "connectTimeoutMS": min(max(int(config.get("connect_timeout_ms", 5000)), 1000), 30000),
        }
        if not uri:
            options["port"] = int(config.get("port", 27017))
        client = MongoClient(uri or host, **options)
        if health_check:
            client.admin.command("ping")
            duration_ms = (time.time() - started) * 1000
            return "SUCCEEDED", {}, {"connected": True}, {"duration_ms": duration_ms}
        collection = client[database_name][collection_name]
        emit_progress("executing_query", {"attempt": 1})
        cursor = collection.find(
            config.get("filter", {}),
            config.get("projection"),
            max_time_ms=timeout_ms,
        ).limit(max_docs + 1)
        count = 0
        for _ in cursor:
            count += 1
        truncated = count > max_docs
        count = min(count, max_docs)
        duration_ms = (time.time() - started) * 1000
        expected_min = config.get("expected_min_count")
        metrics = {"duration_ms": duration_ms, "doc_count": count}
        if expected_min is not None and count < int(expected_min):
            return (
                "ASSERTION_FAILED",
                {"code": "DOC_COUNT_MISMATCH", "message": f"Expected at least {expected_min} documents, found {count}", "class": "ASSERTION", "details": {"actual_count": count, "expected_min": int(expected_min)}},
                {"count": count},
                metrics,
            )
        return "SUCCEEDED", {}, {"count": count, "truncated": truncated}, metrics
    except Exception:
        return (
            "ERROR",
            {"code": "MONGODB_QUERY_FAILED", "message": "Unable to query the configured MongoDB connection", "class": "TRANSIENT_NETWORK", "details": {}},
            {},
            {"duration_ms": (time.time() - started) * 1000},
        )
    finally:
        if client is not None:
            try:
                client.close()
            except Exception:
                pass
