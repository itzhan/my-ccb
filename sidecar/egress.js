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
// 代理：
//   - http/https 代理用 undici ProxyAgent
//   - socks4/socks5 代理用 socks 包建立 TCP 隧道，再由 undici 在隧道上做 TLS
//     (Bun fetch 不支持 socks)。两种方式都保留 header 顺序，且 TLS 指纹不变。
//
// 协议：cc-bridge 用相同的 method/path/query/body 请求本服务，并附带控制头：
//   x-ccb-upstream : 真正的上游基址，如 https://api.anthropic.com
//   x-ccb-proxy    : 账号代理(可空)
//   x-ccb-headers  : base64(JSON [[name,value],...])，要发往上游的【有序】请求头

const { request, ProxyAgent, Agent, buildConnector } = require('undici');
const { SocksClient } = require('socks');
const { Readable } = require('node:stream');

const PORT = Number(process.env.BUN_SIDECAR_PORT || 8788);

// 无代理时复用的默认 dispatcher(keep-alive)
const defaultAgent = new Agent();
// undici 默认 TLS 连接器(socks 隧道建立后用它在已有 socket 上做 TLS)
const tlsConnect = buildConnector({});

// 按代理 URL 缓存 dispatcher，复用连接池
const agentCache = new Map();

function dispatcherForProxy(proxy) {
  if (!proxy) return defaultAgent;
  let agent = agentCache.get(proxy);
  if (agent) return agent;
  agent = /^socks/i.test(proxy) ? makeSocksAgent(proxy) : new ProxyAgent(proxy);
  agentCache.set(proxy, agent);
  return agent;
}

// socks4/socks5 代理：用 socks 建 TCP 隧道，undici 在隧道 socket 上做 TLS。
function makeSocksAgent(proxyUrl) {
  const u = new URL(proxyUrl);
  const type = /socks4/i.test(u.protocol) ? 4 : 5;
  const proxy = { host: u.hostname, port: Number(u.port) || 1080, type };
  if (u.username) proxy.userId = decodeURIComponent(u.username);
  if (u.password) proxy.password = decodeURIComponent(u.password);
  return new Agent({
    connect(opts, callback) {
      SocksClient.createConnection({
        proxy,
        command: 'connect',
        destination: { host: opts.hostname, port: Number(opts.port) || 443 },
      })
        .then(({ socket }) => {
          // 把已建立的隧道 socket 交给 undici 做 TLS(指纹仍是 Bun BoringSSL)
          tlsConnect({ ...opts, httpSocket: socket }, callback);
        })
        .catch((err) => callback(err, null));
    },
  });
}

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
    const b64 = req.headers.get('x-ccb-headers');
    if (!b64) {
      return new Response('missing x-ccb-headers', { status: 400 });
    }
    let orderedHeaders;
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

    let resp;
    try {
      resp = await request(target, {
        method: req.method,
        headers: orderedHeaders, // undici 原样保留顺序+大小写
        body,
        dispatcher: dispatcherForProxy(proxy),
        maxRedirections: 0,
      });
    } catch (e) {
      return new Response('egress error: ' + (e && e.message ? e.message : e), { status: 502 });
    }

    // 回传上游响应头。注意：Bun 的 undici 会【自动解压】gzip/br/deflate/zstd 的 body
    // (与真实 Claude Code 用的同一套 Bun undici 行为一致)，所以这里 body 已是明文，
    // 必须去掉 content-encoding / content-length(否则客户端会按 gzip 解压明文 → ZlibError)。
    // 同时去掉与流式回传冲突的 transfer-encoding / connection。
    const out = new Headers();
    for (const [k, v] of Object.entries(resp.headers)) {
      const lk = k.toLowerCase();
      if (
        lk === 'content-encoding' ||
        lk === 'content-length' ||
        lk === 'transfer-encoding' ||
        lk === 'connection'
      )
        continue;
      if (Array.isArray(v)) for (const vv of v) out.append(k, vv);
      else if (v != null) out.set(k, v);
    }

    const webStream = Readable.toWeb(resp.body);
    return new Response(webStream, { status: resp.statusCode, headers: out });
  },
});

console.log('[egress] bun sidecar (undici+socks) listening on 127.0.0.1:' + PORT + ' (bun ' + Bun.version + ')');
