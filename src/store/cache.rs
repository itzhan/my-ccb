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
