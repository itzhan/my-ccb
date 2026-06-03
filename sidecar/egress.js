// Bun 出口边车 (egress sidecar)
//
// cc-bridge 把要发往上游的请求转发到这里，本边车用 undici(运行在 Bun 上，底层是
// Bun 的 BoringSSL)重新发出 —— 于是发往 api.anthropic.com 的 TLS 指纹永远是
// "真实 Claude Code(Bun)"的指纹(JA3/JA4 完全一致)，随 Bun 版本自动跟随。
//
// 为什么用 undici 而非 Bun 原生 fetch：
//   Bun 的 fetch / node:http 都会对请求头【按字母序重排】，导致 header 顺序与真实
//   Claude Code 不一致(真 CC 用 Anthropic SDK 内置的 undici，保留 SDK 插入顺序)。
//   undici 会【原样保留】传入的 header 顺序与大小写，传输头(Host/Connection/
//   Accept-Encoding/Content-Length)自动追加到末尾 —— 与真 CC 抓包一字不差。
//
// 协议：cc-bridge 用相同的 method/path/query/body 请求本服务，并附带控制头：
//   x-ccb-upstream : 真正的上游基址，如 https://api.anthropic.com
//   x-ccb-proxy    : 账号代理(可空)
//   x-ccb-headers  : base64(JSON [[name,value],...])，要发往上游的【有序】请求头
// 本服务用 undici 把这些头按原序发往 upstream，并把响应(含 SSE 流)原样回传。

const { request, ProxyAgent, Agent } = require('undici');
const { Readable } = require('node:stream');

const PORT = Number(process.env.BUN_SIDECAR_PORT || 8788);

// 复用一个默认 dispatcher(keep-alive)，无代理时用它
const defaultAgent = new Agent();

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

    // 解析有序请求头(cc-bridge 已恢复真实大小写并按真 CC 顺序排好)
    let orderedHeaders;
    const b64 = req.headers.get('x-ccb-headers');
    if (!b64) {
      return new Response('missing x-ccb-headers', { status: 400 });
    }
    try {
      const pairs = JSON.parse(Buffer.from(b64, 'base64').toString('utf8'));
      // 用对象保留插入顺序(undici 原样发出)；CC 请求头无重复名
      orderedHeaders = {};
      for (const [k, v] of pairs) orderedHeaders[k] = v;
    } catch (e) {
      return new Response('bad x-ccb-headers: ' + e.message, { status: 400 });
    }

    const hasBody = req.method !== 'GET' && req.method !== 'HEAD';
    const body = hasBody ? Buffer.from(await req.arrayBuffer()) : undefined;

    // socks 代理 undici 不支持，退回 Bun fetch(会丢 header 顺序，仅 socks 账号受影响)
    if (proxy && /^socks/i.test(proxy)) {
      return await viaBunFetch(target, req.method, orderedHeaders, body, proxy);
    }

    const dispatcher = proxy ? new ProxyAgent(proxy) : defaultAgent;

    let resp;
    try {
      resp = await request(target, {
        method: req.method,
        headers: orderedHeaders, // undici 原样保留顺序+大小写
        body,
        dispatcher,
        maxRedirections: 0,
      });
    } catch (e) {
      return new Response('egress error: ' + (e && e.message ? e.message : e), { status: 502 });
    }

    // 透传上游响应；undici 不自动解压，content-encoding 与 body 一致，原样回传。
    // 仅去掉会与流式回传冲突的 transfer-encoding / connection。
    const out = new Headers();
    for (const [k, v] of Object.entries(resp.headers)) {
      const lk = k.toLowerCase();
      if (lk === 'transfer-encoding' || lk === 'connection') continue;
      if (Array.isArray(v)) for (const vv of v) out.append(k, vv);
      else if (v != null) out.set(k, v);
    }

    const webStream = Readable.toWeb(resp.body);
    return new Response(webStream, { status: resp.statusCode, headers: out });
  },
});

// socks 代理回退路径：Bun 原生 fetch(支持 socks，但会重排 header 顺序)
async function viaBunFetch(target, method, orderedHeaders, body, proxy) {
  const init = { method, headers: orderedHeaders, redirect: 'manual', proxy };
  if (body) init.body = body;
  let resp;
  try {
    resp = await fetch(target, init);
  } catch (e) {
    return new Response('egress error(socks): ' + (e && e.message ? e.message : e), { status: 502 });
  }
  const out = new Headers(resp.headers);
  out.delete('content-encoding');
  out.delete('content-length');
  out.delete('transfer-encoding');
  return new Response(resp.body, { status: resp.status, headers: out });
}

console.log('[egress] bun sidecar (undici) listening on 127.0.0.1:' + PORT + ' (bun ' + Bun.version + ')');
