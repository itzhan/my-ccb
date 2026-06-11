use axum::extract::{Path, Query, Request, State};
use chrono::TimeZone;
use serde::Deserialize;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rust_embed::Embed;
use std::sync::Arc;

use crate::config::Config;
use crate::error::AppError;
use crate::middleware::auth::{admin_auth, extract_key};
use crate::model::account::{Account, AccountAuthType, AccountStatus};
use crate::model::api_token::{self, ApiToken};
use crate::model::warmup::{WarmupStatus, WarmupTask};
use crate::service::account::AccountService;
use crate::service::account_warmer::AccountWarmerService;
use crate::service::gateway::GatewayService;
use crate::service::oauth::TokenTester;
use crate::service::oauth_flow::OAuthFlowService;
use crate::service::telemetry::TelemetryService;
use crate::store::token_store::TokenStore;
use crate::store::warmup_store::WarmupStore;

#[derive(Clone)]
pub struct AppState {
    pub gateway_svc: Arc<GatewayService>,
    pub account_svc: Arc<AccountService>,
    pub token_tester: Arc<TokenTester>,
    pub token_store: Arc<TokenStore>,
    pub oauth_flow_svc: Arc<OAuthFlowService>,
    pub telemetry_svc: Arc<TelemetryService>,
    pub warmup_store: Arc<WarmupStore>,
    pub warmer_svc: Arc<AccountWarmerService>,
    pub admin_password: String,
    /// 运行时可改的客户端限制级别（与网关共享）。
    pub client_restriction: Arc<std::sync::RwLock<crate::service::client_guard::ClientRestriction>>,
    /// thinking 块 400 自动整流重试开关（与网关共享）。
    pub thinking_repair: Arc<std::sync::atomic::AtomicBool>,
    /// 数据库连接池（设置持久化用）。
    pub pool: sqlx::AnyPool,
}

pub fn build_router(
    cfg: &Config,
    gateway_svc: Arc<GatewayService>,
    account_svc: Arc<AccountService>,
    token_tester: Arc<TokenTester>,
    token_store: Arc<TokenStore>,
    oauth_flow_svc: Arc<OAuthFlowService>,
    telemetry_svc: Arc<TelemetryService>,
    warmup_store: Arc<WarmupStore>,
    warmer_svc: Arc<AccountWarmerService>,
    client_restriction: Arc<std::sync::RwLock<crate::service::client_guard::ClientRestriction>>,
    thinking_repair: Arc<std::sync::atomic::AtomicBool>,
    pool: sqlx::AnyPool,
) -> Router {
    let state = AppState {
        gateway_svc,
        account_svc,
        token_tester,
        token_store,
        oauth_flow_svc,
        telemetry_svc,
        warmup_store,
        warmer_svc,
        admin_password: cfg.admin.password.clone(),
        client_restriction,
        thinking_repair,
        pool,
    };

    let admin_password = state.admin_password.clone();

    // 前端页面（显式注册 SPA 路由）
    let frontend_routes = Router::new()
        .route("/", get(spa_handler))
        .route("/login", get(spa_handler))
        .route("/tokens", get(spa_handler))
        .route("/usage", get(spa_handler))
        .route("/warmup", get(spa_handler))
        .route("/settings", get(spa_handler));

    // 前端静态资源
    let asset_routes = Router::new()
        .route("/assets/*rest", get(asset_handler))
        .route("/favicon.svg", get(asset_handler));

    // 管理 API（密码认证，完整路径注册）
    let admin_routes = Router::new()
        .route("/admin/accounts", get(list_accounts).post(create_account))
        .route(
            "/admin/accounts/:id",
            put(update_account).delete(delete_account),
        )
        .route("/admin/accounts/:id/test", post(test_account))
        .route("/admin/accounts/:id/usage", post(refresh_usage))
        .route("/admin/tokens", get(list_tokens).post(create_token))
        .route(
            "/admin/tokens/:id",
            put(update_token).delete(delete_token_handler),
        )
        .route("/admin/warmup/tasks", get(list_warmup_tasks).post(create_warmup_task))
        .route(
            "/admin/warmup/tasks/:id",
            put(update_warmup_task).delete(delete_warmup_task),
        )
        .route("/admin/warmup/tasks/:id/start", post(start_warmup_task))
        .route("/admin/warmup/tasks/:id/stop", post(stop_warmup_task))
        .route("/admin/warmup/tokens", get(list_warmup_tokens))
        .route("/admin/warmup/ensure-tokens", post(ensure_warmup_tokens))
        .route("/admin/warmup/logs", get(get_warmup_logs))
        .route("/admin/warmup/turns", get(get_warmup_turns))
        .route("/admin/warmup/questions", get(warmup_questions))
        .route("/admin/warmup/questions/count", get(warmup_questions_count))
        .route("/admin/dashboard", get(get_dashboard))
        .route("/admin/settings", get(get_settings).put(update_settings))
        .route("/admin/usage/logs", get(get_usage_logs).delete(delete_usage_logs))
        .route("/admin/usage/stats", get(get_usage_stats))
        .route("/admin/oauth/generate-auth-url", post(oauth_generate_auth_url))
        .route("/admin/oauth/generate-setup-token-url", post(oauth_generate_setup_token_url))
        .route("/admin/oauth/exchange-code", post(oauth_exchange_code))
        .route("/admin/oauth/exchange-session-key", post(oauth_exchange_session_key))
        .route("/admin/oauth/exchange-setup-token-code", post(oauth_exchange_setup_token_code))
        .layer(middleware::from_fn(move |req, next: Next| {
            let pwd = admin_password.clone();
            admin_auth(pwd, req, next)
        }))
        .with_state(state.clone());

    // 组合路由：前端 + 管理 API + 其余全部透传网关
    Router::new()
        .merge(frontend_routes)
        .merge(asset_routes)
        .merge(admin_routes)
        .fallback(gateway_fallback)
        .with_state(state)
}

// --- Handlers ---

/// 网关透传 fallback：鉴权 + 代理上游
async fn gateway_fallback(
    State(state): State<AppState>,
    req: Request,
) -> Response {
    let key = extract_key(&req);
    if key.is_empty() {
        return err_json(StatusCode::UNAUTHORIZED, "missing api key");
    }
    let api_token = match state.token_store.get_by_token(&key).await {
        Ok(Some(t)) => t,
        Ok(None) => return err_json(StatusCode::UNAUTHORIZED, "invalid api key"),
        Err(_) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, "authentication failed"),
    };
    state.gateway_svc.handle_request(req, Some(&api_token)).await
}

/// 统一 JSON 错误响应
fn err_json(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

// --- Account Handlers ---

#[derive(Deserialize)]
struct PageQuery {
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn list_accounts(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(12).clamp(1, 100);
    let (accounts, total) = state.account_svc.list_accounts_paged(page, page_size).await?;
    let total_pages = (total + page_size - 1) / page_size;

    // 批量获取 RPM 计数（所有账号都计数，不再只看 rpm_limit）
    let rpm_account_ids: Vec<i64> = accounts.iter().map(|a| a.id).collect();
    let rpm_counts = if !rpm_account_ids.is_empty() {
        state.account_svc.get_rpm_batch(&rpm_account_ids).await
    } else {
        std::collections::HashMap::new()
    };

    // 为每个账号附加遥测会话过期时间和 current_rpm
    let mut data: Vec<serde_json::Value> = Vec::with_capacity(accounts.len());
    for a in &accounts {
        let mut obj = serde_json::to_value(a).unwrap_or_default();
        if let Some(expires) = state.telemetry_svc.get_session_expires_at(a.id).await {
            obj["telemetry_expires_at"] = serde_json::json!(expires.to_rfc3339());
        }
        obj["current_rpm"] = serde_json::json!(rpm_counts.get(&a.id).copied().unwrap_or(0));
        // 实时并发占用数 + 活跃会话数
        obj["current_concurrency"] = serde_json::json!(state.account_svc.get_slot_count(a.id).await);
        obj["current_sessions"] = serde_json::json!(state.account_svc.session_count(a.id).await);
        // 当天(北京时间固定窗口)已承接的不同设备数 / 不同会话数(配额用量)
        let (cur_devices, cur_window_sessions) = state.account_svc.quota_usage(a.id).await;
        obj["current_devices"] = serde_json::json!(cur_devices);
        obj["current_window_sessions"] = serde_json::json!(cur_window_sessions);
        // 5h 滑动窗口的累计消费(USD,按官方价格表计算)
        obj["cost_5h_usd"] = serde_json::json!(state.account_svc.five_hour_cost(a.id).await);
        // 该账号当前呈现的虚拟身份（自定义优先，留空则派生）+ 机器指纹，供详情展示
        let (vuser, vgit) = a.effective_virtual_identity();
        let env: crate::model::account::CanonicalEnvData =
            serde_json::from_value(a.canonical_env.clone()).unwrap_or_default();
        obj["effective_identity"] = serde_json::json!({
            "device_id": a.device_id,
            "virtual_user": vuser,
            "git_name": vgit,
            "platform": env.platform,
            "arch": env.arch,
        });
        data.push(obj);
    }

    Ok(Json(serde_json::json!({
        "data": data,
        "total": total,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

#[derive(Deserialize)]
struct CreateAccountRequest {
    name: Option<String>,
    email: String,
    token: Option<String>,
    setup_token: Option<String>,
    auth_type: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    proxy_url: Option<String>,
    billing_mode: Option<String>,
    account_uuid: Option<String>,
    organization_uuid: Option<String>,
    subscription_type: Option<String>,
    concurrency: Option<i32>,
    priority: Option<i32>,
    auto_telemetry: Option<bool>,
    rpm_limit: Option<i32>,
    identity_mode: Option<String>,
    virtual_user: Option<String>,
    virtual_git_name: Option<String>,
    path_mode: Option<String>,
    session_mode: Option<String>,
    device_quota: Option<i32>,
    session_quota: Option<i32>,
    warmup_skip: Option<bool>,
    recapture_days: Option<i64>,
    max_sessions: Option<i32>,
    allowed_client_types: Option<String>,
    window_5h_cost_cap_usd: Option<f64>,
}

async fn create_account(
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<Account>), AppError> {
    if req.email.is_empty() {
        return Err(AppError::BadRequest("email is required".into()));
    }
    let auth_type = req
        .auth_type
        .unwrap_or_else(|| "setup_token".into())
        .into();
    let setup_token = req
        .setup_token
        .or(req.token)
        .unwrap_or_default();
    let mut account = Account {
        id: 0,
        name: req.name.unwrap_or_default(),
        email: req.email,
        status: AccountStatus::Active,
        auth_type,
        setup_token,
        access_token: req.access_token.unwrap_or_default(),
        refresh_token: req.refresh_token.unwrap_or_default(),
        expires_at: req.expires_at.and_then(timestamp_millis_to_utc),
        oauth_refreshed_at: None,
        auth_error: String::new(),
        proxy_url: req.proxy_url.unwrap_or_default(),
        device_id: String::new(),
        canonical_env: serde_json::json!({}),
        canonical_prompt: serde_json::json!({}),
        canonical_process: serde_json::json!({}),
        billing_mode: req.billing_mode.unwrap_or_else(|| "strip".into()).into(),
        account_uuid: req.account_uuid,
        organization_uuid: req.organization_uuid,
        subscription_type: req.subscription_type,
        concurrency: req.concurrency.unwrap_or(3),
        priority: req.priority.unwrap_or(50),
        rate_limited_at: None,
        rate_limit_reset_at: None,
        disable_reason: String::new(),
        auto_telemetry: req.auto_telemetry.unwrap_or(false),
        telemetry_count: 0,
        rpm_limit: req.rpm_limit.filter(|&v| v > 0),
        usage_data: serde_json::json!({}),
        usage_fetched_at: None,
        identity_mode: req.identity_mode.unwrap_or_default(),
        virtual_user: req.virtual_user.unwrap_or_default(),
        virtual_git_name: req.virtual_git_name.unwrap_or_default(),
        path_mode: req.path_mode.unwrap_or_default(),
        session_mode: req.session_mode.unwrap_or_default(),
        device_quota: req.device_quota.unwrap_or(10),
        session_quota: req.session_quota.unwrap_or(20),
        warmup_skip: req.warmup_skip.unwrap_or(false),
        identity_captured_at: None,
        captured_session_id: String::new(),
        captured_session_at: None,
        recapture_days: req.recapture_days.unwrap_or(0),
        max_sessions: req.max_sessions.unwrap_or(3),
        allowed_client_types: req.allowed_client_types.unwrap_or_default(),
        window_5h_cost_cap_usd: req.window_5h_cost_cap_usd.filter(|v| *v > 0.0),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.account_svc.create_account(&mut account).await?;
    Ok((StatusCode::CREATED, Json(account)))
}

async fn update_account(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<Account>, AppError> {
    let mut existing = state.account_svc.get_account(id).await?;

    if let Some(name) = updates.get("name").and_then(|v| v.as_str()) {
        if !name.is_empty() {
            existing.name = name.to_string();
        }
    }
    if let Some(email) = updates.get("email").and_then(|v| v.as_str()) {
        if !email.is_empty() {
            existing.email = email.to_string();
        }
    }
    if let Some(auth_type) = updates.get("auth_type").and_then(|v| v.as_str()) {
        existing.auth_type = auth_type.to_string().into();
        match existing.auth_type {
            AccountAuthType::SetupToken => {
                existing.access_token.clear();
                existing.refresh_token.clear();
                existing.expires_at = None;
                existing.oauth_refreshed_at = None;
                existing.auth_error.clear();
            }
            AccountAuthType::Oauth => {
                existing.setup_token.clear();
            }
        }
    }
    if let Some(token) = updates.get("token").and_then(|v| v.as_str()) {
        existing.setup_token = token.to_string();
    }
    if let Some(setup_token) = updates.get("setup_token").and_then(|v| v.as_str()) {
        existing.setup_token = setup_token.to_string();
    }
    if let Some(access_token) = updates.get("access_token").and_then(|v| v.as_str()) {
        existing.access_token = access_token.to_string();
    }
    if let Some(refresh_token) = updates.get("refresh_token").and_then(|v| v.as_str()) {
        existing.refresh_token = refresh_token.to_string();
    }
    if updates.get("expires_at").is_some() {
        existing.expires_at = updates
            .get("expires_at")
            .and_then(|v| v.as_i64())
            .and_then(timestamp_millis_to_utc);
    }
    if let Some(proxy_url) = updates.get("proxy_url").and_then(|v| v.as_str()) {
        existing.proxy_url = proxy_url.to_string();
    }
    if let Some(concurrency) = updates.get("concurrency").and_then(|v| v.as_i64()) {
        if concurrency > 0 {
            existing.concurrency = concurrency as i32;
        }
    }
    if let Some(priority) = updates.get("priority").and_then(|v| v.as_i64()) {
        if priority > 0 {
            existing.priority = priority as i32;
        }
    }
    if let Some(v) = updates.get("identity_mode").and_then(|v| v.as_str()) {
        existing.identity_mode = v.to_string();
    }
    if let Some(v) = updates.get("virtual_user").and_then(|v| v.as_str()) {
        existing.virtual_user = v.to_string();
    }
    if let Some(v) = updates.get("virtual_git_name").and_then(|v| v.as_str()) {
        existing.virtual_git_name = v.to_string();
    }
    if let Some(v) = updates.get("path_mode").and_then(|v| v.as_str()) {
        existing.path_mode = v.to_string();
    }
    if let Some(v) = updates.get("session_mode").and_then(|v| v.as_str()) {
        existing.session_mode = v.to_string();
    }
    if let Some(v) = updates.get("device_quota").and_then(|v| v.as_i64()) {
        existing.device_quota = v as i32;
    }
    if let Some(v) = updates.get("session_quota").and_then(|v| v.as_i64()) {
        existing.session_quota = v as i32;
    }
    if let Some(v) = updates.get("warmup_skip").and_then(|v| v.as_bool()) {
        existing.warmup_skip = v;
    }
    if let Some(v) = updates.get("recapture_days").and_then(|v| v.as_i64()) {
        existing.recapture_days = v.max(0);
    }
    if let Some(v) = updates.get("max_sessions").and_then(|v| v.as_i64()) {
        existing.max_sessions = v.max(0) as i32;
    }
    if let Some(v) = updates.get("allowed_client_types").and_then(|v| v.as_str()) {
        existing.allowed_client_types = v.trim().to_string();
    }
    if let Some(status) = updates.get("status").and_then(|v| v.as_str()) {
        if !status.is_empty() {
            if status == "active" {
                state.account_svc.enable_account(id).await?;
                existing = state.account_svc.get_account(id).await?;
                return Ok(Json(existing));
            } else if status == "disabled" {
                state
                    .account_svc
                    .disable_account(
                        id,
                        AccountStatus::Disabled,
                        "手动停用",
                        None,
                    )
                    .await?;
                existing = state.account_svc.get_account(id).await?;
                return Ok(Json(existing));
            } else {
                existing.status = status.to_string().into();
            }
        }
    }
    if let Some(billing_mode) = updates.get("billing_mode").and_then(|v| v.as_str()) {
        if !billing_mode.is_empty() {
            existing.billing_mode = billing_mode.to_string().into();
        }
    }
    if updates.get("account_uuid").is_some() {
        existing.account_uuid = updates
            .get("account_uuid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if updates.get("organization_uuid").is_some() {
        existing.organization_uuid = updates
            .get("organization_uuid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if updates.get("subscription_type").is_some() {
        existing.subscription_type = updates
            .get("subscription_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if let Some(auto_telemetry) = updates.get("auto_telemetry").and_then(|v| v.as_bool()) {
        existing.auto_telemetry = auto_telemetry;
    }
    if updates.get("rpm_limit").is_some() {
        let val = updates.get("rpm_limit").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        existing.rpm_limit = if val > 0 { Some(val) } else { None };
    }
    if updates.get("window_5h_cost_cap_usd").is_some() {
        let val = updates
            .get("window_5h_cost_cap_usd")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        existing.window_5h_cost_cap_usd = if val > 0.0 { Some(val) } else { None };
        // 改了上限立刻让缓存失效,前端能立刻反映新状态
        crate::service::account::invalidate_cost_cache(id);
    }

    state.account_svc.update_account(&existing).await?;
    Ok(Json(existing))
}

async fn delete_account(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.account_svc.delete_account(id).await?;
    Ok(Json(serde_json::json!({"status": "deleted"})))
}

async fn test_account(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let account = state.account_svc.get_account(id).await?;
    let token = match state.account_svc.resolve_upstream_token(id).await {
        Ok(token) => token,
        Err(e) => {
            return Ok(Json(
                serde_json::json!({"status": "error", "message": e.to_string()}),
            ))
        }
    };
    match state
        .token_tester
        .test_token(&token, &account.proxy_url)
        .await
    {
        Ok(()) => Ok(Json(serde_json::json!({"status": "ok"}))),
        Err(e) => Ok(Json(
            serde_json::json!({"status": "error", "message": e.to_string()}),
        )),
    }
}

async fn refresh_usage(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    match state.account_svc.refresh_usage(id).await {
        Ok(usage) => Ok(Json(serde_json::json!({"status": "ok", "usage": usage}))),
        Err(e) => {
            let message = match &e {
                AppError::TooManyRequests(_) => "用量查询接口超限，请一分钟后再试".to_string(),
                _ => e.to_string(),
            };
            Ok(Json(
                serde_json::json!({"status": "error", "message": message}),
            ))
        }
    }
}

// --- Token Handlers ---

async fn list_tokens(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let total = state.token_store.count().await?;
    let tokens = state.token_store.list_paged(page, page_size).await?;
    let total_pages = (total + page_size - 1) / page_size;
    Ok(Json(serde_json::json!({
        "data": tokens,
        "total": total,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

#[derive(Deserialize)]
struct CreateTokenRequest {
    name: Option<String>,
    allowed_accounts: Option<String>,
    blocked_accounts: Option<String>,
    /// customer（默认）/ warmup（养号专用）。
    category: Option<String>,
    concurrency: Option<i32>,
    /// RFC3339 时间字符串，留空表示永不过期。
    expires_at: Option<String>,
}

async fn create_token(
    State(state): State<AppState>,
    Json(req): Json<CreateTokenRequest>,
) -> Result<(StatusCode, Json<ApiToken>), AppError> {
    let mut token = ApiToken {
        id: 0,
        name: req.name.unwrap_or_default(),
        token: api_token::generate_token(),
        allowed_accounts: req.allowed_accounts.unwrap_or_default(),
        blocked_accounts: req.blocked_accounts.unwrap_or_default(),
        status: api_token::ApiTokenStatus::Active,
        category: req.category.unwrap_or_else(|| "customer".into()).into(),
        concurrency: req.concurrency.unwrap_or(0).max(0),
        expires_at: parse_expires_at(req.expires_at.as_deref()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.token_store.create(&mut token).await?;
    Ok((StatusCode::CREATED, Json(token)))
}

/// 解析 RFC3339 过期时间，空串/None/解析失败均视为不过期。
fn parse_expires_at(s: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s.map(str::trim).filter(|s| !s.is_empty())?;
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

async fn update_token(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<ApiToken>, AppError> {
    let mut existing = state.token_store.get_by_id(id).await?;

    if let Some(name) = updates.get("name").and_then(|v| v.as_str()) {
        existing.name = name.to_string();
    }
    if let Some(allowed) = updates.get("allowed_accounts").and_then(|v| v.as_str()) {
        existing.allowed_accounts = allowed.to_string();
    }
    if let Some(blocked) = updates.get("blocked_accounts").and_then(|v| v.as_str()) {
        existing.blocked_accounts = blocked.to_string();
    }
    if let Some(category) = updates.get("category").and_then(|v| v.as_str()) {
        if !category.is_empty() {
            existing.category = category.to_string().into();
        }
    }
    if let Some(status) = updates.get("status").and_then(|v| v.as_str()) {
        if !status.is_empty() {
            existing.status = status.to_string().into();
        }
    }
    if let Some(concurrency) = updates.get("concurrency").and_then(|v| v.as_i64()) {
        existing.concurrency = (concurrency as i32).max(0);
    }
    // expires_at: 传 null 或空串清除过期，传 RFC3339 字符串设置过期
    if let Some(v) = updates.get("expires_at") {
        existing.expires_at = match v {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => parse_expires_at(Some(s)),
            _ => existing.expires_at,
        };
    }

    state.token_store.update(&existing).await?;
    Ok(Json(existing))
}

async fn delete_token_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.token_store.delete(id).await?;
    Ok(Json(serde_json::json!({"status": "deleted"})))
}

// --- Warmup (养号) Handlers ---

async fn list_warmup_tasks(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tasks = state.warmup_store.list().await?;
    Ok(Json(serde_json::json!({ "data": tasks })))
}

/// 列出所有 warmup 分类的令牌（养号任务选择对象）。
async fn list_warmup_tokens(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tokens = state.token_store.list_by_category("warmup").await?;
    Ok(Json(serde_json::json!({ "data": tokens })))
}

#[derive(Deserialize)]
struct EnsureTokensRequest {
    account_ids: Vec<i64>,
}

/// 按账号确保养号令牌：每个账号若已存在「绑定该单账号的 warmup 令牌」则复用，
/// 否则新建一个(令牌名 = 账号名/邮箱，分类 warmup，allowed_accounts = 该账号)。返回这些令牌。
async fn ensure_warmup_tokens(
    State(state): State<AppState>,
    Json(req): Json<EnsureTokensRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 先创建缺失的(已有「恰好绑定该单账号」的养号令牌则跳过)
    let existing = state.token_store.list_by_category("warmup").await?;
    for acc_id in &req.account_ids {
        if existing
            .iter()
            .any(|t| t.allowed_account_ids() == vec![*acc_id])
        {
            continue;
        }
        let acc = state.account_svc.get_account(*acc_id).await?;
        let name = if !acc.name.is_empty() { acc.name.clone() } else { acc.email.clone() };
        let mut token = ApiToken {
            id: 0,
            name,
            token: api_token::generate_token(),
            allowed_accounts: acc_id.to_string(),
            blocked_accounts: String::new(),
            status: api_token::ApiTokenStatus::Active,
            category: api_token::ApiTokenCategory::Warmup,
            concurrency: 0,
            expires_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        state.token_store.create(&mut token).await?;
    }
    // 重新从库读取，确保返回的 id 真实有效(不依赖 last_insert_id)
    let all = state.token_store.list_by_category("warmup").await?;
    let mut out: Vec<ApiToken> = Vec::new();
    for acc_id in &req.account_ids {
        if let Some(t) = all.iter().find(|t| t.allowed_account_ids() == vec![*acc_id]) {
            out.push(t.clone());
        }
    }
    Ok(Json(serde_json::json!({ "data": out })))
}

/// 养号对话记录(问题+回答),分页,最新在前。
async fn get_warmup_turns(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).clamp(1, 100);
    let (rows, total) = state.warmup_store.list_turns(page, page_size).await?;
    let total_pages = if page_size > 0 { (total + page_size - 1) / page_size } else { 0 };
    Ok(Json(serde_json::json!({
        "data": rows,
        "total": total,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// 养号日志：warmup 分类令牌产生的调用明细（分页）。
async fn get_warmup_logs(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(50).clamp(1, 200);
    let (rows, total) = crate::store::usage_store::list_warmup_logs(&state.pool, page, page_size)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let total_pages = if page_size > 0 { (total + page_size - 1) / page_size } else { 0 };
    Ok(Json(serde_json::json!({
        "data": rows,
        "total": total,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

async fn warmup_questions_count(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "count": state.warmer_svc.questions_count() }))
}

/// 完整题库(供独立本地运行器拉取，只需 url + admin-key 即可获取)。
async fn warmup_questions(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "data": &*state.warmer_svc.questions_arc() }))
}

#[derive(Deserialize)]
struct CreateWarmupTaskRequest {
    name: Option<String>,
    token_ids: Option<String>,
    msg_interval_secs: Option<i64>,
    total_duration_secs: Option<i64>,
    work_duration_secs: Option<i64>,
    rest_duration_secs: Option<i64>,
    jitter_pct: Option<i64>,
    model: Option<String>,
}

async fn create_warmup_task(
    State(state): State<AppState>,
    Json(req): Json<CreateWarmupTaskRequest>,
) -> Result<(StatusCode, Json<WarmupTask>), AppError> {
    let mut task = WarmupTask {
        id: 0,
        name: req.name.unwrap_or_default(),
        token_ids: req.token_ids.unwrap_or_default(),
        msg_interval_secs: req.msg_interval_secs.unwrap_or(60).max(1),
        total_duration_secs: req.total_duration_secs.unwrap_or(3600).max(1),
        work_duration_secs: req.work_duration_secs.unwrap_or(0).max(0),
        rest_duration_secs: req.rest_duration_secs.unwrap_or(0).max(0),
        jitter_pct: req.jitter_pct.unwrap_or(20).clamp(0, 100),
        model: req.model.unwrap_or_default(),
        status: WarmupStatus::Pending,
        error: String::new(),
        messages_sent: 0,
        started_at: None,
        ends_at: None,
        last_message_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.warmup_store.create(&mut task).await?;
    Ok((StatusCode::CREATED, Json(task)))
}

async fn update_warmup_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<WarmupTask>, AppError> {
    let mut existing = state.warmup_store.get_by_id(id).await?;
    if existing.status == WarmupStatus::Running {
        return Err(AppError::BadRequest("任务运行中，请先停止再编辑".into()));
    }
    if let Some(v) = updates.get("name").and_then(|v| v.as_str()) {
        existing.name = v.to_string();
    }
    if let Some(v) = updates.get("token_ids").and_then(|v| v.as_str()) {
        existing.token_ids = v.to_string();
    }
    if let Some(v) = updates.get("msg_interval_secs").and_then(|v| v.as_i64()) {
        existing.msg_interval_secs = v.max(1);
    }
    if let Some(v) = updates.get("total_duration_secs").and_then(|v| v.as_i64()) {
        existing.total_duration_secs = v.max(1);
    }
    if let Some(v) = updates.get("work_duration_secs").and_then(|v| v.as_i64()) {
        existing.work_duration_secs = v.max(0);
    }
    if let Some(v) = updates.get("rest_duration_secs").and_then(|v| v.as_i64()) {
        existing.rest_duration_secs = v.max(0);
    }
    if let Some(v) = updates.get("jitter_pct").and_then(|v| v.as_i64()) {
        existing.jitter_pct = v.clamp(0, 100);
    }
    if let Some(v) = updates.get("model").and_then(|v| v.as_str()) {
        existing.model = v.to_string();
    }
    state.warmup_store.update(&existing).await?;
    Ok(Json(existing))
}

async fn delete_warmup_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.warmer_svc.stop_task(id).await.ok();
    state.warmup_store.delete(id).await?;
    Ok(Json(serde_json::json!({"status": "deleted"})))
}

async fn start_warmup_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.warmer_svc.start_task(id).await?;
    Ok(Json(serde_json::json!({"status": "running"})))
}

async fn stop_warmup_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.warmer_svc.stop_task(id).await?;
    Ok(Json(serde_json::json!({"status": "stopped"})))
}

// --- Dashboard ---

async fn get_dashboard(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let accounts = state.account_svc.list_accounts().await?;
    let token_count = state.token_store.count().await.unwrap_or(0);

    let mut active = 0;
    let mut err_count = 0;
    let mut disabled = 0;
    for a in &accounts {
        match a.status {
            AccountStatus::Active => active += 1,
            AccountStatus::Error => err_count += 1,
            AccountStatus::Disabled => disabled += 1,
        }
    }

    Ok(Json(serde_json::json!({
        "accounts": {
            "total": accounts.len(),
            "active": active,
            "error": err_count,
            "disabled": disabled,
        },
        "tokens": token_count,
    })))
}

// --- Settings ---

async fn get_settings(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cr = state
        .client_restriction
        .read()
        .map(|g| g.as_str())
        .unwrap_or("off");
    let tr = if state
        .thinking_repair
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        "on"
    } else {
        "off"
    };
    let warm = state.account_svc.warmup_config();
    Json(serde_json::json!({
        "client_restriction": cr,
        "thinking_repair": tr,
        "warmup_enabled": if warm.enabled { "on" } else { "off" },
        "warmup_schedule": warm.to_json(),
    }))
}

async fn update_settings(
    State(state): State<AppState>,
    Json(updates): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(v) = updates.get("client_restriction").and_then(|v| v.as_str()) {
        let parsed = crate::service::client_guard::ClientRestriction::from_env(v);
        crate::store::db::set_setting(&state.pool, "client_restriction", parsed.as_str())
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if let Ok(mut g) = state.client_restriction.write() {
            *g = parsed;
        }
    }
    if let Some(v) = updates.get("thinking_repair").and_then(|v| v.as_str()) {
        let on = v == "on";
        crate::store::db::set_setting(&state.pool, "thinking_repair", if on { "on" } else { "off" })
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        state
            .thinking_repair
            .store(on, std::sync::atomic::Ordering::Relaxed);
    }
    // 新号升温:开关 + 区间表(JSON 数组)。任一字段更新即持久化 + 热更新到 AccountService。
    {
        let mut warm = state.account_svc.warmup_config();
        let mut changed = false;
        if let Some(v) = updates.get("warmup_enabled").and_then(|v| v.as_str()) {
            warm.enabled = v == "on";
            crate::store::db::set_setting(&state.pool, "warmup_enabled", if warm.enabled { "on" } else { "off" })
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;
            changed = true;
        }
        if let Some(v) = updates.get("warmup_schedule").and_then(|v| v.as_str()) {
            if let Some(tiers) = crate::service::account::WarmupConfig::parse(v) {
                warm.tiers = tiers;
                crate::store::db::set_setting(&state.pool, "warmup_schedule", &warm.to_json())
                    .await
                    .map_err(|e| AppError::Internal(e.to_string()))?;
                changed = true;
            }
        }
        if changed {
            state.account_svc.set_warmup(warm);
        }
    }
    let cr = state
        .client_restriction
        .read()
        .map(|g| g.as_str())
        .unwrap_or("off");
    let tr = if state
        .thinking_repair
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        "on"
    } else {
        "off"
    };
    let warm = state.account_svc.warmup_config();
    Ok(Json(serde_json::json!({
        "client_restriction": cr,
        "thinking_repair": tr,
        "warmup_enabled": if warm.enabled { "on" } else { "off" },
        "warmup_schedule": warm.to_json(),
    })))
}

// --- Usage Logs Handlers ---

#[derive(Deserialize)]
struct UsageLogQuery {
    token_id: Option<i64>,
    account_id: Option<i64>,
    model: Option<String>,
    result: Option<String>,
    start: Option<String>,
    end: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn get_usage_logs(
    State(state): State<AppState>,
    Query(q): Query<UsageLogQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 500);
    let (rows, total) = crate::store::usage_store::list_logs(
        &state.pool,
        q.token_id,
        q.account_id,
        q.model.as_deref().filter(|s| !s.is_empty()),
        q.result.as_deref().filter(|s| !s.is_empty()),
        q.start.as_deref(),
        q.end.as_deref(),
        page,
        page_size,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    let total_pages = if page_size > 0 { (total + page_size - 1) / page_size } else { 0 };
    Ok(Json(serde_json::json!({
        "data": rows,
        "total": total,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

#[derive(Deserialize)]
struct UsageStatQuery {
    group_by: Option<String>,
    start: Option<String>,
    end: Option<String>,
}

async fn get_usage_stats(
    State(state): State<AppState>,
    Query(q): Query<UsageStatQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let group_by = q.group_by.as_deref().unwrap_or("total");
    let rows = crate::store::usage_store::stats(
        &state.pool,
        group_by,
        q.start.as_deref(),
        q.end.as_deref(),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "data": rows })))
}

#[derive(Deserialize)]
struct UsagePruneQuery {
    before: String,
}

async fn delete_usage_logs(
    State(state): State<AppState>,
    Query(q): Query<UsagePruneQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    crate::store::usage_store::prune_logs_before(&state.pool, &q.before)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- OAuth Flow Handlers ---

async fn oauth_generate_auth_url(
    State(state): State<AppState>,
    Json(req): Json<crate::service::oauth_flow::GenerateAuthUrlRequest>,
) -> Json<crate::service::oauth_flow::GenerateAuthUrlResponse> {
    Json(state.oauth_flow_svc.generate_auth_url(&req))
}

async fn oauth_generate_setup_token_url(
    State(state): State<AppState>,
    Json(req): Json<crate::service::oauth_flow::GenerateAuthUrlRequest>,
) -> Json<crate::service::oauth_flow::GenerateAuthUrlResponse> {
    Json(state.oauth_flow_svc.generate_setup_token_url(&req))
}

async fn oauth_exchange_code(
    State(state): State<AppState>,
    Json(req): Json<crate::service::oauth_flow::ExchangeCodeRequest>,
) -> Result<Json<crate::service::oauth_flow::ExchangeCodeResponse>, AppError> {
    let resp = state.oauth_flow_svc.exchange_code(&req).await?;
    Ok(Json(resp))
}

/// 粘贴 claude.ai sessionKey 一步录号（自动完成 OAuth 授权换 token）。
async fn oauth_exchange_session_key(
    State(state): State<AppState>,
    Json(req): Json<crate::service::oauth_flow::SessionKeyExchangeRequest>,
) -> Result<Json<crate::service::oauth_flow::ExchangeCodeResponse>, AppError> {
    let resp = state.oauth_flow_svc.exchange_session_key(&req).await?;
    Ok(Json(resp))
}

async fn oauth_exchange_setup_token_code(
    State(state): State<AppState>,
    Json(req): Json<crate::service::oauth_flow::ExchangeCodeRequest>,
) -> Result<Json<crate::service::oauth_flow::ExchangeCodeResponse>, AppError> {
    let resp = state.oauth_flow_svc.exchange_setup_token_code(&req).await?;
    Ok(Json(resp))
}

// --- 内嵌前端静态资源 ---

#[derive(Embed)]
#[folder = "web/dist"]
struct Assets;

/// SPA 页面：返回 index.html
async fn spa_handler() -> impl IntoResponse {
    match Assets::get("index.html") {
        Some(index) => Response::builder()
            .header("content-type", "text/html")
            .body(axum::body::Body::from(index.data.to_vec()))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::from("frontend not built"))
            .unwrap(),
    }
}

/// 前端静态资源：/assets/*
async fn asset_handler(req: Request) -> impl IntoResponse {
    let path = req.uri().path().trim_start_matches('/');
    if let Some(file) = Assets::get(path) {
        let mime = mime_from_path(path);
        return Response::builder()
            .header("content-type", mime)
            .body(axum::body::Body::from(file.data.to_vec()))
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(axum::body::Body::from("not found"))
        .unwrap()
}

fn mime_from_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
}

fn timestamp_millis_to_utc(ts: i64) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::Utc.timestamp_millis_opt(ts).single()
}
