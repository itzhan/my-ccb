use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

use crate::error::AppError;
use crate::model::account::{Account, AccountStatus};
use crate::model::api_token::ApiToken;
use crate::service::account::{AccountService, RateLimitClassification};
use crate::service::client_guard::{self, ClientRestriction};
use crate::service::rewriter::{
    clean_session_id_from_body, detect_client_type, inject_auth_before_xapp,
    order_headers_canonical, passthrough_headers_ordered, ClientType, Rewriter,
};
use base64::Engine;
use crate::service::telemetry::TelemetryService;
use crate::service::usage_recorder::{RecordMeta, SniffStream, UsageRecorder};

/// 转发日志上下文（用量记录用，含详细诊断字段）。
struct LogCtx {
    token_id: i64,
    account_id: i64,
    model: String,
    stream: bool,
    started: std::time::Instant,
    client_ip: String,
    user_agent: String,
    path: String,
    session_id: String,
    user_id: String,
    req_headers: String,
}

/// 取客户端 IP：优先 X-Forwarded-For / X-Real-IP，回退连接对端地址。
fn client_ip_from(req: &Request, headers: &std::collections::HashMap<String, String>) -> String {
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Some(first) = xff.split(',').next() {
            let t = first.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    if let Some(xr) = headers.get("x-real-ip") {
        if !xr.is_empty() {
            return xr.clone();
        }
    }
    if let Some(ci) = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
    {
        return ci.0.ip().to_string();
    }
    String::new()
}

/// 把请求头序列化成 JSON（脱敏 auth/cookie），供封号分析。
fn sanitized_headers_json(headers: &std::collections::HashMap<String, String>) -> String {
    let drop = ["authorization", "x-api-key", "cookie", "proxy-authorization"];
    let m: serde_json::Map<String, serde_json::Value> = headers
        .iter()
        .filter(|(k, _)| !drop.contains(&k.to_lowercase().as_str()))
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    serde_json::to_string(&m).unwrap_or_default()
}

/// 把上游响应头序列化成 JSON（含 anthropic-ratelimit-* / request-id / cf-ray 等封号关键信号）。
fn resp_headers_json(h: &reqwest::header::HeaderMap) -> String {
    let m: serde_json::Map<String, serde_json::Value> = h
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
            )
        })
        .collect();
    serde_json::to_string(&m).unwrap_or_default()
}

/// 脱敏代理 URL 的密码：scheme://user:pass@host -> scheme://user:***@host。
fn mask_proxy(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    if let (Some(scheme_end), Some(at)) = (p.find("://"), p.rfind('@')) {
        if at > scheme_end + 3 {
            let creds = &p[scheme_end + 3..at];
            if let Some(colon) = creds.find(':') {
                return format!(
                    "{}://{}:***@{}",
                    &p[..scheme_end],
                    &creds[..colon],
                    &p[at + 1..]
                );
            }
        }
    }
    p.to_string()
}

const UPSTREAM_BASE: &str = "https://api.anthropic.com";

/// 最大换号次数。
const MAX_ACCOUNT_SWITCHES: usize = 5;
/// 换号间隔延迟。
const SWITCH_DELAY: Duration = Duration::from_millis(500);
/// 同账号 BurstRateLimit 最大重试次数。
const MAX_SAME_ACCOUNT_RETRIES: usize = 3;
/// 同账号重试间隔。
const SAME_ACCOUNT_RETRY_DELAY: Duration = Duration::from_millis(500);
/// RPM 全账号满时的排队轮询间隔。
const RPM_WAIT_STEP: Duration = Duration::from_millis(500);
/// RPM 排队最长等待（超过则放弃，返回 503/上次响应）。
const RPM_WAIT_MAX: Duration = Duration::from_secs(20);

pub struct GatewayService {
    account_svc: Arc<AccountService>,
    rewriter: Arc<Rewriter>,
    telemetry_svc: Arc<TelemetryService>,
    /// 运行时可改的客户端限制级别（设置页可切换）。
    client_restriction: Arc<std::sync::RwLock<ClientRestriction>>,
    /// 多人共号身份归一化的全局默认（账号未单独设置时回退）。
    identity_normalize: bool,
    /// 全局默认每分钟请求数上限（账号未单独设置 rpm_limit 时回退；<=0 不限）。
    default_rpm_limit: i64,
    /// 用量记录器（异步落库，满即丢）。
    usage_recorder: UsageRecorder,
}

impl GatewayService {
    pub fn new(
        account_svc: Arc<AccountService>,
        rewriter: Arc<Rewriter>,
        telemetry_svc: Arc<TelemetryService>,
        client_restriction: Arc<std::sync::RwLock<ClientRestriction>>,
        identity_normalize: bool,
        default_rpm_limit: i64,
        usage_recorder: UsageRecorder,
    ) -> Self {
        Self {
            account_svc,
            rewriter,
            telemetry_svc,
            client_restriction,
            identity_normalize,
            default_rpm_limit,
            usage_recorder,
        }
    }

    /// 核心网关逻辑 -- axum handler。
    pub async fn handle_request(&self, req: Request, api_token: Option<&ApiToken>) -> Response {
        match self.handle_request_inner(req, api_token).await {
            Ok(resp) => resp,
            Err(e) => e.into_response(),
        }
    }

    async fn handle_request_inner(&self, req: Request, api_token: Option<&ApiToken>) -> Result<Response, AppError> {
        let req_started = std::time::Instant::now();
        let token_id = api_token.map(|t| t.id).unwrap_or(0);
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let query = req.uri().query().unwrap_or("").to_string();

        // 提取 header（HashMap 供检测/改写用；有序 Vec 保留客户端原始 wire 顺序，供透传/边车用）
        let headers = extract_headers(req.headers());
        let ordered_headers = extract_headers_ordered(req.headers());
        let ua = headers.get("User-Agent").or_else(|| headers.get("user-agent")).cloned().unwrap_or_default();
        // 详细诊断：客户端 IP / 会话 id / 请求头快照（在 req 被消费前抓取）
        let log_client_ip = client_ip_from(&req, &headers);
        let log_session_id = headers
            .get("x-claude-code-session-id")
            .or_else(|| headers.get("X-Claude-Code-Session-Id"))
            .cloned()
            .unwrap_or_default();
        let log_req_headers = sanitized_headers_json(&headers);

        // 读取请求体
        let body_bytes = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
            .await
            .map_err(|e| AppError::BadRequest(format!("failed to read body: {}", e)))?;

        // 解析请求体
        let body_map: serde_json::Value = if body_bytes.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}))
        };
        let req_model = body_map.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string();
        let is_stream = body_map.get("stream").and_then(|b| b.as_bool()).unwrap_or(false);
        let log_user_id = body_map
            .get("metadata")
            .and_then(|m| m.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 客户端限制：非真实 Claude Code 客户端直接拒绝（运行时可改）
        let restriction = self
            .client_restriction
            .read()
            .map(|g| *g)
            .unwrap_or(ClientRestriction::Off);
        if !client_guard::validate(restriction, &path, &headers, &body_map) {
            warn!("client restriction rejected request: ua=\"{}\" path={}", ua, path);
            return Err(AppError::Forbidden("client not allowed".into()));
        }

        // 检测客户端类型
        let client_type = detect_client_type(&ua, &body_map);

        // per-token 并发槽位（整请求维度，函数结束时释放）
        let _token_guard = match api_token {
            Some(t) if t.concurrency > 0 => {
                let acquired = self
                    .account_svc
                    .acquire_token_slot(t.id, t.concurrency)
                    .await
                    .map_err(|_| {
                        AppError::TooManyRequests("token concurrency slot unavailable".into())
                    })?;
                if !acquired {
                    return Err(AppError::TooManyRequests(
                        "token concurrency limit reached".into(),
                    ));
                }
                let svc = self.account_svc.clone();
                let tid = t.id;
                Some(scopeguard::guard((), move |_| {
                    let svc = svc.clone();
                    tokio::spawn(async move {
                        svc.release_token_slot(tid).await;
                    });
                }))
            }
            _ => None,
        };

        // 生成会话哈希:CC 客户端优先用 x-claude-code-session-id(权威会话标识,且与并发会话限制对齐);
        // 否则回退到内容哈希。这样一条会话稳定粘在一个号,不同会话彼此独立。
        let session_hash = if client_type == ClientType::ClaudeCode && !log_session_id.is_empty() {
            format!("ccsid:{}", log_session_id)
        } else {
            crate::service::account::generate_session_hash(&ua, &body_map, client_type)
        };

        // 根据令牌限制构建账号过滤条件
        let (allowed_ids, blocked_ids) = if let Some(t) = api_token {
            (t.allowed_account_ids(), t.blocked_account_ids())
        } else {
            (vec![], vec![])
        };

        // 并发会话限制:为该 x-claude-code-session-id 占一个有会话容量的号(满则排队等待),
        // 避免"一个号同时挂太多独立会话"被 Anthropic 判定共号。
        if client_type == ClientType::ClaudeCode
            && !self
                .account_svc
                .admit_session(&log_session_id, &session_hash, &allowed_ids, &blocked_ids)
                .await
        {
            warn!("session capacity full for session {}", log_session_id);
            return Err(AppError::TooManyRequests(
                "all accounts at session capacity, please retry".into(),
            ));
        }

        // 429 自动换号重试循环（带延迟、上限、分类决策）
        let mut exclude_ids = blocked_ids.clone();
        let mut last_resp: Option<Response> = None;
        let mut switch_count: usize = 0;
        let mut same_account_retries: std::collections::HashMap<i64, usize> =
            std::collections::HashMap::new();
        // RPM 限速：本轮因配额满被临时排除的账号 + 已累计排队时长。
        let mut rpm_rejected: Vec<i64> = Vec::new();
        let mut rpm_waited = Duration::ZERO;
        // 客户端类型分组（cli/vscode/sdk/desktop/other），供账号级放行过滤。
        let client_category = client_guard::client_type_category(&ua);
        // 因账号不放行该客户端类型而被排除的账号（永久，非排队）。
        let mut type_rejected: Vec<i64> = Vec::new();

        loop {
            let attempt = exclude_ids
                .len()
                .saturating_sub(blocked_ids.len())
                .saturating_sub(rpm_rejected.len())
                .saturating_sub(type_rejected.len());
            // 选择账号
            let account = match self
                .account_svc
                .select_account(&session_hash, &exclude_ids, &allowed_ids)
                .await
            {
                Ok(a) => a,
                Err(e) => {
                    // 全部候选账号当前分钟配额已满 → 排队等待窗口腾出再重试（削峰）。
                    if !rpm_rejected.is_empty() && rpm_waited < RPM_WAIT_MAX {
                        tokio::time::sleep(RPM_WAIT_STEP).await;
                        rpm_waited += RPM_WAIT_STEP;
                        exclude_ids.retain(|id| !rpm_rejected.contains(id));
                        rpm_rejected.clear();
                        continue;
                    }
                    if let Some(r) = last_resp {
                        return Ok(r);
                    }
                    // 所有可用账号都不放行该客户端类型 → 403（而非 503）
                    if !type_rejected.is_empty() {
                        return Err(AppError::Forbidden(format!(
                            "client type '{}' not allowed by any available account",
                            client_category
                        )));
                    }
                    return Err(AppError::ServiceUnavailable(format!(
                        "no available account: {}",
                        e
                    )));
                }
            };

            if attempt > 0 {
                warn!(
                    "429 retry attempt {} with account {} (switch {})",
                    attempt, account.id, switch_count
                );
            }

            // 账号级客户端类型放行：该账号不收此类型则换号（其它类型仍可用别的号）
            if !account.allows_client_type(client_category) {
                warn!(
                    "account {} does not allow client type '{}', switching",
                    account.id, client_category
                );
                exclude_ids.push(account.id);
                type_rejected.push(account.id);
                continue;
            }

            // 自动遥测：拦截遥测请求 + 激活会话
            if account.auto_telemetry {
                use crate::service::telemetry::{is_telemetry_path, fake_metrics_enabled_response, fake_telemetry_response};

                if is_telemetry_path(&path) {
                    let body = if path.contains("/metrics_enabled") {
                        fake_metrics_enabled_response()
                    } else {
                        fake_telemetry_response()
                    };
                    debug!("telemetry: intercepted {} for account {}", path, account.id);
                    return Ok(axum::Json(body).into_response());
                }

                if path.starts_with("/v1/messages") {
                    self.telemetry_svc.activate_session(&account).await;
                }
            }

            // RPM 限速：admission 时原子预占当前分钟配额（避免突发瞬时齐发越线）。
            // 账号 rpm_limit 优先，未设置回退全局默认；满则换号，全满由上方排队等待。
            let rpm_limit = account
                .rpm_limit
                .filter(|&v| v > 0)
                .map(|v| v as i64)
                .unwrap_or(self.default_rpm_limit);
            if rpm_limit > 0 && !self.account_svc.reserve_rpm(account.id, rpm_limit).await {
                warn!(
                    "account {} rpm full (limit {}), switching/queueing",
                    account.id, rpm_limit
                );
                exclude_ids.push(account.id);
                rpm_rejected.push(account.id);
                continue;
            } else if rpm_limit <= 0 {
                // 未配置限速的账号也计当前分钟 RPM（仅用于面板观测，不限流）。
                self.account_svc.incr_rpm(account.id).await;
            }

            // 获取并发槽位
            let acquired = self
                .account_svc
                .acquire_slot(account.id, account.concurrency)
                .await
                .map_err(|_| AppError::TooManyRequests("concurrency slot unavailable".into()))?;
            if !acquired {
                return Err(AppError::TooManyRequests("concurrency slot unavailable".into()));
            }

            // 确保在函数结束后释放槽位
            let account_svc = self.account_svc.clone();
            let account_id_for_release = account.id;
            let _guard = scopeguard::guard((), move |_| {
                let svc = account_svc.clone();
                tokio::spawn(async move {
                    svc.release_slot(account_id_for_release).await;
                });
            });

            // 构建转发请求
            let (final_body, final_headers) = if client_type == ClientType::ClaudeCode {
                // 每账号自己的 identity_mode 优先；账号未设置时回退到全局默认。
                let normalize = account.identity_normalize()
                    || (account.identity_mode.is_empty() && self.identity_normalize);
                if normalize {
                    // 多人共号：把"是谁/哪台机器"归一化成账号固定虚拟身份，
                    // 让一个号始终像同一个人。其余仍尽量保真。
                    let mut bm: serde_json::Value =
                        serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));
                    self.rewriter.normalize_cc_identity(&mut bm, &account);
                    // 漏点A:版本坐标始终从本请求自身提取,让 header 版本 == body 版本(消除不一致)
                    let pkg = headers
                        .get("x-stainless-package-version")
                        .cloned()
                        .unwrap_or_default();
                    let rt = headers
                        .get("x-stainless-runtime-version")
                        .cloned()
                        .unwrap_or_default();
                    let coords = crate::service::rewriter::extract_captured_coords(&ua, &pkg, &rt);
                    // 首次吸取异步存库(仅供面板展示,不影响实际发送)
                    if account.needs_identity_capture() && !coords.cc_version.is_empty() {
                        let svc = self.account_svc.clone();
                        let (aid, cv, pv, rv) = (
                            account.id,
                            coords.cc_version.clone(),
                            coords.package_version.clone(),
                            coords.runtime_version.clone(),
                        );
                        tokio::spawn(async move {
                            svc.persist_captured_identity(aid, &cv, &pv, &rv).await;
                        });
                    }
                    // 漏点B:按本请求版本重算 cc_version 哈希 + cch attestation,与归一化后 body 一致
                    self.rewriter.reattest_cch(&mut bm, &coords.cc_version);
                    let fb = serde_json::to_vec(&bm).unwrap_or_else(|_| body_bytes.to_vec());
                    let fb = crate::service::rewriter::compute_cch_attestation(fb);
                    let mut h = passthrough_headers_ordered(&ordered_headers);
                    self.rewriter.normalize_os_headers_ordered(
                        &mut h,
                        &account,
                        &req_model,
                        Some(&coords),
                    );
                    (fb, h)
                } else {
                    // 纯透传：真实 Claude Code 客户端的请求原样转发，一个字节不改，
                    // 仅注入账号 token。连 header 顺序+大小写都保留客户端原样。
                    (body_bytes.to_vec(), passthrough_headers_ordered(&ordered_headers))
                }
            } else {
                // 非 CC 客户端（API 注入模式）：保留原有"伪装成 CC"的改写
                debug!(
                    "request body BEFORE rewrite: {}",
                    truncate_body(&body_bytes, 4096)
                );
                let rewritten_body =
                    self.rewriter
                        .rewrite_body(&body_bytes, &path, &account, client_type);
                let mut rewritten_body_map: serde_json::Value =
                    serde_json::from_slice(&rewritten_body).unwrap_or(serde_json::json!({}));
                let model_id = body_map.get("model").and_then(|m| m.as_str()).unwrap_or("");
                let rewritten_headers = self.rewriter.rewrite_headers(
                    &headers,
                    &account,
                    client_type,
                    model_id,
                    &rewritten_body_map,
                );
                clean_session_id_from_body(&mut rewritten_body_map);
                let fb = serde_json::to_vec(&rewritten_body_map).unwrap_or(rewritten_body);
                // 排成真 CC 规范顺序，供边车(undici)按序发出
                (fb, order_headers_canonical(&rewritten_headers))
            };

            let upstream_token = self.account_svc.resolve_upstream_token(account.id).await?;
            // 注入账号 token 到真 CC 的 auth 槽位(x-app 之前)，保持顺序一致
            let final_headers = inject_auth_before_xapp(final_headers, &upstream_token);

            // 转发到上游
            let (upstream_status, upstream_headers, resp) = self
                .forward_request(
                    &method.to_string(),
                    &path,
                    &query,
                    &final_headers,
                    &final_body,
                    &account,
                    LogCtx {
                        token_id,
                        account_id: account.id,
                        model: req_model.clone(),
                        stream: is_stream,
                        started: req_started,
                        client_ip: log_client_ip.clone(),
                        user_agent: ua.clone(),
                        path: path.clone(),
                        session_id: log_session_id.clone(),
                        user_id: log_user_id.clone(),
                        req_headers: log_req_headers.clone(),
                    },
                )
                .await?;

            // 非 429 直接返回（RPM 已在 admission 预占，无需此处递增）
            if upstream_status != StatusCode::TOO_MANY_REQUESTS {
                return Ok(resp);
            }

            // 429：根据响应头分类决策
            let classification = self
                .account_svc
                .handle_rate_limit(&account, &upstream_headers)
                .await;

            match classification {
                RateLimitClassification::NotRealRateLimit => {
                    // 非真实 429，直接透传给客户端
                    return Ok(resp);
                }
                RateLimitClassification::BurstRateLimit => {
                    // 同账号重试
                    let retries = same_account_retries.entry(account.id).or_insert(0);
                    if *retries < MAX_SAME_ACCOUNT_RETRIES {
                        *retries += 1;
                        warn!(
                            "account {} burst rate limited, same-account retry {}/{}",
                            account.id, *retries, MAX_SAME_ACCOUNT_RETRIES
                        );
                        // 释放槽位后等待
                        std::mem::forget(_guard);
                        self.account_svc.release_slot(account.id).await;
                        tokio::time::sleep(SAME_ACCOUNT_RETRY_DELAY).await;
                        last_resp = Some(resp);
                        continue; // 不排除账号，重试
                    }
                    // 同账号重试用尽，换号
                }
                RateLimitClassification::FiveHourWall(_)
                | RateLimitClassification::SevenDayWall(_) => {
                    // 撞墙：账号已被 handle_rate_limit 隔离，直接换号
                }
            }

            // 换号
            if switch_count >= MAX_ACCOUNT_SWITCHES {
                warn!(
                    "max account switches ({}) reached, returning last 429",
                    MAX_ACCOUNT_SWITCHES
                );
                return Ok(resp);
            }

            exclude_ids.push(account.id);
            switch_count += 1;
            std::mem::forget(_guard);
            self.account_svc.release_slot(account.id).await;
            last_resp = Some(resp);

            // 换号延迟
            tokio::time::sleep(SWITCH_DELAY).await;
        }
    }

    /// 转发请求到上游，返回 (状态码, 上游响应头, axum Response)。
    /// 状态码和响应头在构建 Response 前 clone 出来，供重试循环判断。
    async fn forward_request(
        &self,
        method: &str,
        path: &str,
        query: &str,
        headers: &[(String, String)],
        body: &[u8],
        account: &Account,
        log: LogCtx,
    ) -> Result<(StatusCode, reqwest::header::HeaderMap, Response), AppError> {
        let upstream_base =
            std::env::var("UPSTREAM_BASE").unwrap_or_else(|_| UPSTREAM_BASE.to_string());
        // path + query(确保带 beta=true)
        let qbeta = if query.is_empty() {
            "beta=true".to_string()
        } else if query.contains("beta=true") {
            query.to_string()
        } else {
            format!("{}&beta=true", query)
        };
        let path_query = format!("{}?{}", path, qbeta);

        // 出口模式：bun_sidecar = 经本地 Bun 边车发出（真实 BoringSSL/Bun 指纹，随版本自动跟随）；
        // 否则用 craftls 直连（内置 Bun 指纹快照）。
        let use_sidecar =
            std::env::var("EGRESS_MODE").map(|v| v == "bun_sidecar").unwrap_or(false);

        let (client, request_url) = if use_sidecar {
            let port = std::env::var("BUN_SIDECAR_PORT").unwrap_or_else(|_| "8788".into());
            let c = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            (c, format!("http://127.0.0.1:{}{}", port, path_query))
        } else {
            (
                crate::tlsfp::make_request_client(&account.proxy_url),
                format!("{}{}", upstream_base, path_query),
            )
        };

        debug!("egress URL: {} (sidecar={})", request_url, use_sidecar);

        let mut req_builder = match method {
            "GET" => client.get(&request_url),
            "POST" => client.post(&request_url),
            "PUT" => client.put(&request_url),
            "DELETE" => client.delete(&request_url),
            "PATCH" => client.patch(&request_url),
            _ => client.post(&request_url),
        };

        if use_sidecar {
            // 边车模式：不逐个发头(到本地边车的这一跳顺序无意义且会被 Bun 重排)，
            // 而是把【有序】请求头打包进 x-ccb-headers，由边车用 undici 按真 CC 顺序发出。
            let payload = serde_json::to_string(headers).unwrap_or_else(|_| "[]".into());
            let b64 = base64::engine::general_purpose::STANDARD.encode(payload);
            req_builder = req_builder.header("x-ccb-upstream", &upstream_base);
            req_builder = req_builder.header("x-ccb-proxy", &account.proxy_url);
            req_builder = req_builder.header("x-ccb-headers", b64);
        } else {
            // craftls 直连：reqwest 按添加顺序发头，逐个按序加上
            for (k, v) in headers {
                req_builder = req_builder.header(k, v);
            }
            req_builder = req_builder.header("Host", "api.anthropic.com");
        }
        req_builder = req_builder.body(body.to_vec());

        let resp = req_builder
            .send()
            .await
            .map_err(|e| {
                warn!("upstream error for account {}: {}", account.id, e);
                AppError::BadGateway("upstream request failed".into())
            })?;

        let status_code_u16 = resp.status().as_u16();
        let status_code = StatusCode::from_u16(status_code_u16)
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        debug!("upstream response: {}", status_code_u16);

        // clone 上游响应头（在消费 body stream 之前）
        let upstream_headers = resp.headers().clone();

        // 处理认证失败：403 永久停用（但如果账号已处于 429 限流中则跳过，避免误判）
        if status_code_u16 == 403 {
            let is_rate_limited = account
                .rate_limit_reset_at
                .map(|reset| Utc::now() < reset)
                .unwrap_or(false);
            if is_rate_limited {
                warn!(
                    "account {} got 403 while rate-limited, skipping permanent disable",
                    account.id
                );
            } else if let Err(e) = self
                .account_svc
                .disable_account(
                    account.id,
                    AccountStatus::Disabled,
                    "403 认证失败",
                    None,
                )
                .await
            {
                warn!("failed to disable account {} for 403: {}", account.id, e);
            } else {
                warn!("account {} permanently disabled for 403", account.id);
            }
        }

        // 构建响应
        let mut response_builder = Response::builder().status(status_code);

        for (k, v) in &upstream_headers {
            let name = k.as_str();
            if is_gateway_fingerprint_header(name) {
                continue;
            }
            response_builder = response_builder.header(k.clone(), v.clone());
        }

        // 流式传输响应体；同时旁路嗅探 token 用量（不改转发字节），body 读完或丢弃时异步落库
        let meta = RecordMeta {
            token_id: log.token_id,
            account_id: log.account_id,
            req_model: log.model.clone(),
            stream: log.stream,
            status_code: status_code.as_u16() as i32,
            started: log.started,
            client_ip: log.client_ip.clone(),
            user_agent: log.user_agent.clone(),
            path: log.path.clone(),
            session_id: log.session_id.clone(),
            user_id: log.user_id.clone(),
            proxy: mask_proxy(&account.proxy_url),
            req_headers: log.req_headers.clone(),
            resp_headers: resp_headers_json(&upstream_headers),
        };
        let inner: std::pin::Pin<
            Box<dyn futures_core::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>,
        > = Box::pin(resp.bytes_stream());
        let sniffed = SniffStream::new(inner, self.usage_recorder.clone(), meta);
        let axum_body = Body::from_stream(sniffed);

        let response = response_builder
            .body(axum_body)
            .map_err(|e| AppError::Internal(format!("build response: {}", e)))?;

        Ok((status_code, upstream_headers, response))
    }

}

fn extract_headers(headers: &HeaderMap) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for (k, v) in headers {
        if let Ok(val) = v.to_str() {
            map.insert(k.to_string(), val.to_string());
        }
    }
    map
}

/// 按客户端发来的【原始 wire 顺序】提取 header（hyper 的 HeaderMap 迭代即插入顺序）。
/// 保留顺序是为了透传时让发往上游的 header 顺序与真实 Claude Code 完全一致。
fn extract_headers_ordered(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(headers.len());
    for (k, v) in headers {
        if let Ok(val) = v.to_str() {
            out.push((k.as_str().to_string(), val.to_string()));
        }
    }
    out
}

/// Claude Code 主动扫描响应头检测 AI Gateway/代理（src/services/api/logging.ts）。
/// 过滤这些指纹前缀以防止客户端上报 gateway 类型。
/// Claude Code 扫描的 AI Gateway 响应头前缀（来源: src/services/api/logging.ts）。
const GATEWAY_HEADER_PREFIXES: &[&str] = &[
    "x-litellm-", "helicone-", "x-portkey-", "cf-aig-", "x-kong-", "x-bt-",
];

fn is_gateway_fingerprint_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    GATEWAY_HEADER_PREFIXES.iter().any(|p| lower.starts_with(p))
}

fn truncate_body(b: &[u8], max: usize) -> String {
    if b.len() > max {
        format!(
            "{}...(truncated)",
            String::from_utf8_lossy(&b[..max])
        )
    } else {
        String::from_utf8_lossy(b).to_string()
    }
}
