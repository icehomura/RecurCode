# Stage 1: Build Next.js static export
FROM node:24-alpine AS frontend
RUN corepack enable
WORKDIR /app
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY src/ ./src/
COPY public/ ./public/
COPY next.config.ts tsconfig.json postcss.config.mjs components.json ./
RUN pnpm build

# Stage 2: Build Rust server binary + recur-code-mcp companion
FROM rust:slim-bookworm AS backend
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app/src-tauri
COPY src-tauri/ ./
# recur-code-mcp is the stdio MCP companion the runtime injects per session
# (see acp/delegation/companion.rs). It must ship next to recur-code-server so
# `locate_recur_code_mcp_binary()` finds it via the exe-sibling lookup.
RUN cargo build --release --bin recur-code-server --no-default-features --features server-bin \
 && cargo build --release --bin recur-code-mcp --no-default-features --features mcp-bin

# Stage 3: Runtime
FROM node:24-bookworm-slim
RUN apt-get update && apt-get install -y \
    libsqlite3-0 \
    git \
    openssh-client \
    ca-certificates \
    curl \
    python3 \
    python3-pip \
    libicu72 \
    && rm -rf /var/lib/apt/lists/*
# libicu72: OfficeCLI ships as a self-contained binary with an embedded .NET
# runtime, which requires the system ICU library at startup. node:*-bookworm-slim
# bundles Node's own ICU statically and so does NOT install system libicu — without
# this, every `officecli` invocation aborts with "Couldn't find a valid ICU package
# installed on the system", breaking both skill sync and office file preview in the
# server/Docker mode. The version (72) is pinned to Debian bookworm; bump it to match
# if the base image moves to a newer Debian release (e.g. trixie ships libicu76).

COPY --from=backend /app/src-tauri/target/release/recur-code-server /usr/local/bin/recur-code-server
COPY --from=backend /app/src-tauri/target/release/recur-code-mcp /usr/local/bin/recur-code-mcp
COPY --from=frontend /app/out /app/web

ENV RC_STATIC_DIR=/app/web
ENV RC_DATA_DIR=/data
ENV RC_PORT=3080
ENV RC_HOST=0.0.0.0
ENV SHELL=/bin/bash
# In-place self-update markers: tells the running server it is a container
# (for the post-upgrade "also pull the image" hint) and how long the
# supervisor waits before relaunching the worker after an upgrade.
ENV RC_RUNTIME=docker
ENV RC_RESTART_DELAY_MS=2000

EXPOSE 3080
# Port bridge for dev servers (RC_BRIDGE_PORTS; default RC_PORT+1..+10).
# Not needed when RC_BRIDGE_HOST_PATTERN names the dev servers by hostname
# instead: those answer on 3080, and no range has to be guessed in advance.
EXPOSE 3081-3090
VOLUME /data

# Run under the built-in supervisor (PID 1) so an in-place upgrade can swap
# the binary and have the worker relaunched without stopping the container.
CMD ["recur-code-server", "--supervise"]
