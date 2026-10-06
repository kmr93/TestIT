# Workflow Model, Node Catalog, and Reuse

## 1. Authoring model

A suite is an ordered list of test-case revision references. A test case is a directed acyclic graph of node instances. The v1 editor supports a simple ordered sequence as the default, plus explicit condition/branch and cleanup/finally groups.

Definitions have two layers:

- Draft: editable, validated on every save, never used by a run.
- Published revision: immutable JSON with a content checksum and dependency references.

A run pins the exact published suite revision and all reachable asset revisions. “Use latest” may be offered as an explicit author action when updating a draft, but a published suite never resolves latest at execution time.

Cases may declare bounded JSON/CSV data sets. Each row becomes a separately identifiable case iteration with its own inputs, outputs, status, timings, and report entry. The run manifest records the data-set checksum and row count. Secrets are prohibited in data-set files; secret references are supplied by connection profiles. Enforce maximum rows, bytes, and total iterations before starting.

Suite setup runs once before the suite's case iterations; suite teardown runs once after them. Case setup and teardown run for each case iteration. Teardown runs after success, failure, timeout, or cancellation while execution remains available. A teardown failure is visible and cannot convert a failed run to PASS. For parallel data iterations, users must choose isolated records or an environment/resource lock.

## 2. Common node contract

Each node instance has:

    id                 stable UUID within the case revision
    type               versioned adapter type, such as api.request
    type_version       schema version
    name               author-visible label
    config             validated typed values and secret references
    inputs             typed references to case inputs or prior node outputs
    timeout_seconds    required bounded integer
    retry_policy       zero by default; explicit safe retry only
    position           canvas coordinates; presentation only
    on_error            fail, branch-to-handler, or continue only when declared safe

Each invocation receives a run/case/step ID, immutable config, declared inputs, a deadline, and step-scoped credential references. It returns status, typed outputs, sanitized error details, timings, metrics, and artifact references. The worker protocol is versioned independently from the UI canvas.

Variable references are selected from typed pickers and inserted into supported fields. The platform owns a versioned reference format; authors do not need to memorize syntax. Do not accept arbitrary Python, JavaScript, shell, or template expressions in the normal editor.

## 3. v1 node catalog

### Typed variables and built-in function picker

Users can create custom variables as typed literals or compose a value from the built-in function catalog. A custom variable is a named value; user-authored reusable functions and arbitrary code expressions are out of scope.

Supported scopes are environment, run input, suite, case, data-row iteration, and step output. The picker displays both name and scope, for example environment.api_base_url, suite.customer_type, iteration.email, or step.create_customer.output.id. References are explicit; duplicate visible names are rejected unless the author explicitly renames or maps them, so silent scope shadowing does not occur. System values use a reserved sys namespace.

Initial safe built-ins:

- Context: sys.run_id, sys.suite_id, sys.case_id, sys.iteration_id, and sys.started_at_utc.
- Unique/test data: UUID, seeded random integer/string, and a clearly labeled synthetic email generator for reserved test domains.
- String: concatenate, lowercase/uppercase, trim, replace, split/join, and length.
- Numeric/date: add/subtract/round, UTC date formatting, and bounded date arithmetic.
- Structured data: JSON field selection and URL encoding.

The Rust resolver validates types and evaluates functions from a versioned allow-list. A function-backed variable is evaluated once for its declared scope and cached; every reference to that variable within the scope receives the same value. Random functions use a run/iteration seed recorded in the manifest so reruns can reproduce generated test data. A separate variable definition produces a separate generated value. Functions have input, output, length, and execution-time bounds; they cannot access files, networks, environment secrets, or arbitrary code.

Custom variable types include string, integer, decimal, boolean, object, array, datetime, and SecretRef. Secret values are injected through the secret manager and remain tainted/masked; ordinary custom variables cannot contain secret plaintext.

Variable references may be used in API path segments, query values, header values, JSON/form body values, script inputs, and supported assertion operands. JSON bodies use typed value slots so serialization escapes values correctly. SQL/CQL values remain bound parameters; variables cannot alter query structure or identifiers through interpolation.

### Variable preview

The editor can resolve a sample context through the same Rust evaluator used for execution. Preview shows the resolved URL, query, non-secret headers, and body structure; secrets display as masked placeholders. It reports missing variables and type mismatches. Preview never sends an HTTP request, runs a worker, or changes target state. Save/publish validation also checks the function-catalog version and each variable's output type.

### HTTP API request

Fields: connection/base URL, HTTP method, path, query parameters, headers, JSON/form/text body, auth profile reference, timeout, redirect policy, allowed status codes. Common connection auth profiles include bearer token, API key, basic auth, and OAuth 2.0 client credentials with bounded token refresh. OAuth tokens and other credential material are secrets and are never included in run logs.

Assertions: status equals/in-range, header exists/equal, JSON path typed comparison, imported OpenAPI response schema, response time below limit. Extract selected JSON fields into outputs. Do not log authorization headers or full bodies by default. Disable insecure TLS override by default.

### OpenAPI import

Import an OpenAPI description from an uploaded file or an administrator-approved URL. OpenAPI is a language-neutral description format intended to help people and tools understand and interact with HTTP APIs; see the [OpenAPI Specification](https://spec.openapis.org/oas/). Parse and validate it without executing remote references. The importer creates editable API request templates with path/query/header/body fields, operation summaries, auth requirements, and response schemas. Authors choose operations and add test data/assertions; import does not automatically call every operation. Store the source spec checksum and supported format version in the asset revision so later spec drift can be reviewed and re-imported deliberately.

Use Playwright Python APIRequestContext for direct API operations. Automatic retry is off by default; retry only transient transport failures for explicitly idempotent requests or requests carrying a declared idempotency key.

### Shell script / Python script

Authors select an approved script asset and immutable version, then map typed inputs and outputs. Admins upload, review, scan, and publish script versions. Do not include an inline editor in the default no-code experience. An optional admin-only editor can be a later feature.

Execution fields: asset revision/hash, argument map, declared connection references, timeout, expected exit codes, output size cap. No arbitrary package installation, host filesystem mounts, Docker socket, privileged mode, or default internet access. Script artifacts are explicit and size-limited.

### Database validation / extract

Support configured connection profiles and parameterized queries. Validation modes: row count, column/value check, existence, aggregate threshold, or comparison to expected dataset. Extract mode returns selected bounded columns/rows into case variables.

- Require read-only target credentials by default.
- Parameterize values; never string-concatenate user-controlled values into SQL/CQL.
- Require max rows and max bytes.
- Warn when query has no limit and reject unbounded extract.
- Data mutation/setup requires an explicitly approved separate node and credential profile; no automatic DB mutation from a read-only validation node.
- Mongo filters, projections, and aggregation stages use typed JSON forms, with a separately permissioned raw pipeline feature if needed.

### Delta Lake / Parquet validation

Read-only v1 operations: table/file exists, schema comparison, row count, selected-column assertions, and bounded row sample. For Delta, optionally pin a table version. Support local allow-listed paths and S3-compatible URI first. Require explicit storage connection references. Cap scanned bytes, rows, and execution time.

### Sleep / wait

A fixed duration with a strict maximum is available for simple delays. The preferred node is “wait until condition”: poll an API or database assertion at a configurable interval until it passes or a hard deadline expires. Show attempts, elapsed time, last observed status/value (redacted), and timeout outcome. Never allow unbounded waits.

### Transform / extract / assert

Use a constrained, typed expression and JSONPath-like picker, not a general scripting language. Supports string/number/date conversion, field selection, null checks, equality, ranges, contains, and list matching. Fail validation on incompatible types.

### Condition / branch

A condition routes execution to one branch using the typed expression subset. Every path must join or terminate explicitly. Validation catches unreachable nodes and paths that omit required cleanup.

### Reusable case call

References an exact test-case revision and maps typed inputs to that case. The called case's output schema is checked at publish time. The dependency graph must be acyclic and have a maximum depth (recommended 8). Each invocation is recorded as a child execution in the report.

### Cleanup / finally

Run cleanup after success, failure, timeout, or cancellation while the worker remains alive. Cleanup gets its own bounded timeout and explicit credentials. Cleanup failure is visible and cannot turn a failed main path into PASS.

### Environment/resource lock

An author may declare exclusive resources such as a shared environment, tenant, account, or named test fixture. The scheduler acquires the lock before setup and releases it after cleanup. A lock wait has a deadline and visible queued state. Expired or uncertain locks are not silently stolen when the previous run may still have side effects.

## 4. Live run reporting contract

The run page updates from the durable event stream. Show exact completed/total case iterations and node outcomes, status counts, active case/step, retry count, elapsed time, and API request/latency aggregates when available. Mark ETA as estimated and hide it when there is insufficient comparable history. Branches and data-set expansion can change the visible work count; explain that skipped branch nodes are counted as terminal outcomes. See the API and UI documents for payload and display details.

## 5. Variables and sensitive data

- Run manifests pin variable definitions, function-catalog version, data-set hash, run seed, and run start timestamp; do not persist secret plaintext.
- Case inputs declare name, type, required flag, and safe display value. Environment and suite/case custom variable sets are versioned with their owner configuration.
- Step outputs declare type and sensitivity. Secret outputs are tainted; downstream display and logs redact them automatically.
- Only a step may request a SecretRef explicitly declared in its configuration.
- Preview contexts use sample values. Never call the target API or resolve a secret value just to preview a request.
- Large datasets are artifacts, not variable values.

## 6. Status and retry semantics

Node states: PENDING, QUEUED, RUNNING, RETRY_WAIT, SUCCEEDED, ASSERTION_FAILED, ERROR, TIMED_OUT, CANCELED, SKIPPED, INTERRUPTED. RETRY_WAIT is a bounded delay before an explicitly allowed attempt; it is separate from terminal progress.

Retry policy is zero attempts beyond the first by default. If enabled, the author chooses maximum attempts, eligible error classes, delay/backoff, and confirms idempotency. The platform does not infer that a remote side effect is safe to repeat. Retry attempts are shown separately in the report.

## 7. Validation before publish/run

Reject or flag:

- missing required config, invalid graph edge, duplicate node ID, unsupported schema version;
- undeclared variable, type mismatch, reference to a later node on a sequential path;
- unknown/disabled built-in function, unsupported function-catalog version, invalid function argument, or secret reference used in an unsafe field;
- missing/unavailable environment connection or permission;
- unpinned or missing dependency revision;
- cycle or excessive reusable-case depth;
- timeout/row/output size beyond policy;
- script asset not approved for the workspace;
- URI/path outside the administrator's allowlist;
- unsafe retry on a non-idempotent action without an explicit idempotency design.

