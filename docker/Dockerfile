# Stage 1: Build frontend
FROM --platform=$BUILDPLATFORM node:22-alpine AS frontend
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# Stage 2: Build Rust backend
FROM --platform=$BUILDPLATFORM rust:slim-bookworm AS backend
ARG TARGETPLATFORM
WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev gcc-aarch64-linux-gnu && rm -rf /var/lib/apt/lists/*

# 根据目标平台设置交叉编译
RUN case "$TARGETPLATFORM" in \
      "linux/arm64") \
        rustup target add aarch64-unknown-linux-gnu \
        ;; \
    esac

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY craftls/ ./craftls/
COPY assets/ ./assets/
COPY --from=frontend /app/web/dist ./web/dist/

RUN case "$TARGETPLATFORM" in \
      "linux/arm64") \
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc && \
        cargo build --release --target aarch64-unknown-linux-gnu && \
        cp target/aarch64-unknown-linux-gnu/release/claude-code-gateway /app/claude-code-gateway-bin \
        ;; \
      *) \
        cargo build --release && \
        cp target/release/claude-code-gateway /app/claude-code-gateway-bin \
        ;; \
    esac

# Stage 3: Runtime
FROM debian:bookworm-slim
# Bun 版本：与 Claude Code 当前打包的 Bun 对齐（CC 升级 bun 时改这一行即可）
ARG BUN_VERSION=1.3.14
RUN apt-get update && apt-get install -y ca-certificates curl unzip libstdc++6 && \
    curl -fsSL "https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/bun-linux-x64.zip" -o /tmp/bun.zip && \
    unzip -q /tmp/bun.zip -d /tmp/bun && \
    mv /tmp/bun/bun-linux-x64/bun /usr/local/bin/bun && chmod +x /usr/local/bin/bun && \
    rm -rf /tmp/bun.zip /tmp/bun && \
    apt-get purge -y curl unzip && apt-get autoremove -y && rm -rf /var/lib/apt/lists/* && \
    bun --version
WORKDIR /app

COPY --from=backend /app/claude-code-gateway-bin ./claude-code-gateway
COPY --from=frontend /app/web/dist ./web/dist/
COPY sidecar/ ./sidecar/
# 边车用 undici 出口（保留真 CC 的 header 顺序+大小写），在镜像内安装依赖
RUN cd /app/sidecar && bun install --production && bun --version
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# 默认走 Bun 边车出口（真实 BoringSSL 指纹）
ENV EGRESS_MODE=bun_sidecar
ENV BUN_SIDECAR_PORT=8788

RUN mkdir -p data
EXPOSE 5674
CMD ["/usr/local/bin/entrypoint.sh"]
