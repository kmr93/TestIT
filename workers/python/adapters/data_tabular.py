import time
from typing import Any, Dict, Tuple


def execute_tabular_check(
    config: Dict[str, Any],
    inputs: Dict[str, Any],
    secrets: Dict[str, str],
    emit_progress: Any,
) -> Tuple[str, Dict[str, Any], Dict[str, Any], Dict[str, Any]]:
    """Read tabular metadata and a bounded row count without returning row data."""
    format_type = str(config.get("format", "parquet")).lower()
    path = config.get("path", "")
    if format_type not in {"parquet", "delta"} or not path:
        return (
            "ERROR",
            {"code": "TABULAR_CONFIG_REQUIRED", "message": "A supported format and path are required", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": 0.0},
        )

    started = time.time()
    emit_progress("reading_tabular_metadata", {"format_version": 1})
    try:
        max_rows = min(max(int(config.get("max_rows", 100000)), 1), 1_000_000)
        if format_type == "parquet":
            import pyarrow.parquet as parquet

            file = parquet.ParquetFile(path)
            metadata = file.metadata
            row_count = metadata.num_rows
            schema = [field.name for field in file.schema_arrow]
        else:
            from deltalake import DeltaTable

            table = DeltaTable(path, version=config.get("version"))
            schema = [field.name for field in table.schema().fields]
            batch_size = min(max_rows, 8192)
            max_scan_bytes = min(max(int(config.get("max_scan_bytes", 67_108_864)), 1_048_576), 536_870_912)
            row_count = 0
            scanned_bytes = 0
            truncated = False
            for batch in table.to_pyarrow_dataset().to_batches(batch_size=batch_size):
                row_count += batch.num_rows
                scanned_bytes += batch.nbytes
                if row_count >= max_rows or scanned_bytes >= max_scan_bytes:
                    truncated = True
                    break

        if format_type == "parquet":
            truncated = row_count > max_rows
            row_count = min(row_count, max_rows)
        duration_ms = (time.time() - started) * 1000
        expected_min = config.get("expected_min_rows")
        metrics = {"duration_ms": duration_ms, "row_count": row_count}
        if expected_min is not None and row_count < int(expected_min):
            return (
                "ASSERTION_FAILED",
                {"code": "ROW_COUNT_MISMATCH", "message": f"Expected at least {expected_min} rows, found {row_count}", "class": "ASSERTION", "details": {"actual_rows": row_count, "expected_min": int(expected_min)}},
                {"row_count": row_count, "schema": schema},
                metrics,
            )
        return "SUCCEEDED", {}, {"row_count": row_count, "schema": schema, "truncated": truncated}, metrics
    except Exception:
        return (
            "ERROR",
            {"code": "TABULAR_READ_FAILED", "message": "Unable to read the configured tabular source", "class": "INTERNAL", "details": {}},
            {},
            {"duration_ms": (time.time() - started) * 1000},
        )
