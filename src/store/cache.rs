use crate::error::AppError;
use std::collections::HashMap;
use std::time::Duration;

#[axum::async_trait]
pub trait CacheStore: Send + Sync {
    async fn get_session_account_id(&self, session_hash: &str) -> Result<Option<i64>, AppError>;
    async fn set_session_account_id(
        &self,
        session_hash: &str,
        account_id: i64,
        ttl: Duration,
    ) -> Result<(), AppError>;
    async fn delete_session(&self, session_hash: &str) -> Result<(), AppError>;
    async fn acquire_slot(&self, key: &str, max: i32, ttl: Duration) -> Result<bool, AppError>;
    async fn release_slot(&self, key: &str);
    /// 读取当前并发槽位占用数（实时并发展示用）。
    async fn get_slot_count(&self, key: &str) -> i64;

    /// 尝试把一个会话(x-claude-code-session-id)登记到账号的活跃会话集合。
    /// force=true(已粘性绑定的老会话)或 max<=0(不限)时总是成功;否则:已存在→刷新成功,
    /// 未满(< max)→登记成功,已满→false。ttl 内无新请求的会话视为过期、自动腾位。
    async fn session_admit(
        &self,
        account_id: i64,
        session_id: &str,
        max: i32,
        ttl: Duration,
        force: bool,
    ) -> bool;

    /// 账号当前活跃会话数(ttl 内有请求的不同 session_id 数)。
    async fn session_count(&self, account_id: i64, ttl: Duration) -> i64;
    async fn acquire_lock(
        &self,
        key: &str,
        owner: &str,
        ttl: Duration,
    ) -> Result<bool, AppError>;
    async fn release_lock(&self, key: &str, owner: &str);

    // --- RPM (Requests Per Minute) ---

    /// 递增账号当前分钟 RPM 计数，返回递增后的值。
    async fn incr_rpm(&self, account_id: i64) -> Result<i64, AppError>;
    /// 获取账号当前分钟 RPM 计数。
    async fn get_rpm(&self, account_id: i64) -> Result<i64, AppError>;
    /// 批量获取多个账号的当前分钟 RPM 计数。
    async fn get_rpm_batch(&self, account_ids: &[i64]) -> Result<HashMap<i64, i64>, AppError>;
}

/// 生成 RPM 缓存 key：rpm:{account_id}:{minute_timestamp}
pub fn rpm_key(account_id: i64) -> String {
    let minute_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 60;
    format!("rpm:{}:{}", account_id, minute_ts)
}

/// RPM key 的 TTL（120 秒，覆盖当前分钟窗口 + 缓冲）。
pub const RPM_TTL: Duration = Duration::from_secs(120);
