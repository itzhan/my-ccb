import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// 开发期把 /admin、/v1 代理到后端。默认指向线上网关，便于用真实数据看效果；
// 本地起了 cargo run 的话设 VITE_PROXY_TARGET=http://localhost:5674 即可。
const TARGET = process.env.VITE_PROXY_TARGET || 'http://67.21.86.146:5674';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    port: 3000,
    proxy: {
      '/admin': { target: TARGET, changeOrigin: true },
      '/v1': { target: TARGET, changeOrigin: true },
      '/_health': { target: TARGET, changeOrigin: true },
    },
  },
});
