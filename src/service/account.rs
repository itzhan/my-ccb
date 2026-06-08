use chrono::Utc;
use once_cell::sync::Lazy;
use rand::seq::SliceRandom;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::AppError;
use crate::model::account::{Account, AccountAuthType};
use crate::service::rewriter::ClientType;
use crate::store::account_store::AccountStore;
use crate::store::cache::CacheStore;

const STICKY_SESSION_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const OAUTH_REFRESH_BUFFER_SECONDS: i64 = 5 * 60;
/// 会话空闲多久视为结束、自动腾出并发会话名额。
const SESSION_TTL: Duration = Duration::from_secs(60);
/// 账号设备/会话「总量配额」的【固定窗口】长度(24h,对齐北京时间 0 点滚动):
/// 同一固定窗口内该号服务过的不同设备数 / 不同会话数各自不超过 device_quota / session_quota;
/// 每天北京时间 0 点整体清零、配额重置(不是滑动窗口)。
const QUOTA_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// 所有号都满时,新会话排队的重试间隔与最大次数(约 20s)。
const SESSION_WAIT_RETRY: Duration = Duration::from_millis(500);
const SESSION_WAIT_ATTEMPTS: usize = 40;
const OAUTH_LOCK_TTL: Duration = Duration::from_secs(30);
const OAUTH_WAIT_RETRY: Duration = Duration::from_millis(500);
const OAUTH_WAIT_ATTEMPTS: usize = 20;

/// 账号 5h 消费的内存缓存 TTL —— 避免每次选号都查 DB。
const COST_CACHE_TTL: Duration = Duration::from_secs(60);
/// 5h 配额接近 cap 多少比例时降级为兜底(优先选其他号)。
const COST_SOFT_LIMIT_RATIO: f64 = 0.85;

/// 账号 5h 消费缓存:account_id -> (cost_usd, computed_at)。
static COST_CACHE: Lazy<Mutex<HashMap<i64, (f64, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 写入/刷新缓存。
fn cost_cache_put(account_id: i64, cost: f64) {
    if let Ok(mut map) = COST_CACHE.lock() {
        map.insert(account_id, (cost, Instant::now()));
    }
}

/// 命中且未过期则返回 Some(cost)。
fn cost_cache_get(account_id: i64) -> Option<f64> {
    let map = COST_CACHE.lock().ok()?;
    let (cost, at) = map.get(&account_id)?;
    if at.elapsed() < COST_CACHE_TTL {
        Some(*cost)
    } else {
        None
    }
}

/// 强制失效(刚记完一笔账时调用,让下次查询拿到新值)。
pub fn invalidate_cost_cache(account_id: i64) {
    if let Ok(mut map) = COST_CACHE.lock() {
        map.remove(&account_id);
    }
}

/// 用量利用率达到此阈值即视为”撞墙”。
const USAGE_HIT_THRESHOLD: f64 = 97.0;
/// 撞墙之外的纯速率限制的短冷却时间。
const PURE_RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(60);
/// 无法确定限流原因时的保守限流时长（与历史行为一致）。
const FALLBACK_QUARANTINE: Duration = Duration::from_secs(5 * 60 * 60);

/// 429 限流分类结果。
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitClassification {
    /// 7 天用量墙命中，隔离到 reset 时间。
    SevenDayWall(chrono::DateTime<Utc>),
    /// 5 小时用量墙命中，隔离到 reset 时间。
    FiveHourWall(chrono::DateTime<Utc>),
    /// 纯速率限制（未撞墙），短冷却。
    BurstRateLimit,
    /// 非真实 429（如 “Extra usage required”），不隔离。
    NotRealRateLimit,
}

pub struct AccountService {
    store: Arc<AccountStore>,
    cache: Arc<dyn CacheStore>,
}

impl AccountService {
    pub fn new(store: Arc<AccountStore>, cache: Arc<dyn CacheStore>) -> Self {
        Self { store, cache }
    }

    /// 创建新账号并自动生成身份信息。
    pub async fn create_account(&self, a: &mut Account) -> Result<(), AppError> {
        let (device_id, env, prompt, process) =
            crate::model::identity::generate_canonical_identity();
        a.device_id = device_id;
        a.canonical_env = env;
        a.canonical_prompt = prompt;
        a.canonical_process = process;

        if a.status == crate::model::account::AccountStatus::Active && a.status.to_string() == "active" {
            // default already active
        }
        if a.concurrency == 0 {
            a.concurrency = 3;
        }
        if a.priority == 0 {
            a.priority = 50;
        }
        if a.billing_mode == crate::model::account::BillingMode::Strip
            && a.billing_mode.to_string() == "strip"
        {
            // default already strip
        }

        normalize_account_auth(a)?;

        self.store.create(a).await
    }

    pub async fn update_account(&self, a: &Account) -> Result<(), AppError> {
        let mut normalized = a.clone();
        normalize_account_auth(&mut normalized)?;
        self.store.update(&normalized).await
    }

    pub async fn delete_account(&self, id: i64) -> Result<(), AppError> {
        self.store.delete(id).await
    }

    pub async fn get_account(&self, id: i64) -> Result<Account, AppError> {
        self.store.get_by_id(id).await
    }

    pub async fn list_accounts(&self) -> Result<Vec<Account>, AppError> {
        self.store.list().await
    }

    /// 号池是否存在任意一个可调度账号（active + 未限流）。供探针健康判定用,只读无副作用。
    pub async fn has_schedulable_account(&self) -> bool {
        self.store
            .list_schedulable()
            .await
            .map(|v| v.iter().any(|a| a.is_schedulable()))
            .unwrap_or(false)
    }

    pub async fn list_accounts_paged(&self, page: i64, page_size: i64) -> Result<(Vec<Account>, i64), AppError> {
        let total = self.store.count().await?;
        let accounts = self.store.list_paged(page, page_size).await?;
        Ok((accounts, total))
    }

    /// 使用粘性会话为请求选择账号。
    /// `exclude_ids` 为令牌的不可用账号，`allowed_ids` 为令牌的可用账号（空表示不限制）。
    pub async fn select_account(
        &self,
        session_hash: &str,
        exclude_ids: &[i64],
        allowed_ids: &[i64],
    ) -> Result<Account, AppError> {
        // 检查粘性会话:粘性号若已 5h 耗尽,允许跳过去找新号。
        if !session_hash.is_empty() {
            if let Ok(Some(account_id)) = self.cache.get_session_account_id(session_hash).await {
                if account_id > 0 {
                    if let Ok(account) = self.store.get_by_id(account_id).await {
                        let id_allowed = allowed_ids.is_empty() || allowed_ids.contains(&account_id);
                        let cost = self.five_hour_cost(account_id).await;
                        let exhausted = Self::cost_exhausted(&account, cost);
                        if account.is_schedulable()
                            && !exclude_ids.contains(&account_id)
                            && id_allowed
                            && !exhausted
                        {
                            return Ok(account);
                        }
                        if exhausted {
                            warn!(
                                "account {} 5h cost ${:.2} >= cap ${:.2}, releasing sticky binding",
                                account_id,
                                cost,
                                account.window_5h_cost_cap_usd.unwrap_or(0.0),
                            );
                        }
                    }
                    // 过期绑定 / 已耗尽 → 删除
                    let _ = self.cache.delete_session(session_hash).await;
                }
            }
        }

        // 获取可调度账号
        let accounts = self.store.list_schedulable().await?;

        // 过滤:排除项 + 可用账号限制 + 5h 已耗尽的
        let mut primary: Vec<Account> = Vec::new();
        let mut fallback: Vec<Account> = Vec::new();
        for a in accounts.into_iter() {
            if exclude_ids.contains(&a.id) {
                continue;
            }
            if !(allowed_ids.is_empty() || allowed_ids.contains(&a.id)) {
                continue;
            }
            let cost = self.five_hour_cost(a.id).await;
            if Self::cost_exhausted(&a, cost) {
                continue; // 5h 配额已耗尽,本轮不可用
            }
            if Self::cost_soft_limited(&a, cost) {
                fallback.push(a); // 接近上限,降级
            } else {
                primary.push(a);
            }
        }

        let candidates = if !primary.is_empty() {
            primary
        } else if !fallback.is_empty() {
            fallback
        } else {
            return Err(AppError::ServiceUnavailable(
                "no available accounts (all exhausted or rate-limited)".into(),
            ));
        };

        // 按优先级 + 实时会话占用率排序，取最高优先级里最空的号
        let ranked = self.rank_candidates(candidates).await;
        let selected = ranked.into_iter().next().ok_or_else(|| {
            AppError::ServiceUnavailable("no available accounts after ranking".into())
        })?;

        // 绑定粘性会话
        if !session_hash.is_empty() {
            let _ = self
                .cache
                .set_session_account_id(session_hash, selected.id, STICKY_SESSION_TTL)
                .await;
        }

        Ok(selected)
    }

    /// 把候选账号按调度顺序排序(select_account / admit_session 共用,保证两条路径口径一致):
    /// 1) 优先级数值小者优先(高优先级);
    /// 2) 同优先级按会话占用率(活跃会话数 / max_sessions)升序 —— 选当前最空的号做负载均衡;
    /// 3) 占用率相同随机打散,避免总落在同一个号上。
    async fn rank_candidates(&self, candidates: Vec<Account>) -> Vec<Account> {
        // 预取各号活跃会话数,算占用率。max_sessions<=0(不限)时退化为按绝对会话数比较。
        let mut scored: Vec<(f64, Account)> = Vec::with_capacity(candidates.len());
        for a in candidates {
            let count = self.cache.session_count(a.id, SESSION_TTL).await;
            let ratio = if a.max_sessions > 0 {
                count as f64 / a.max_sessions as f64
            } else {
                count as f64
            };
            scored.push((ratio, a));
        }
        // 先随机打散,再做稳定排序 → 优先级/占用率并列的项保持随机顺序。
        scored.shuffle(&mut rand::thread_rng());
        scored.sort_by(|(ra, aa), (rb, ab)| aa.priority.cmp(&ab.priority).then(ra.total_cmp(rb)));
        scored.into_iter().map(|(_, a)| a).collect()
    }

    /// 尝试获取账号的并发槽位。
    pub async fn acquire_slot(&self, account_id: i64, max: i32) -> Result<bool, AppError> {
        let key = format!("concurrency:account:{}", account_id);
        self.cache
            .acquire_slot(&key, max, Duration::from_secs(300))
            .await
    }

    /// 释放并发槽位。
    pub async fn release_slot(&self, account_id: i64) {
        let key = format!("concurrency:account:{}", account_id);
        self.cache.release_slot(&key).await;
    }

    /// 读取账号当前并发占用数（实时展示用）。
    pub async fn get_slot_count(&self, account_id: i64) -> i64 {
        let key = format!("concurrency:account:{}", account_id);
        self.cache.get_slot_count(&key).await
    }

    /// 账号当前活跃会话数(实时展示用)。
    pub async fn session_count(&self, account_id: i64) -> i64 {
        self.cache.session_count(account_id, SESSION_TTL).await
    }

    /// 账号本固定窗口(北京时间当天)已承接的不同设备数 / 不同会话数(配额用量展示用)。
    pub async fn quota_usage(&self, account_id: i64) -> (i64, i64) {
        self.cache.quota_counts(account_id, QUOTA_TTL).await
    }

    /// 账号过去 5 小时累计消费(USD,内存缓存 60s)。
    pub async fn five_hour_cost(&self, account_id: i64) -> f64 {
        if let Some(c) = cost_cache_get(account_id) {
            return c;
        }
        let cost = self
            .store
            .compute_5h_cost_usd(account_id)
            .await
            .unwrap_or(0.0);
        cost_cache_put(account_id, cost);
        cost
    }

    /// 5h 配额已耗尽 → 直接跳过此号。
    pub fn cost_exhausted(account: &Account, cost_5h: f64) -> bool {
        match account.window_5h_cost_cap_usd {
            Some(cap) if cap > 0.0 => cost_5h >= cap,
            _ => false,
        }
    }

    /// 5h 配额接近上限(>=85%)→ 仅在没有更便宜的号时才用。
    pub fn cost_soft_limited(account: &Account, cost_5h: f64) -> bool {
        match account.window_5h_cost_cap_usd {
            Some(cap) if cap > 0.0 => cost_5h >= cap * COST_SOFT_LIMIT_RATIO,
            _ => false,
        }
    }

    /// 并发会话准入(满则排队等待)。返回 true=已为该会话占到/确认一个账号;false=超时仍满。
    /// 已粘性绑定的老会话始终放行;新会话只进有容量的号;全满则等待直到有名额或超时。
    pub async fn admit_session(
        &self,
        session_id: &str,
        device_id: &str,
        session_hash: &str,
        allowed_ids: &[i64],
        blocked_ids: &[i64],
    ) -> bool {
        if session_id.is_empty() {
            return true; // 无 session id(如 count_tokens)不限制
        }
        for _ in 0..SESSION_WAIT_ATTEMPTS {
            // 1) 已粘性绑定的老会话:始终放行(force),刷新活跃时间;但 5h 耗尽除外
            if !session_hash.is_empty() {
                if let Ok(Some(aid)) = self.cache.get_session_account_id(session_hash).await {
                    if aid > 0 {
                        if let Ok(acc) = self.store.get_by_id(aid).await {
                            let id_allowed =
                                allowed_ids.is_empty() || allowed_ids.contains(&aid);
                            let cost = self.five_hour_cost(aid).await;
                            let exhausted = Self::cost_exhausted(&acc, cost);
                            if acc.is_schedulable()
                                && !blocked_ids.contains(&aid)
                                && id_allowed
                                && !exhausted
                            {
                                // 老会话(已粘性绑定)始终放行,并刷新其在并发集合 + 配额窗口的活跃时间。
                                self.cache
                                    .session_admit(aid, session_id, acc.max_sessions, SESSION_TTL, true)
                                    .await;
                                self.cache
                                    .account_quota_admit(
                                        aid, device_id, session_id,
                                        acc.device_quota, acc.session_quota, QUOTA_TTL, true,
                                    )
                                    .await;
                                return true;
                            }
                        }
                    }
                }
            }
            // 2) 新会话:按优先级找一个有会话容量且 5h 未耗尽的号,占位 + 绑定粘性
            if let Ok(accounts) = self.store.list_schedulable().await {
                let mut primary: Vec<Account> = Vec::new();
                let mut fallback: Vec<Account> = Vec::new();
                for a in accounts.into_iter() {
                    if blocked_ids.contains(&a.id) {
                        continue;
                    }
                    if !(allowed_ids.is_empty() || allowed_ids.contains(&a.id)) {
                        continue;
                    }
                    let cost = self.five_hour_cost(a.id).await;
                    if Self::cost_exhausted(&a, cost) {
                        continue;
                    }
                    if Self::cost_soft_limited(&a, cost) {
                        fallback.push(a);
                    } else {
                        primary.push(a);
                    }
                }
                let cands = if !primary.is_empty() { primary } else { fallback };
                // 与 select_account 统一:优先级数值小者优先,同优先级选会话最空的号。
                let ranked = self.rank_candidates(cands).await;
                for acc in &ranked {
                    // 新会话:先过瞬时并发上限(max_sessions),再过 24h 总量配额(设备≤device_quota、
                    // 会话≤session_quota)。两者都过才占号;配额满的号本轮跳过,改选别的号。
                    if self
                        .cache
                        .session_admit(acc.id, session_id, acc.max_sessions, SESSION_TTL, false)
                        .await
                        && self
                            .cache
                            .account_quota_admit(
                                acc.id, device_id, session_id,
                                acc.device_quota, acc.session_quota, QUOTA_TTL, false,
                            )
                            .await
                    {
                        if !session_hash.is_empty() {
                            let _ = self
                                .cache
                                .set_session_account_id(session_hash, acc.id, STICKY_SESSION_TTL)
                                .await;
                        }
                        return true;
                    }
                }
            }
            // 3) 全满 → 排队等待
            sleep(SESSION_WAIT_RETRY).await;
        }
        false
    }

    /// 持久化从客户端吸取的版本坐标(CC 版本/package/runtime)到账号 canonical_env。
    /// 异步调用,best-effort,失败不影响转发。
    pub async fn persist_captured_identity(
        &self,
        account_id: i64,
        cc_version: &str,
        package_version: &str,
        runtime_version: &str,
    ) {
        let account = match self.store.get_by_id(account_id).await {
            Ok(a) => a,
            Err(_) => return,
        };
        let mut env = account.canonical_env.clone();
        if let Some(obj) = env.as_object_mut() {
            if !cc_version.is_empty() {
                obj.insert("version".into(), serde_json::json!(cc_version));
                obj.insert("version_base".into(), serde_json::json!(cc_version));
            }
            if !runtime_version.is_empty() {
                obj.insert("node_version".into(), serde_json::json!(runtime_version));
            }
            if !package_version.is_empty() {
                obj.insert("package_version".into(), serde_json::json!(package_version));
            }
        }
        let now = Utc::now();
        if let Err(e) = self
            .store
            .update_captured_identity(account_id, &env, &now)
            .await
        {
            warn!("persist captured identity failed for {}: {}", account_id, e);
        }
    }

    /// 持久化当前对上游呈现的 session_id（每 15-20min 轮换吸取时调用）。纯展示用，失败仅告警。
    pub async fn persist_captured_session(&self, account_id: i64, session_id: &str) {
        if let Err(e) = self.store.update_captured_session(account_id, session_id).await {
            warn!("persist captured session failed for {}: {}", account_id, e);
        }
    }

    /// 尝试获取 API 令牌维度的并发槽位。
    pub async fn acquire_token_slot(&self, token_id: i64, max: i32) -> Result<bool, AppError> {
        let key = format!("concurrency:token:{}", token_id);
        self.cache
            .acquire_slot(&key, max, Duration::from_secs(300))
            .await
    }

    /// 释放 API 令牌维度的并发槽位。
    pub async fn release_token_slot(&self, token_id: i64) {
        let key = format!("concurrency:token:{}", token_id);
        self.cache.release_slot(&key).await;
    }

    /// 从 Anthropic API 获取账号用量并缓存到数据库。
    /// 仅支持 OAuth 账号，SetupToken 账号无法查询用量。
    pub async fn refresh_usage(&self, id: i64) -> Result<serde_json::Value, AppError> {
        let account = self.store.get_by_id(id).await?;
        if account.auth_type != crate::model::account::AccountAuthType::Oauth {
            return Err(AppError::BadRequest(
                "usage query is only supported for OAuth accounts, SetupToken accounts cannot query usage via this endpoint".into(),
            ));
        }
        let token = self.resolve_oauth_access_token(&account).await?;
        let usage = crate::service::oauth::fetch_usage(&token, &account.proxy_url).await?;
        let usage_str = serde_json::to_string(&usage).unwrap_or_else(|_| "{}".into());
        self.store.update_usage(id, &usage_str).await?;
        Ok(usage)
    }

    pub async fn resolve_upstream_token(&self, id: i64) -> Result<String, AppError> {
        let account = self.store.get_by_id(id).await?;
        match account.auth_type {
            AccountAuthType::SetupToken => {
                if account.setup_token.is_empty() {
                    return Err(AppError::ServiceUnavailable(
                        "setup token is empty".into(),
                    ));
                }
                Ok(account.setup_token)
            }
            AccountAuthType::Oauth => self.resolve_oauth_access_token(&account).await,
        }
    }

    async fn resolve_oauth_access_token(&self, account: &Account) -> Result<String, AppError> {
        if account.has_valid_oauth_access_token(OAUTH_REFRESH_BUFFER_SECONDS) {
            return Ok(account.access_token.clone());
        }
        if account.refresh_token.is_empty() {
            let _ = self
                .store
                .update_auth_error(account.id, "missing refresh token")
                .await;
            return Err(AppError::ServiceUnavailable(
                "oauth refresh token is empty".into(),
            ));
        }

        let lock_key = format!("oauth:refresh:account:{}", account.id);
        let lock_owner = Uuid::new_v4().to_string();
        let acquired = self
            .cache
            .acquire_lock(&lock_key, &lock_owner, OAUTH_LOCK_TTL)
            .await?;

        if acquired {
            let result = self.refresh_oauth_access_token(account.id).await;
            self.cache.release_lock(&lock_key, &lock_owner).await;
            return result;
        }

        for _ in 0..OAUTH_WAIT_ATTEMPTS {
            sleep(OAUTH_WAIT_RETRY).await;
            let latest = self.store.get_by_id(account.id).await?;
            if latest.has_valid_oauth_access_token(OAUTH_REFRESH_BUFFER_SECONDS) {
                return Ok(latest.access_token);
            }
        }

        Err(AppError::ServiceUnavailable(
            "oauth token refresh timeout".into(),
        ))
    }

    async fn refresh_oauth_access_token(&self, id: i64) -> Result<String, AppError> {
        let latest = self.store.get_by_id(id).await?;
        if latest.has_valid_oauth_access_token(OAUTH_REFRESH_BUFFER_SECONDS) {
            return Ok(latest.access_token);
        }
        if latest.refresh_token.is_empty() {
            let _ = self
                .store
                .update_auth_error(id, "missing refresh token")
                .await;
            return Err(AppError::ServiceUnavailable(
                "oauth refresh token is empty".into(),
            ));
        }

        let fallback_access_token = latest.access_token.clone();
        let fallback_is_still_valid = latest
            .expires_at
            .map(|expires_at| expires_at > Utc::now())
            .unwrap_or(false);

        match crate::service::oauth::refresh_oauth_token(&latest.refresh_token, &latest.proxy_url).await {
            Ok(tokens) => {
                self.store
                    .update_oauth_tokens(
                        id,
                        &tokens.access_token,
                        &tokens.refresh_token,
                        tokens.expires_at,
                    )
                    .await?;
                Ok(tokens.access_token)
            }
            Err(err) => {
                let msg = err.to_string();
                let _ = self.store.update_auth_error(id, &msg).await;
                if fallback_is_still_valid && !fallback_access_token.is_empty() {
                    warn!(
                        "oauth refresh failed for account {}, using current access token until expiry: {}",
                        id, msg
                    );
                    return Ok(fallback_access_token);
                }
                Err(AppError::ServiceUnavailable(format!(
                    "oauth refresh failed: {}",
                    msg
                )))
            }
        }
    }

    /// 获取账号当前分钟 RPM 计数。
    pub async fn get_rpm(&self, account_id: i64) -> i64 {
        self.cache.get_rpm(account_id).await.unwrap_or(0)
    }

    /// 批量获取多个账号的 RPM 计数。
    pub async fn get_rpm_batch(&self, account_ids: &[i64]) -> std::collections::HashMap<i64, i64> {
        self.cache.get_rpm_batch(account_ids).await.unwrap_or_default()
    }

    /// 递增账号 RPM 计数（成功转发后调用）。
    pub async fn incr_rpm(&self, account_id: i64) -> i64 {
        self.cache.incr_rpm(account_id).await.unwrap_or(0)
    }

    /// admission 时预占当前分钟 RPM 配额：未超 limit 返回 true（已计数），已满返回 false。
    /// 缓存出错时放行（false-open），不因限速基础设施故障阻断请求。
    pub async fn reserve_rpm(&self, account_id: i64, limit: i64) -> bool {
        self.cache.reserve_rpm(account_id, limit).await.unwrap_or(true)
    }

    pub async fn set_rate_limit(
        &self,
        id: i64,
        reset_at: chrono::DateTime<Utc>,
    ) -> Result<(), AppError> {
        self.store.set_rate_limit(id, reset_at).await
    }

    pub async fn disable_account(
        &self,
        id: i64,
        status: crate::model::account::AccountStatus,
        reason: &str,
        rate_limit_reset_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), AppError> {
        self.store
            .disable_account(id, status, reason, rate_limit_reset_at)
            .await
    }

    pub async fn enable_account(&self, id: i64) -> Result<(), AppError> {
        self.store.enable_account(id).await
    }

    /// 处理上游返回 429 的情况。
    ///
    /// 优先从响应头解析限流窗口（SetupToken/OAuth 统一），不再同步调 usage API。
    /// 返回分类结果供重试循环决策。
    pub async fn handle_rate_limit(
        &self,
        account: &Account,
        upstream_headers: &reqwest::header::HeaderMap,
    ) -> RateLimitClassification {
        let classification = classify_from_headers(upstream_headers);
        let now = Utc::now();

        match &classification {
            RateLimitClassification::SevenDayWall(reset_at) => {
                info!(
                    "account {} hit 7-day wall, quarantine until {}",
                    account.id,
                    reset_at.to_rfc3339()
                );
                let _ = self
                    .store
                    .disable_account(
                        account.id,
                        crate::model::account::AccountStatus::Active,
                        "周限额已满",
                        Some(*reset_at),
                    )
                    .await;
            }
            RateLimitClassification::FiveHourWall(reset_at) => {
                info!(
                    "account {} hit 5-hour wall, quarantine until {}",
                    account.id,
                    reset_at.to_rfc3339()
                );
                let _ = self
                    .store
                    .disable_account(
                        account.id,
                        crate::model::account::AccountStatus::Active,
                        "5 小时限额已满",
                        Some(*reset_at),
                    )
                    .await;
            }
            RateLimitClassification::BurstRateLimit => {
                let reset_at =
                    now + chrono::Duration::from_std(PURE_RATE_LIMIT_COOLDOWN).unwrap();
                info!(
                    "account {} burst rate limited, short cooldown until {}",
                    account.id,
                    reset_at.to_rfc3339()
                );
                let _ = self
                    .store
                    .disable_account(
                        account.id,
                        crate::model::account::AccountStatus::Active,
                        "速率限制（未达用量墙）",
                        Some(reset_at),
                    )
                    .await;
            }
            RateLimitClassification::NotRealRateLimit => {
                warn!(
                    "account {} got 429 without rate limit headers, not quarantining (likely not a real rate limit)",
                    account.id
                );
            }
        }

        classification
    }
}

// ---------------------------------------------------------------------------
// Anthropic 429 响应头解析（参照 sub2api ratelimit_service.go）
// ---------------------------------------------------------------------------

/// 从 Anthropic 429 响应头判断限流类型。
///
/// 解析 `anthropic-ratelimit-unified-{5h,7d}-{reset,utilization,surpassed-threshold}` 头，
/// 不依赖 usage API。SetupToken 和 OAuth 统一走此路径。
pub fn classify_from_headers(headers: &reqwest::header::HeaderMap) -> RateLimitClassification {
    let five_h_exceeded = is_window_exceeded(headers, "5h");
    let seven_d_exceeded = is_window_exceeded(headers, "7d");

    let five_h_reset = parse_reset_header(headers, "5h");
    let seven_d_reset = parse_reset_header(headers, "7d");

    // 检查聚合 reset 头（兜底）
    let unified_reset = headers
        .get("anthropic-ratelimit-unified-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default());

    let has_any_reset = five_h_reset.is_some() || seven_d_reset.is_some() || unified_reset.is_some();

    // 选择逻辑：优先看哪个窗口超限
    if seven_d_exceeded {
        if let Some(reset) = seven_d_reset.or(five_h_reset).or(unified_reset) {
            return RateLimitClassification::SevenDayWall(reset);
        }
    }
    if five_h_exceeded {
        if let Some(reset) = five_h_reset.or(seven_d_reset).or(unified_reset) {
            return RateLimitClassification::FiveHourWall(reset);
        }
    }

    // 都没超限但有 reset 头 → 纯速率限制
    if has_any_reset {
        return RateLimitClassification::BurstRateLimit;
    }

    // Retry-After 兜底
    if let Some(retry_after) = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        if retry_after > 0 {
            return RateLimitClassification::BurstRateLimit;
        }
    }

    // 没有任何限流头 → 非真实 429
    RateLimitClassification::NotRealRateLimit
}

/// 检查指定窗口是否超限。
fn is_window_exceeded(headers: &reqwest::header::HeaderMap, window: &str) -> bool {
    // 1. surpassed-threshold 头（最明确）
    let threshold_key = format!("anthropic-ratelimit-unified-{}-surpassed-threshold", window);
    if let Some(val) = headers.get(threshold_key.as_str()).and_then(|v| v.to_str().ok()) {
        if val.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    // 2. utilization >= 1.0
    let util_key = format!("anthropic-ratelimit-unified-{}-utilization", window);
    if let Some(val) = headers.get(util_key.as_str()).and_then(|v| v.to_str().ok()) {
        if let Ok(util) = val.parse::<f64>() {
            if util >= 1.0 - 1e-9 {
                return true;
            }
        }
    }
    false
}

/// 解析指定窗口的 reset Unix 时间戳头。
fn parse_reset_header(
    headers: &reqwest::header::HeaderMap,
    window: &str,
) -> Option<chrono::DateTime<Utc>> {
    let key = format!("anthropic-ratelimit-unified-{}-reset", window);
    let val = headers.get(key.as_str())?.to_str().ok()?;
    let ts = val.parse::<i64>().ok()?;
    let dt = chrono::DateTime::from_timestamp(ts, 0)?;
    if dt <= Utc::now() {
        return None;
    }
    Some(dt)
}

// ---------------------------------------------------------------------------
// 旧 usage JSON 分类（保留用于 dashboard 用量查询等场景）
// ---------------------------------------------------------------------------

/// 命中的用量窗口类型。
enum RateLimitWindow {
    /// 7 天窗口命中，携带其 resets_at。
    SevenDay(chrono::DateTime<Utc>),
    /// 5 小时窗口命中，携带其 resets_at。
    FiveHour(chrono::DateTime<Utc>),
}

/// 根据 usage_data JSON 判断哪个窗口撞墙。
/// 优先检查 7 天窗口（同时命中时 7 天 reset 更晚，限流更久）。
/// Sonnet 7 天窗口暂不纳入判断。
fn classify_rate_limit(
    usage: &serde_json::Value,
    threshold: f64,
) -> Option<RateLimitWindow> {
    if let Some(reset_at) = check_usage_window(usage, "seven_day", threshold) {
        return Some(RateLimitWindow::SevenDay(reset_at));
    }
    if let Some(reset_at) = check_usage_window(usage, "five_hour", threshold) {
        return Some(RateLimitWindow::FiveHour(reset_at));
    }
    None
}

/// 检查单个窗口是否达到撞墙阈值，返回其 resets_at（若命中且在未来）。
fn check_usage_window(
    usage: &serde_json::Value,
    key: &str,
    threshold: f64,
) -> Option<chrono::DateTime<Utc>> {
    let window = usage.get(key)?;
    let util = window.get("utilization")?.as_f64()?;
    if util < threshold {
        return None;
    }
    let resets_at_str = window.get("resets_at")?.as_str()?;
    let dt = chrono::DateTime::parse_from_rfc3339(resets_at_str)
        .ok()?
        .with_timezone(&Utc);
    if dt <= Utc::now() {
        return None;
    }
    Some(dt)
}

fn normalize_account_auth(account: &mut Account) -> Result<(), AppError> {
    match account.auth_type {
        AccountAuthType::SetupToken => {
            if account.setup_token.trim().is_empty() {
                return Err(AppError::BadRequest("setup_token is required".into()));
            }
            account.access_token.clear();
            account.refresh_token.clear();
            account.expires_at = None;
            account.oauth_refreshed_at = None;
            account.auth_error.clear();
        }
        AccountAuthType::Oauth => {
            if account.refresh_token.trim().is_empty() {
                return Err(AppError::BadRequest("refresh_token is required".into()));
            }
            account.setup_token.clear();
            account.auth_error.clear();
            if account.access_token.trim().is_empty() {
                account.access_token.clear();
                account.expires_at = None;
            }
        }
    }
    Ok(())
}

/// 根据客户端类型创建会话哈希。
/// CC 客户端：使用 metadata.user_id 中的 session_id。
/// API 客户端：使用 sha256(UA + 系统提示词/首条消息 + 小时窗口)。
pub fn generate_session_hash(
    user_agent: &str,
    body: &serde_json::Value,
    client_type: ClientType,
) -> String {
    if client_type == ClientType::ClaudeCode {
        if let Some(metadata) = body.get("metadata").and_then(|m| m.as_object()) {
            if let Some(user_id_str) = metadata.get("user_id").and_then(|u| u.as_str()) {
                // JSON 格式
                if let Ok(uid) = serde_json::from_str::<serde_json::Value>(user_id_str) {
                    if let Some(sid) = uid.get("session_id").and_then(|s| s.as_str()) {
                        if !sid.is_empty() {
                            return sid.to_string();
                        }
                    }
                }
                // 旧格式
                if let Some(idx) = user_id_str.rfind("_session_") {
                    return user_id_str[idx + 9..].to_string();
                }
            }
        }
    }

    // API 模式：UA + 系统提示词/首条消息 + 小时窗口
    let mut content = String::new();

    // Try system prompt first
    match body.get("system") {
        Some(serde_json::Value::String(sys)) => {
            content = sys.clone();
        }
        Some(serde_json::Value::Array(arr)) => {
            for item in arr {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    content = text.to_string();
                    break;
                }
            }
        }
        _ => {}
    }

    // 回退到首条消息
    if content.is_empty() {
        if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
            if let Some(msg) = messages.first().and_then(|m| m.as_object()) {
                match msg.get("content") {
                    Some(serde_json::Value::String(c)) => {
                        content = c.clone();
                    }
                    Some(serde_json::Value::Array(arr)) => {
                        for item in arr {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                content = text.to_string();
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let hour_window = Utc::now().format("%Y-%m-%dT%H").to_string();
    let raw = format!("{}|{}|{}", user_agent, content, hour_window);
    let hash = Sha256::digest(raw.as_bytes());
    hex::encode(&hash[..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use serde_json::json;

    /// 生成一个相对当前时间指定偏移的 RFC3339 字符串。
    fn rfc3339_at(offset: ChronoDuration) -> String {
        (Utc::now() + offset).to_rfc3339()
    }

    fn make_window(util: serde_json::Value, resets_at: &str) -> serde_json::Value {
        json!({ "utilization": util, "resets_at": resets_at })
    }

    // ---- check_usage_window ----

    #[test]
    fn check_window_below_threshold_returns_none() {
        let future = rfc3339_at(ChronoDuration::hours(1));
        let usage = json!({ "five_hour": make_window(json!(96.9), &future) });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_at_threshold_returns_some() {
        let future = rfc3339_at(ChronoDuration::hours(1));
        let usage = json!({ "five_hour": make_window(json!(97.0), &future) });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_some());
    }

    #[test]
    fn check_window_above_threshold_returns_some() {
        let future = rfc3339_at(ChronoDuration::hours(1));
        let usage = json!({ "five_hour": make_window(json!(99.9), &future) });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_some());
    }

    #[test]
    fn check_window_integer_utilization_works() {
        let future = rfc3339_at(ChronoDuration::hours(1));
        let usage = json!({ "five_hour": make_window(json!(100), &future) });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_some());
    }

    #[test]
    fn check_window_missing_key_returns_none() {
        let usage = json!({});
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_missing_utilization_returns_none() {
        let future = rfc3339_at(ChronoDuration::hours(1));
        let usage = json!({ "five_hour": { "resets_at": future } });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_missing_resets_at_returns_none() {
        let usage = json!({ "five_hour": { "utilization": 100 } });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_invalid_rfc3339_returns_none() {
        let usage = json!({
            "five_hour": { "utilization": 100, "resets_at": "not-a-date" }
        });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_past_time_returns_none() {
        let past = rfc3339_at(ChronoDuration::hours(-1));
        let usage = json!({ "five_hour": make_window(json!(100), &past) });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_null_utilization_returns_none() {
        let future = rfc3339_at(ChronoDuration::hours(1));
        let usage = json!({
            "five_hour": { "utilization": null, "resets_at": future }
        });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_string_utilization_returns_none() {
        let future = rfc3339_at(ChronoDuration::hours(1));
        let usage = json!({
            "five_hour": { "utilization": "100", "resets_at": future }
        });
        assert!(check_usage_window(&usage, "five_hour", 97.0).is_none());
    }

    #[test]
    fn check_window_returns_parsed_reset_at() {
        let future = rfc3339_at(ChronoDuration::hours(3));
        let usage = json!({ "five_hour": make_window(json!(100), &future) });
        let result = check_usage_window(&usage, "five_hour", 97.0).unwrap();
        let expected = chrono::DateTime::parse_from_rfc3339(&future)
            .unwrap()
            .with_timezone(&Utc);
        // 允许纳秒级精度差
        assert_eq!(result.timestamp(), expected.timestamp());
    }

    // ---- classify_rate_limit ----

    #[test]
    fn classify_empty_usage_returns_none() {
        let usage = json!({});
        assert!(classify_rate_limit(&usage, 97.0).is_none());
    }

    #[test]
    fn classify_only_five_hour_hit_returns_five_hour() {
        let future = rfc3339_at(ChronoDuration::hours(2));
        let usage = json!({
            "five_hour": make_window(json!(100), &future),
            "seven_day": make_window(json!(50), &rfc3339_at(ChronoDuration::days(5))),
        });
        match classify_rate_limit(&usage, 97.0) {
            Some(RateLimitWindow::FiveHour(_)) => {}
            other => panic!("expected FiveHour, got {:?}", match other {
                Some(RateLimitWindow::SevenDay(_)) => "SevenDay",
                Some(RateLimitWindow::FiveHour(_)) => "FiveHour",
                None => "None",
            }),
        }
    }

    #[test]
    fn classify_only_seven_day_hit_returns_seven_day() {
        let usage = json!({
            "five_hour": make_window(json!(50), &rfc3339_at(ChronoDuration::hours(2))),
            "seven_day": make_window(json!(99), &rfc3339_at(ChronoDuration::days(5))),
        });
        assert!(matches!(
            classify_rate_limit(&usage, 97.0),
            Some(RateLimitWindow::SevenDay(_))
        ));
    }

    #[test]
    fn classify_both_hit_prioritizes_seven_day() {
        // 同时命中时，7 天窗口优先（限流更久）
        let usage = json!({
            "five_hour": make_window(json!(100), &rfc3339_at(ChronoDuration::hours(2))),
            "seven_day": make_window(json!(100), &rfc3339_at(ChronoDuration::days(5))),
        });
        assert!(matches!(
            classify_rate_limit(&usage, 97.0),
            Some(RateLimitWindow::SevenDay(_))
        ));
    }

    #[test]
    fn classify_only_sonnet_hit_is_ignored() {
        // Sonnet 7 天窗口命中，但其他两个未命中 → 返回 None（暂不处理 sonnet）
        let usage = json!({
            "five_hour": make_window(json!(10), &rfc3339_at(ChronoDuration::hours(2))),
            "seven_day": make_window(json!(10), &rfc3339_at(ChronoDuration::days(5))),
            "seven_day_sonnet": make_window(json!(100), &rfc3339_at(ChronoDuration::days(5))),
        });
        assert!(classify_rate_limit(&usage, 97.0).is_none());
    }

    #[test]
    fn classify_all_below_threshold_returns_none() {
        let usage = json!({
            "five_hour": make_window(json!(80), &rfc3339_at(ChronoDuration::hours(2))),
            "seven_day": make_window(json!(50), &rfc3339_at(ChronoDuration::days(5))),
        });
        assert!(classify_rate_limit(&usage, 97.0).is_none());
    }

    #[test]
    fn classify_boundary_at_exactly_97() {
        let usage = json!({
            "five_hour": make_window(json!(97), &rfc3339_at(ChronoDuration::hours(2))),
        });
        assert!(matches!(
            classify_rate_limit(&usage, 97.0),
            Some(RateLimitWindow::FiveHour(_))
        ));
    }

    #[test]
    fn classify_boundary_just_below_97() {
        let usage = json!({
            "five_hour": make_window(json!(96.99), &rfc3339_at(ChronoDuration::hours(2))),
        });
        assert!(classify_rate_limit(&usage, 97.0).is_none());
    }

    #[test]
    fn classify_seven_day_expired_reset_falls_through_to_five_hour() {
        // 7d utilization 命中但 resets_at 已过期 → check_usage_window 返回 None，降级到 5h 检查
        let usage = json!({
            "five_hour": make_window(json!(100), &rfc3339_at(ChronoDuration::hours(2))),
            "seven_day": make_window(json!(100), &rfc3339_at(ChronoDuration::hours(-1))),
        });
        assert!(matches!(
            classify_rate_limit(&usage, 97.0),
            Some(RateLimitWindow::FiveHour(_))
        ));
    }

    #[test]
    fn classify_invalid_json_structure_returns_none() {
        let usage = json!("not-an-object");
        assert!(classify_rate_limit(&usage, 97.0).is_none());
    }

    #[test]
    fn classify_threshold_config_is_honored() {
        // 测试不同 threshold 参数行为
        let usage = json!({
            "five_hour": make_window(json!(95), &rfc3339_at(ChronoDuration::hours(2))),
        });
        assert!(classify_rate_limit(&usage, 97.0).is_none());
        assert!(classify_rate_limit(&usage, 90.0).is_some());
    }
}
