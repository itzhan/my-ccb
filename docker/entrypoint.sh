#!/bin/sh
# 同时拉起 Bun 出口边车 + cc-bridge 主进程。
# 边车崩溃自动重启；主进程退出则容器退出（由 docker restart 策略接管）。
set -e

PORT="${BUN_SIDECAR_PORT:-8788}"

(
  while true; do
    echo "[entrypoint] starting bun egress sidecar on :${PORT}"
    BUN_SIDECAR_PORT="${PORT}" bun run /app/sidecar/egress.js || true
    echo "[entrypoint] sidecar exited, restarting in 1s"
    sleep 1
  done
) &

exec /app/claude-code-gateway
