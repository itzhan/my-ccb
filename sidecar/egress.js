// Bun 出口边车 (egress sidecar)
//
// cc-bridge 把要发往上游的请求转发到这里，本边车用 Bun 的原生 fetch(BoringSSL)
// 重新发出 —— 于是发往 api.anthropic.com 的 TLS 指纹永远是"真实 Claude Code(Bun)"的指纹，
// 随 Bun 版本自动跟随，无需手工维护 craftls 的 ClientHello。
//
// 协议：cc-bridge 用相同的 method/path/query/headers/body 请求本服务，并附带两个控制头：
//   x-ccb-upstream : 真正的上游基址，如 https://api.anthropic.com
//   x-ccb-proxy    : 账号代理（可空）
// 本服务剥掉控制头，用 Bun fetch 透传到 upstream，并把响应(含 SSE 流)原样回传。

const PORT = Number(process.env.BUN_SIDECAR_PORT || 8788);

const HOP_BY_HOP = ['host', 'content-length', 'connection', 'x-ccb-upstream', 'x-ccb-proxy'];

Bun.serve({
  port: PORT,
  hostname: '127.0.0.1',
  idleTimeout: 0, // 不限制空闲超时，支持长 SSE 流
  maxRequestBodySize: 64 * 1024 * 1024,
  async fetch(req) {
    const url = new URL(req.url);
    const upstream = req.headers.get('x-ccb-upstream');
    if (!upstream) {
      return new Response('missing x-ccb-upstream', { status: 400 });
    }
    const proxy = req.headers.get('x-ccb-proxy') || '';

    const target = upstream.replace(/\/+$/, '') + url.pathname + url.search;

    // 复制客户端要发的头，剥掉控制头与 hop-by-hop
    const headers = new Headers(req.headers);
    for (const h of HOP_BY_HOP) headers.delete(h);

    const init = {
      method: req.method,
      headers,
      redirect: 'manual',
    };
    if (proxy) init.proxy = proxy;
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      init.body = await req.arrayBuffer();
    }

    let resp;
    try {
      resp = await fetch(target, init);
    } catch (e) {
      return new Response('egress error: ' + (e && e.message ? e.message : e), { status: 502 });
    }

    // 透传上游响应；Bun 已解压 body，去掉会与解压后 body 冲突的头，按流回传
    const out = new Headers(resp.headers);
    out.delete('content-encoding');
    out.delete('content-length');
    out.delete('transfer-encoding');

    return new Response(resp.body, { status: resp.status, headers: out });
  },
});

console.log('[egress] bun sidecar listening on 127.0.0.1:' + PORT + ' (bun ' + Bun.version + ')');
