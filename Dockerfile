# ── Stage 1: Build WASM with Trunk ────────────────────────────
FROM rust:1-bookworm@sha256:365468470075493dc4583f47387001854321c5a8583ea9604b297e67f01c5a4f AS builder

ARG TRUNK_VERSION=0.21.14

RUN rustup target add wasm32-unknown-unknown \
    && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xzf - -C /usr/local/bin

WORKDIR /app
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
RUN cargo build --target wasm32-unknown-unknown --release 2>/dev/null || true

COPY . .

ARG PUBLIC_URL=/
RUN trunk build --release --public-url "${PUBLIC_URL}"

# ── Stage 2: Download static-web-server ───────────────────────
FROM docker.io/joseluisq/static-web-server:2@sha256:2d67e47e22172235e339908777e692006ffdcf42dc4c531aff5d4337a7559a1e AS sws

# ── Stage 3: Minimal runtime — distroless, no shell, no CVE ──
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:7e5b8df2f4d36f5599ef4ab856d7d444922531709becb03f3368c6d797d0a5eb

COPY --from=sws /static-web-server /static-web-server
COPY --from=builder /app/dist /public

ENV SERVER_PORT=8080
ENV SERVER_ROOT=/public
ENV SERVER_LOG_LEVEL=info
ENV SERVER_FALLBACK_PAGE=/public/index.html

EXPOSE 8080

ENTRYPOINT ["/static-web-server"]
