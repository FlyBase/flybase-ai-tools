# flybase-mcp

The FlyBase MCP server: a Rust crate (library plus the `flybase-mcp` binary) that serves FlyBase data over
MCP Streamable HTTP. Tools are listed with their schemas in [`docs/tools.md`](../docs/tools.md); running it
in production is covered in [`docs/deployment.md`](../docs/deployment.md).

## Toolchain

Stable Rust with `clippy` and `rustfmt` (`rust-toolchain.toml` pins the channel). If cargo is not on your
`PATH`:

    export PATH="$HOME/.cargo/bin:$PATH"

Run every command below from `mcp/`.

## Databases

The integration tests need a chado database with the `mcp` migrations applied. They read `CHADO_URL` and
default to a full release restore running locally in Docker on port 5498:

    postgres://flybase_mcp:flybase_mcp@127.0.0.1:5498/flybase

CI uses the small fixture database instead, on port 5499. See [`fixture/README.md`](../fixture/README.md)
for `export.sh` and `load.sh`. To test against the fixture locally, start it and override the default:

    fixture/load.sh                              # from the repo root
    CHADO_URL=postgres://flybase_mcp:flybase_mcp@127.0.0.1:5499/flybase cargo test --tests

The test server applies migrations only when the startup check fails. Set `FLYBASE_TEST_MIGRATE=1` to force
a rebuild after editing a migration; it takes about a minute and locks the derived tables.

## Tests

    cargo test --lib       # unit tests, no database
    cargo test --tests     # integration tests in tests/, need CHADO_URL
    cargo fmt && cargo clippy --all-targets -- -D warnings

Each file in `tests/` is its own binary and starts one in-process server on a random port, shared by its
tests (`tests/support/server.rs`). Use `support::server::call` for a successful call's structured content
and `call_raw` to assert on errors.

## Layout

- `src/server.rs`: `FlyBaseServer`, `all_tool_routers`, the axum app (`/mcp`, `/health`, `/metrics`).
- `src/tools/`: one module per tool, each with its own `#[tool_router]`.
- `src/report/`: `get_report` section code, one `ReportType` per ID prefix.
- `src/backends/`: the connection pool (`chado.rs`) and the named-query registry (`sql.rs`).
- `src/limits.rs`, `src/output.rs`, `src/error.rs`: permits, the result envelope, and error classes.
- `sql/queries/<tool>/*.sql`: one query per file, parameters bound as `$1`, `$2`, ...
- `sql/migrations/NNN_name.sql`: idempotent migrations run by `flybase-mcp migrate`.
- `src/bin/tool-catalog.rs`: prints `docs/tools.md`.

## Handler pattern

Every tool handler has the same shape:

1. Take a permit with `self.limits.light(T)` (or `heavy`) so database work stays bounded globally. A permit
   that does not arrive within 5 s becomes a "busy" error.
2. Check out one connection with `self.chado.get(T)` and run every query for the call on it with
   `Chado::query_on(&conn, T, "tool/query_name", &[...])`. `self.chado.query` is a shortcut for a call
   that runs a single query. Never check out a second connection inside a call; under load the pool can
   deadlock against itself.
3. Build the result with `Envelope::new(T, &self.release)` and return `ToolResult<CallToolResult>`.
4. Exit through `output::complete(T, started, res)`, which records metrics and turns a `ToolError` into an
   MCP error result. The public method returns `Ok(complete(...))`; the work lives in an `*_impl` method.

`src/tools/species.rs` is the smallest example.

## Adding a tool

1. Create `src/tools/<tool>.rs` with a params struct (`Deserialize`, `JsonSchema`,
   `#[serde(deny_unknown_fields)]`, a doc comment on every field) and an
   `#[tool_router(router = tool_router_<tool>, vis = "pub")] impl FlyBaseServer` block. The description is
   one or two sentences plus a "Use when" line that separates it from neighbouring tools.
2. Add the module to `src/tools/mod.rs` and its router to `FlyBaseServer::all_tool_routers` in
   `src/server.rs`.
3. Put its SQL in `sql/queries/<tool>/<name>.sql` and register each file in `src/backends/sql.rs`
   (a unit test fails if a file is missing from the registry).
4. If a query reads a relation not yet in `sql/queries/startup/tables.sql`, add it there and bump
   `STARTUP_RELATIONS` in `src/backends/chado.rs`.
5. Add `tests/tools_<tool>.rs` using `mod support;` and the shared server.
6. Regenerate `docs/tools.md` (below).

## Adding a report section

1. Write `sql/queries/report/<type>/<section>.sql` and register it in `src/backends/sql.rs`.
2. Add one `match` arm for the section name in that type's `section` method in `src/report/<type>.rs`.
   List sections query `ctx.fetch` rows (one more than the limit) and return `paged(rows, ctx.limit)`.
3. Add the name to the type's `DEFAULT` list if it is cheap and broadly useful, else to `OPTIONAL`.
   `list_options` and section validation read these lists.
4. Cover it in `tests/tools_report_*.rs`.

A new ID prefix needs a new `ReportType` module and an arm in `report::for_id`.

## Regenerating the tool catalog

    cargo run --bin tool-catalog > ../docs/tools.md

CI runs the same command and fails if the committed `docs/tools.md` differs, so regenerate it whenever a
tool's name, description, or parameters change.

## Docker

    docker build -t flybase-mcp .

See [`docs/deployment.md`](../docs/deployment.md) for the environment, migrations, and nginx.
