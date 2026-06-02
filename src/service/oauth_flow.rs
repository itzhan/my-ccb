use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// OAuth 常量
// ---------------------------------------------------------------------------

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";

const SCOPE_FULL: &str = "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
const SCOPE_INFERENCE: &str = "user:inference";

/// 会话 TTL（30 分钟）。
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

/// Setup-Token 有效期（1 年）。
const SETUP_TOKEN_EXPIRES_IN: i64 = 365 * 24 * 60 * 60;

/// claude.ai 网页 API 基址（session-key 自动授权用）。
const CLAUDE_AI_BASE: &str = "https://claude.ai";

/// 访问 claude.ai 时使用的浏览器 User-Agent。
const BROWSER_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

// ---------------------------------------------------------------------------
// PKCE 工具
// ---------------------------------------------------------------------------

/// base64url 编码（无填充）。
fn base64url_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// 生成 PKCE code_verifier（32 字节随机 → 43 字符 base64url）。
fn generate_code_verifier() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill(&mut buf);
    base64url_encode(&buf)
}

/// 计算 S256 code_challenge。
fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64url_encode(&hash)
}

/// 生成随机 state。
fn generate_state() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill(&mut buf);
    base64url_encode(&buf)
}

/// 生成 session_id。
fn generate_session_id() -> String {
    let mut buf = [0u8; 16];
    rand::thread_rng().fill(&mut buf);
    hex::encode(buf)
}

// ---------------------------------------------------------------------------
// 会话存储
// ---------------------------------------------------------------------------

struct OAuthSession {
    state: String,
    code_verifier: String,
    scope: String,
    proxy_url: String,
    created_at: Instant,
}

/// 内存级 OAuth 会话存储，带 TTL 自动清理。
struct SessionStore {
    sessions: Mutex<HashMap<String, OAuthSession>>,
}

impl SessionStore {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn set(&self, id: &str, session: OAuthSession) {
        let mut map = self.sessions.lock().unwrap();
        // 顺便清理过期会话
        map.retain(|_, s| s.created_at.elapsed() < SESSION_TTL);
        map.insert(id.to_string(), session);
    }

    fn take(&self, id: &str) -> Option<OAuthSession> {
        let mut map = self.sessions.lock().unwrap();
        map.remove(id)
    }
}

// ---------------------------------------------------------------------------
// 请求 / 响应
// ---------------------------------------------------------------------------

/// 生成授权 URL 的请求。
#[derive(Deserialize)]
pub struct GenerateAuthUrlRequest {
    pub proxy_url: Option<String>,
}

/// 生成授权 URL 的响应。
#[derive(Serialize)]
pub struct GenerateAuthUrlResponse {
    pub auth_url: String,
    pub session_id: String,
}

/// 交换 code 的请求。
#[derive(Deserialize)]
pub struct ExchangeCodeRequest {
    pub session_id: String,
    pub code: String,
}

/// session-key 自动授权请求（粘贴 claude.ai sessionKey 录号）。
#[derive(Deserialize)]
pub struct SessionKeyExchangeRequest {
    pub session_key: String,
    pub proxy_url: Option<String>,
}

/// claude.ai /api/organizations 返回项。
#[derive(Deserialize)]
struct OrgEntry {
    uuid: String,
    #[serde(default)]
    raven_type: Option<String>,
}

/// claude.ai /v1/oauth/{org}/authorize 返回体。
#[derive(Deserialize)]
struct AuthorizeResponse {
    #[serde(default)]
    redirect_uri: String,
}

/// 交换 code 的响应。
#[derive(Serialize)]
pub struct ExchangeCodeResponse {
    pub access_token: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub refresh_token: String,
    pub expires_in: i64,
    pub expires_at: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub account_uuid: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub organization_uuid: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub email_address: String,
}

/// 平台 token exchange 原始响应。
#[derive(Deserialize)]
struct TokenExchangeResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    scope: String,
    account: Option<TokenAccount>,
    organization: Option<TokenOrganization>,
}

#[derive(Deserialize)]
struct TokenAccount {
    uuid: String,
    #[serde(default)]
    email_address: String,
}

#[derive(Deserialize)]
struct TokenOrganization {
    uuid: String,
}

// ---------------------------------------------------------------------------
// OAuthFlowService
// ---------------------------------------------------------------------------

/// 处理 OAuth 授权链接生成和 code 交换。
pub struct OAuthFlowService {
    store: SessionStore,
}

impl OAuthFlowService {
    pub fn new() -> Self {
        Self {
            store: SessionStore::new(),
        }
    }

    /// 生成 OAuth 授权 URL（完整 scope）。
    pub fn generate_auth_url(&self, req: &GenerateAuthUrlRequest) -> GenerateAuthUrlResponse {
        self.build_auth_url(SCOPE_FULL, req.proxy_url.as_deref().unwrap_or(""))
    }

    /// 生成 Setup-Token 授权 URL（仅 user:inference）。
    pub fn generate_setup_token_url(
        &self,
        req: &GenerateAuthUrlRequest,
    ) -> GenerateAuthUrlResponse {
        self.build_auth_url(SCOPE_INFERENCE, req.proxy_url.as_deref().unwrap_or(""))
    }

    /// 交换 code 获取 OAuth token（完整 scope）。
    pub async fn exchange_code(
        &self,
        req: &ExchangeCodeRequest,
    ) -> Result<ExchangeCodeResponse, AppError> {
        self.do_exchange(&req.session_id, &req.code, false).await
    }

    /// 交换 code 获取 Setup-Token。
    pub async fn exchange_setup_token_code(
        &self,
        req: &ExchangeCodeRequest,
    ) -> Result<ExchangeCodeResponse, AppError> {
        self.do_exchange(&req.session_id, &req.code, true).await
    }

    /// session-key 一步录号：粘贴 claude.ai sessionKey，自动完成 OAuth 授权并换取 token。
    /// 移植自 sub2api：GetOrganizationUUID → GetAuthorizationCode → ExchangeCodeForToken。
    pub async fn exchange_session_key(
        &self,
        req: &SessionKeyExchangeRequest,
    ) -> Result<ExchangeCodeResponse, AppError> {
        let session_key = req.session_key.trim();
        if session_key.is_empty() {
            return Err(AppError::BadRequest("session_key is required".into()));
        }
        let proxy_url = req.proxy_url.as_deref().unwrap_or("");

        let state = generate_state();
        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);

        // Step 1: 用 sessionKey 拿组织 UUID
        let org_uuid = self.get_organization_uuid(session_key, proxy_url).await?;
        // Step 2: 用 sessionKey 在 claude.ai 上自动授权，拿 authorization code
        let full_code = self
            .get_authorization_code(session_key, &org_uuid, SCOPE_FULL, &code_challenge, &state, proxy_url)
            .await?;
        // Step 3: 用 code 换 token（完整 scope，非 setup-token）
        self.exchange_core(&full_code, &code_verifier, &state, proxy_url, false)
            .await
    }

    // --- 内部实现 ---

    /// Step 1：带 sessionKey cookie 请求 claude.ai 组织列表，优先返回 team 组织。
    async fn get_organization_uuid(
        &self,
        session_key: &str,
        proxy_url: &str,
    ) -> Result<String, AppError> {
        let client = crate::tlsfp::make_request_client(proxy_url);
        let resp = client
            .get(format!("{}/api/organizations", CLAUDE_AI_BASE))
            .header("Cookie", format!("sessionKey={}", session_key))
            .header("User-Agent", BROWSER_UA)
            .header("Accept", "application/json")
            .header("Referer", "https://claude.ai/")
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("get organizations request failed: {}", e)))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(AppError::BadRequest(format!(
                "无法获取组织（sessionKey 可能无效或已过期）: status {} {}",
                status,
                truncate(&text, 256)
            )));
        }

        let orgs: Vec<OrgEntry> = serde_json::from_str(&text).map_err(|_| {
            AppError::BadRequest(
                "无法解析组织列表（claude.ai 返回非 JSON，sessionKey 可能无效或已过期）".into(),
            )
        })?;
        if orgs.is_empty() {
            return Err(AppError::BadRequest("该 sessionKey 下没有任何组织".into()));
        }
        // 优先选择 team 组织
        if let Some(team) = orgs
            .iter()
            .find(|o| o.raven_type.as_deref() == Some("team"))
        {
            return Ok(team.uuid.clone());
        }
        Ok(orgs[0].uuid.clone())
    }

    /// Step 2：带 sessionKey cookie POST claude.ai 授权端点，从返回的 redirect_uri 抠出 code。
    async fn get_authorization_code(
        &self,
        session_key: &str,
        org_uuid: &str,
        scope: &str,
        code_challenge: &str,
        state: &str,
        proxy_url: &str,
    ) -> Result<String, AppError> {
        let client = crate::tlsfp::make_request_client(proxy_url);
        let url = format!("{}/v1/oauth/{}/authorize", CLAUDE_AI_BASE, org_uuid);
        let body = serde_json::json!({
            "response_type": "code",
            "client_id": CLIENT_ID,
            "organization_uuid": org_uuid,
            "redirect_uri": REDIRECT_URI,
            "scope": scope,
            "state": state,
            "code_challenge": code_challenge,
            "code_challenge_method": "S256",
        });

        let resp = client
            .post(&url)
            .header("Cookie", format!("sessionKey={}", session_key))
            .header("User-Agent", BROWSER_UA)
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Origin", "https://claude.ai")
            .header("Referer", "https://claude.ai/new")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("authorize request failed: {}", e)))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(AppError::BadRequest(format!(
                "自动授权失败: status {} {}",
                status,
                truncate(&text, 256)
            )));
        }

        let parsed: AuthorizeResponse = serde_json::from_str(&text).map_err(|_| {
            AppError::BadRequest(
                "无法解析授权响应（claude.ai 返回非 JSON，sessionKey 可能无效或已过期）".into(),
            )
        })?;
        if parsed.redirect_uri.is_empty() {
            return Err(AppError::BadRequest("授权响应中没有 redirect_uri".into()));
        }

        let auth_code = extract_query_param(&parsed.redirect_uri, "code")
            .ok_or_else(|| AppError::BadRequest("redirect_uri 中没有 code".into()))?;
        let resp_state = extract_query_param(&parsed.redirect_uri, "state").unwrap_or_default();

        Ok(if resp_state.is_empty() {
            auth_code
        } else {
            format!("{}#{}", auth_code, resp_state)
        })
    }

    fn build_auth_url(&self, scope: &str, proxy_url: &str) -> GenerateAuthUrlResponse {
        let state = generate_state();
        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);
        let session_id = generate_session_id();

        self.store.set(
            &session_id,
            OAuthSession {
                state: state.clone(),
                code_verifier,
                scope: scope.to_string(),
                proxy_url: proxy_url.to_string(),
                created_at: Instant::now(),
            },
        );

        let encoded_redirect = percent_encode(REDIRECT_URI);
        let encoded_scope = scope.replace(' ', "+");

        let auth_url = format!(
            "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
            AUTHORIZE_URL, CLIENT_ID, encoded_redirect, encoded_scope, code_challenge, state
        );

        debug!("generated auth URL for session {}", session_id);

        GenerateAuthUrlResponse {
            auth_url,
            session_id,
        }
    }

    async fn do_exchange(
        &self,
        session_id: &str,
        raw_code: &str,
        is_setup_token: bool,
    ) -> Result<ExchangeCodeResponse, AppError> {
        let session = self
            .store
            .take(session_id)
            .ok_or_else(|| AppError::BadRequest("invalid or expired session_id".into()))?;

        if session.created_at.elapsed() >= SESSION_TTL {
            return Err(AppError::BadRequest("session expired".into()));
        }

        self.exchange_core(
            raw_code,
            &session.code_verifier,
            &session.state,
            &session.proxy_url,
            is_setup_token,
        )
        .await
    }

    /// 用 authorization code 换 token（手动流程与 session-key 流程共用）。
    async fn exchange_core(
        &self,
        raw_code: &str,
        code_verifier: &str,
        state_fallback: &str,
        proxy_url: &str,
        is_setup_token: bool,
    ) -> Result<ExchangeCodeResponse, AppError> {
        // code 可能携带 state：code#state
        let (auth_code, code_state) = if let Some(idx) = raw_code.find('#') {
            (&raw_code[..idx], &raw_code[idx + 1..])
        } else {
            (raw_code, "")
        };

        // 构建 token exchange 请求体
        let mut body = serde_json::json!({
            "grant_type": "authorization_code",
            "code": auth_code,
            "redirect_uri": REDIRECT_URI,
            "client_id": CLIENT_ID,
            "code_verifier": code_verifier,
        });

        if !code_state.is_empty() {
            body["state"] = serde_json::Value::String(code_state.to_string());
        } else if !state_fallback.is_empty() {
            body["state"] = serde_json::Value::String(state_fallback.to_string());
        }

        if is_setup_token {
            body["expires_in"] = serde_json::json!(SETUP_TOKEN_EXPIRES_IN);
        }

        debug!("exchanging authorization code for token");

        // 发送 token exchange 请求
        let client = crate::tlsfp::make_request_client(proxy_url);
        let resp = client
            .post(TOKEN_URL)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("token exchange request failed: {}", e)))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "token exchange failed: status {} {}",
                status, text
            )));
        }

        let token_resp: TokenExchangeResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("token exchange parse failed: {}", e)))?;

        let expires_in = if token_resp.expires_in > 0 {
            token_resp.expires_in
        } else {
            3600
        };
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        Ok(ExchangeCodeResponse {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            expires_in,
            expires_at,
            scope: token_resp.scope,
            account_uuid: token_resp
                .account
                .as_ref()
                .map(|a| a.uuid.clone())
                .unwrap_or_default(),
            email_address: token_resp
                .account
                .as_ref()
                .map(|a| a.email_address.clone())
                .unwrap_or_default(),
            organization_uuid: token_resp
                .organization
                .as_ref()
                .map(|o| o.uuid.clone())
                .unwrap_or_default(),
        })
    }
}

/// 从 URL 的 query 中取出指定参数（值做 percent-decode）。
fn extract_query_param(uri: &str, key: &str) -> Option<String> {
    let query = uri.split_once('?').map(|(_, q)| q)?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(percent_decode(v));
            }
        }
    }
    None
}

/// 简易 percent-decode（%XX 与 +→空格）。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 截断字符串用于错误信息展示（按字符截断，避免切到多字节边界）。
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max).collect::<String>())
    }
}

/// 简易 percent-encode（仅编码 URL 不安全字符）。
fn percent_encode(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 3);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

