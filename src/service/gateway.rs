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
    clean_session_id_from_body, detect_client_type, ClientType, Rewriter,
};
use crate::service::telemetry::TelemetryService;

const UPSTREAM_BASE: &str = "https://api.anthropic.com";

/// 最大换号次数。
const MAX_ACCOUNT_SWITCHES: usize = 5;
/// 换号间隔延迟。
const SWITCH_DELAY: Duration = Duration::from_millis(500);
/// 同账号 BurstRateLimit 最大重试次数。
const MAX_SAME_ACCOUNT_RETRIES: usize = 3;
/// 同账号重试间隔。
const SAME_ACCOUNT_RETRY_DELAY: Duration = Duration::from_millis(500);

pub struct GatewayService {
    account_svc: Arc<AccountService>,
    rewriter: Arc<Rewriter>,
    telemetry_svc: Arc<TelemetryService>,
    client_restriction: ClientRestriction,
}

impl GatewayService {
    pub fn new(
        account_svc: Arc<AccountService>,
        rewriter: Arc<Rewriter>,
        telemetry_svc: Arc<TelemetryService>,
        client_restriction: ClientRestriction,
    ) -> Self {
        Self {
            account_svc,
            rewriter,
            telemetry_svc,
            client_restriction,
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
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let query = req.uri().query().unwrap_or("").to_string();

        // 提取 header
        let headers = extract_headers(req.headers());
        let ua = headers.get("User-Agent").or_else(|| headers.get("user-agent")).cloned().unwrap_or_default();

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

        // 客户端限制：非真实 Claude Code 客户端直接拒绝
        if !client_guard::validate(self.client_restriction, &path, &headers, &body_map) {
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

        // 生成会话哈希
        let session_hash =
            crate::service::account::generate_session_hash(&ua, &body_map, client_type);

        // 根据令牌限制构建账号过滤条件
        let (allowed_ids, blocked_ids) = if let Some(t) = api_token {
            (t.allowed_account_ids(), t.blocked_account_ids())
        } else {
            (vec![], vec![])
        };

        // 429 自动换号重试循环（带延迟、上限、分类决策）
        let mut exclude_ids = blocked_ids.clone();
        let mut last_resp: Option<Response> = None;
        let mut switch_count: usize = 0;
        let mut same_account_retries: std::collections::HashMap<i64, usize> =
            std::collections::HashMap::new();

        loop {
            let attempt = exclude_ids.len().saturating_sub(blocked_ids.len());
            // 选择账号
            let account = match self
                .account_svc
                .select_account(&session_hash, &exclude_ids, &allowed_ids)
                .await
            {
                Ok(a) => a,
                Err(_) if last_resp.is_some() => {
                    return Ok(last_resp.unwrap());
                }
                Err(e) => {
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

            // RPM 检查：超限则排除该账号
            if let Some(rpm_limit) = account.rpm_limit {
                if rpm_limit > 0 {
                    let current_rpm = self.account_svc.get_rpm(account.id).await;
                    if current_rpm >= rpm_limit as i64 {
                        warn!(
                            "account {} rpm exceeded ({}/{}), skipping",
                            account.id, current_rpm, rpm_limit
                        );
                        exclude_ids.push(account.id);
                        continue;
                    }
                }
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

            // 改写请求体
            debug!(
                "request body BEFORE rewrite: {}",
                truncate_body(&body_bytes, 4096)
            );
            let rewritten_body =
                self.rewriter
                    .rewrite_body(&body_bytes, &path, &account, client_type);
            debug!(
                "request body AFTER rewrite: {}",
                truncate_body(&rewritten_body, 4096)
            );

            // 重新解析改写后的 body
            let mut rewritten_body_map: serde_json::Value =
                serde_json::from_slice(&rewritten_body).unwrap_or(serde_json::json!({}));

            // 改写 header
            let model_id = body_map
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let rewritten_headers = self.rewriter.rewrite_headers(
                &headers,
                &account,
                client_type,
                model_id,
                &rewritten_body_map,
            );

            // 清理 body 中的 _session_id 标记并重新序列化
            let final_body = if client_type == ClientType::API {
                clean_session_id_from_body(&mut rewritten_body_map);
                serde_json::to_vec(&rewritten_body_map).unwrap_or_else(|_| rewritten_body.clone())
            } else {
                rewritten_body.clone()
            };

            let upstream_token = self.account_svc.resolve_upstream_token(account.id).await?;
            let mut final_headers = rewritten_headers;
            final_headers.insert(
                "authorization".into(),
                format!("Bearer {}", upstream_token),
            );

            // 转发到上游
            let (upstream_status, upstream_headers, resp) = self
                .forward_request(
                    &method.to_string(),
                    &path,
                    &query,
                    &final_headers,
                    &final_body,
                    &account,
                )
                .await?;

            // 非 429 直接返回
            if upstream_status != StatusCode::TOO_MANY_REQUESTS {
                // 成功转发，递增 RPM
                if upstream_status.is_success() || upstream_status.is_informational() || upstream_status == StatusCode::OK {
                    self.account_svc.incr_rpm(account.id).await;
                }
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
        headers: &std::collections::HashMap<String, String>,
        body: &[u8],
        account: &Account,
    ) -> Result<(StatusCode, reqwest::header::HeaderMap, Response), AppError> {
        let mut target_url = format!("{}{}", UPSTREAM_BASE, path);
        if !query.is_empty() {
            let q = if query.contains("beta=true") {
                query.to_string()
            } else {
                format!("{}&beta=true", query)
            };
            target_url = format!("{}?{}", target_url, q);
        } else {
            target_url = format!("{}?beta=true", target_url);
        }

        debug!("upstream URL: {}", target_url);

        let client = crate::tlsfp::make_request_client(&account.proxy_url);

        let mut req_builder = match method {
            "GET" => client.get(&target_url),
            "POST" => client.post(&target_url),
            "PUT" => client.put(&target_url),
            "DELETE" => client.delete(&target_url),
            "PATCH" => client.patch(&target_url),
            _ => client.post(&target_url),
        };

        for (k, v) in headers {
            debug!("upstream header: {}: {}", k, v);
            req_builder = req_builder.header(k, v);
        }
        req_builder = req_builder.header("Host", "api.anthropic.com");
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

        // 流式传输响应体
        let body_stream = resp.bytes_stream();
        let axum_body = Body::from_stream(body_stream);

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
