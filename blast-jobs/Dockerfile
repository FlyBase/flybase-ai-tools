# blast-jobs: the service binary plus NCBI BLAST+ 2.17.0 from NCBI's official tarball (design 3.9, ruling 6).
# rust-toolchain.toml is excluded by .dockerignore: the image pins its own toolchain here, and rustup would
# otherwise try to install the file's channel at build time.
FROM rust:1.90-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked --bin blast-jobs

# Versioned path (never LATEST), pinned by SHA-256. NCBI publishes only an MD5 next to the tarball
# (bdec166721de3b55f90a3badc83538e8); the SHA-256 below was computed from a download that matched it.
FROM debian:trixie-slim AS blast
ARG BLAST_URL=https://ftp.ncbi.nlm.nih.gov/blast/executables/blast+/2.17.0/ncbi-blast-2.17.0+-x64-linux.tar.gz
ARG BLAST_SHA256=3888112d8207831aa47371d93583c601f058f88b5db22dc782438b039a3a411b
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL --retry 3 -o /tmp/blast.tar.gz "$BLAST_URL" \
 && echo "$BLAST_SHA256  /tmp/blast.tar.gz" | sha256sum -c - \
 && mkdir -p /opt/ncbi-blast/bin \
 && tar -xzf /tmp/blast.tar.gz -C /tmp \
 && cd /tmp/ncbi-blast-2.17.0+/bin \
 && cp blastn blastp blastx tblastn tblastx blastdbcmd makeblastdb /opt/ncbi-blast/bin/ \
 && cp /tmp/ncbi-blast-2.17.0+/LICENSE /opt/ncbi-blast/LICENSE \
 && rm -rf /tmp/blast.tar.gz /tmp/ncbi-blast-2.17.0+

FROM debian:trixie-slim
# curl for the HEALTHCHECK; libgomp1 is the one shared library BLAST+ needs beyond the base image.
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends ca-certificates curl libgomp1 && rm -rf /var/lib/apt/lists/*
COPY --from=blast /opt/ncbi-blast /opt/ncbi-blast
ENV PATH=/opt/ncbi-blast/bin:$PATH NCBI_DONT_USE_NCBIRC=1 NCBI_DONT_USE_LOCAL_CONFIG=1 BLAST_USAGE_REPORT=false
# A missing shared library or a wrong build fails here, not at the first job.
RUN for b in blastn blastp blastx tblastn tblastx blastdbcmd makeblastdb; do \
      "$b" -version | grep -q '2\.17\.0+' || { echo "$b is not BLAST+ 2.17.0" >&2; exit 1; }; \
    done
RUN useradd --system --uid 10002 --no-create-home blastjobs
COPY --from=build /src/target/release/blast-jobs /usr/local/bin/blast-jobs
USER blastjobs
EXPOSE 8800
ENV RUST_LOG=info
# Liveness only (design 3.8): a full queue answers 503 on the plain health route but must not restart
# the container. The port is the config's `bind`; the sample uses 8800.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
  CMD curl -fsS 'http://127.0.0.1:8800/internal/health?liveness=1' > /dev/null || exit 1
ENTRYPOINT ["blast-jobs"]
CMD ["serve", "/etc/blast-jobs/config.json"]
