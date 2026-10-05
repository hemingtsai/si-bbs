# ---- Frontend build ----
FROM node:26-alpine AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ---- Backend build (musl, fully static) ----
FROM rust:1.98-alpine AS backend
RUN apk add --no-cache musl-dev gcc
# In slow-registry environments, build with e.g.
#   docker build --build-arg CARGO_REGISTRY_MIRROR=https://mirrors.ustc.edu.cn/crates.io-index/ .
ARG CARGO_REGISTRY_MIRROR=""
RUN if [ -n "$CARGO_REGISTRY_MIRROR" ]; then \
      printf '[source.crates-io]\nreplace-with = "mirror"\n[source.mirror]\nregistry = "sparse+%s"\n' \
        "$CARGO_REGISTRY_MIRROR" > /usr/local/cargo/config.toml; \
    fi
WORKDIR /app/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
COPY backend/migrations ./migrations
RUN cargo build --release

# ---- Runtime ----
FROM alpine:3.21
RUN adduser -D -u 10001 sibbs
WORKDIR /app
COPY --from=backend /app/backend/target/release/si-bbs-backend /app/si-bbs-backend
COPY backend/migrations ./migrations
COPY --from=frontend /app/frontend/dist ./static
ENV STATIC_DIR=/app/static \
    DATABASE_URL=sqlite:///data/si-bbs.db?mode=rwc \
    RUST_LOG=si_bbs_backend=info,tower_http=info
RUN mkdir -p /data && chown -R sibbs:sibbs /data /app
USER sibbs
EXPOSE 3000
VOLUME ["/data"]
CMD ["/app/si-bbs-backend"]
