FROM rust:1.90-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY sql ./sql
COPY data ./data
RUN cargo build --release --locked --bin flybase-mcp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 flybase
COPY --from=build /src/target/release/flybase-mcp /usr/local/bin/flybase-mcp
USER flybase
EXPOSE 7200
ENV FLYBASE_MCP_BIND=0.0.0.0:7200 RUST_LOG=info
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://127.0.0.1:7200/health | grep -q '"status":"ok"' || exit 1
ENTRYPOINT ["flybase-mcp"]
CMD ["serve"]
