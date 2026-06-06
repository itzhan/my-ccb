//! 用量记录写入管道：热路径只 try_send（满即丢，绝不阻塞代理），
//! 后台 writer 批量(100 条 / 500ms)落库并累加每日汇总。
//! 同时包含从响应(SSE / 非流式)里抽取 4 类 token 的解析器。

use std::time::Duration;

use serde_json::Value;
use sqlx::AnyPool;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::model::usage::UsageRecord;
use crate::store::usage_store;

const CHANNEL_CAP: usize = 4096;
const BATCH_SIZE: usize = 100;
const FLUSH_MS: u64 = 500;
const PARSER_BUF_CAP: usize = 512 * 1024; // 非流式 body / 单行缓冲上限

/// 异步用量写入器：克隆廉价（内部一个 mpsc Sender）。
#[derive(Clone)]
pub struct UsageRecorder {
    tx: Option<mpsc::Sender<UsageRecord>>,
}

impl UsageRecorder {
    /// 禁用态（不记录）。
    pub fn disabled() -> Self {
        Self { tx: None }
    }

    /// 启动写入管道 + 每日清理任务，返回可克隆的记录器。
    pub fn start(pool: AnyPool, retain_days: i64) -> Self {
        let (tx, rx) = mpsc::channel::<UsageRecord>(CHANNEL_CAP);
        tokio::spawn(writer_loop(rx, pool.clone()));
        tokio::spawn(prune_loop(pool, retain_days));
        Self { tx: Some(tx) }
    }

    /// 投递一条记录（满即丢，绝不阻塞）。
    pub fn record(&self, rec: UsageRecord) {
        if let Some(tx) = &self.tx {
            if tx.try_send(rec).is_err() {
                debug!("usage channel full or closed, dropping record");
            }
        }
    }
}

async fn writer_loop(mut rx: mpsc::Receiver<UsageRecord>, pool: AnyPool) {
    let mut buf: Vec<UsageRecord> = Vec::with_capacity(BATCH_SIZE);
    let mut ticker = tokio::time::interval(Duration::from_millis(FLUSH_MS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            maybe = rx.recv() => {
                match maybe {
                    Some(r) => {
                        buf.push(r);
                        if buf.len() >= BATCH_SIZE {
                            flush(&pool, &mut buf).await;
                        }
                    }
                    None => {
                        flush(&pool, &mut buf).await;
                        break;
                    }
                }
            }
            _ = ticker.tick() => {
                if !buf.is_empty() {
                    flush(&pool, &mut buf).await;
                }
            }
        }
    }
}

async fn flush(pool: &AnyPool, buf: &mut Vec<UsageRecord>) {
    if buf.is_empty() {
        return;
    }
    // 记下涉及的账号 id,刷盘后失效 5h cost 缓存,确保下次选号读到新值
    let touched: std::collections::HashSet<i64> =
        buf.iter().map(|r| r.account_id).filter(|id| *id > 0).collect();
    if let Err(e) = usage_store::batch_insert_and_rollup(pool, buf).await {
        warn!("usage batch insert failed ({} rows): {}", buf.len(), e);
    }
    for id in touched {
        crate::service::account::invalidate_cost_cache(id);
    }
    buf.clear();
}

async fn prune_loop(pool: AnyPool, retain_days: i64) {
    if retain_days <= 0 {
        return; // 0/负 = 永久保留明细
    }
    let mut ticker = tokio::time::interval(Duration::from_secs(24 * 3600));
    loop {
        ticker.tick().await;
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(retain_days))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        if let Err(e) = usage_store::prune_logs_before(&pool, &cutoff).await {
            warn!("usage prune failed: {}", e);
        }
    }
}

// ---------------- 响应 token 解析 ----------------

/// 从响应字节流中增量抽取 4 类 token。
/// 流式：逐行扫 `data:` 事件（message_start 取 input/cache，message_delta 取 output）。
/// 非流式：累积 body 后解析顶层 `usage`。
pub struct UsageParser {
    is_stream: bool,
    line_buf: Vec<u8>,
    body_buf: Vec<u8>,
    pub input: i64,
    pub output: i64,
    pub cache_create: i64,
    pub cache_read: i64,
    pub cc_5m: i64,
    pub cc_1h: i64,
    pub model: String,
    pub request_id: String,
}

impl UsageParser {
    pub fn new(is_stream: bool) -> Self {
        Self {
            is_stream,
            line_buf: Vec::new(),
            body_buf: Vec::new(),
            input: 0,
            output: 0,
            cache_create: 0,
            cache_read: 0,
            cc_5m: 0,
            cc_1h: 0,
            model: String::new(),
            request_id: String::new(),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) {
        if self.is_stream {
            if self.line_buf.len() + chunk.len() > PARSER_BUF_CAP {
                self.line_buf.clear(); // 异常超长行，丢弃保护内存
            }
            self.line_buf.extend_from_slice(chunk);
            while let Some(pos) = self.line_buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.line_buf.drain(..=pos).collect();
                self.process_line(&line[..line.len() - 1]);
            }
        } else if self.body_buf.len() < PARSER_BUF_CAP {
            self.body_buf.extend_from_slice(chunk);
        }
    }

    fn process_line(&mut self, line: &[u8]) {
        let s = match std::str::from_utf8(line) {
            Ok(s) => s.trim(),
            Err(_) => return,
        };
        let payload = match s.strip_prefix("data:") {
            Some(p) => p.trim(),
            None => return,
        };
        if !payload.contains("\"usage\"") {
            return;
        }
        if let Ok(v) = serde_json::from_str::<Value>(payload) {
            self.absorb_event(&v);
        }
    }

    /// 流结束：非流式在此解析整段 body。
    pub fn finish(&mut self) {
        if !self.is_stream && !self.body_buf.is_empty() {
            if let Ok(v) = serde_json::from_slice::<Value>(&self.body_buf) {
                self.absorb_event(&v);
            }
        }
    }

    fn absorb_event(&mut self, v: &Value) {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("message_start") => {
                if let Some(msg) = v.get("message") {
                    self.set_meta(msg);
                    if let Some(u) = msg.get("usage") {
                        self.absorb_usage(u, true);
                    }
                }
            }
            Some("message_delta") => {
                if let Some(u) = v.get("usage") {
                    self.absorb_usage(u, false);
                }
            }
            _ => {
                // 非流式完整响应
                self.set_meta(v);
                if let Some(u) = v.get("usage") {
                    self.absorb_usage(u, true);
                }
            }
        }
    }

    fn set_meta(&mut self, node: &Value) {
        if self.model.is_empty() {
            if let Some(m) = node.get("model").and_then(|m| m.as_str()) {
                self.model = m.to_string();
            }
        }
        if self.request_id.is_empty() {
            if let Some(id) = node.get("id").and_then(|i| i.as_str()) {
                self.request_id = id.to_string();
            }
        }
    }

    /// with_input=true 时(message_start / 非流式)记录 input/cache；否则(message_delta)只更新 output。
    fn absorb_usage(&mut self, u: &Value, with_input: bool) {
        let get = |k: &str| u.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        let out = get("output_tokens");
        if out > 0 {
            self.output = out; // 累计值，后到覆盖
        }
        if with_input {
            self.input = get("input_tokens");
            self.cache_create = get("cache_creation_input_tokens");
            self.cache_read = get("cache_read_input_tokens");
            if let Some(cc) = u.get("cache_creation") {
                self.cc_5m = cc
                    .get("ephemeral_5m_input_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                self.cc_1h = cc
                    .get("ephemeral_1h_input_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
            }
        } else {
            // delta 里偶尔也带 input/cache，>0 才更新
            let i = get("input_tokens");
            if i > 0 {
                self.input = i;
            }
            let cc = get("cache_creation_input_tokens");
            if cc > 0 {
                self.cache_create = cc;
            }
            let cr = get("cache_read_input_tokens");
            if cr > 0 {
                self.cache_read = cr;
            }
        }
    }
}

// ---------------- 旁路嗅探流 ----------------

use std::pin::Pin;
use std::task::{Context, Poll};
use bytes::Bytes;
use futures_core::Stream;

/// 一次转发的日志元数据。
pub struct RecordMeta {
    pub token_id: i64,
    pub account_id: i64,
    pub req_model: String,
    pub stream: bool,
    pub status_code: i32,
    pub started: std::time::Instant,
    // 详细诊断
    pub client_ip: String,
    pub user_agent: String,
    pub path: String,
    pub session_id: String,
    pub user_id: String,
    pub proxy: String,
    pub req_headers: String,
    pub resp_headers: String,
}

/// 包裹上游响应体：转发字节原样不变，流被读完(None)或丢弃(Drop)时落一条用量记录。
/// 用 Drop 兜底是因为：响应带 Content-Length 时，hyper 读够字节后不会再 poll 到末尾 None，
/// 只能靠 body 被丢弃时触发记录。
pub struct SniffStream<S> {
    inner: S,
    parser: Option<UsageParser>,
    err_buf: Vec<u8>,
    is_error: bool,
    recorder: UsageRecorder,
    meta: RecordMeta,
}

const ERR_CAP: usize = 8192;

impl<S> SniffStream<S> {
    pub fn new(inner: S, recorder: UsageRecorder, meta: RecordMeta) -> Self {
        let is_stream = meta.stream;
        let is_error = meta.status_code >= 400;
        Self {
            inner,
            parser: Some(UsageParser::new(is_stream)),
            err_buf: Vec::new(),
            is_error,
            recorder,
            meta,
        }
    }

    fn emit(&mut self) {
        let mut p = match self.parser.take() {
            Some(p) => p,
            None => return, // 已记录过
        };
        p.finish();
        let error = if self.is_error && !self.err_buf.is_empty() {
            String::from_utf8_lossy(&self.err_buf)
                .chars()
                .take(2000)
                .collect::<String>()
        } else {
            String::new()
        };
        let rec = UsageRecord {
            token_id: self.meta.token_id,
            account_id: self.meta.account_id,
            request_id: p.request_id.clone(),
            model: if p.model.is_empty() {
                self.meta.req_model.clone()
            } else {
                p.model.clone()
            },
            input_tokens: p.input,
            output_tokens: p.output,
            cache_creation_tokens: p.cache_create,
            cache_read_tokens: p.cache_read,
            cache_creation_5m_tokens: p.cc_5m,
            cache_creation_1h_tokens: p.cc_1h,
            stream: self.meta.stream,
            status_code: self.meta.status_code,
            duration_ms: self.meta.started.elapsed().as_millis() as i64,
            error,
            client_ip: self.meta.client_ip.clone(),
            user_agent: self.meta.user_agent.clone(),
            path: self.meta.path.clone(),
            session_id: self.meta.session_id.clone(),
            user_id: self.meta.user_id.clone(),
            proxy: self.meta.proxy.clone(),
            req_headers: self.meta.req_headers.clone(),
            resp_headers: self.meta.resp_headers.clone(),
        };
        // 成功(有用量)或失败(有错误正文)才记；中途被丢弃且未读 body 的重试不记
        if rec.has_usage() || !rec.error.is_empty() {
            self.recorder.record(rec);
        }
    }
}

impl<S, E> Stream for SniffStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(b))) => {
                if let Some(p) = this.parser.as_mut() {
                    p.feed(&b);
                }
                if this.is_error && this.err_buf.len() < ERR_CAP {
                    let take = (ERR_CAP - this.err_buf.len()).min(b.len());
                    this.err_buf.extend_from_slice(&b[..take]);
                }
                Poll::Ready(Some(Ok(b)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                this.emit();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Drop for SniffStream<S> {
    fn drop(&mut self) {
        self.emit();
    }
}
