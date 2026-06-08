use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::store::cache::{rpm_key, CacheStore, RPM_TTL};

struct SessionEntry {
    account_id: i64,
    expires_at: tokio::time::Instant,
}

struct LockEntry {
    owner: String,
    expires_at: tokio::time::Instant,
}

struct CounterEntry {
    count: i64,
    expires_at: tokio::time::Instant,
}

pub struct MemoryStore {
    sessions: Mutex<HashMap<String, SessionEntry>>,
    slots: Mutex<HashMap<String, i64>>,
    locks: Mutex<HashMap<String, LockEntry>>,
    counters: Mutex<HashMap<String, CounterEntry>>,
    /// account_id -> (session_id -> 最后活动时间)
    acct_sessions: Mutex<HashMap<i64, HashMap<String, tokio::time::Instant>>>,
    /// 配额(固定窗口):account_id -> (device_id -> 该成员所属窗口编号 win=epoch/window_secs)
    acct_quota_devices: Mutex<HashMap<i64, HashMap<String, u64>>>,
    /// 配额(固定窗口):account_id -> (session_id -> 窗口编号)
    acct_quota_sessions: Mutex<HashMap<i64, HashMap<String, u64>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            slots: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            counters: Mutex::new(HashMap::new()),
            acct_sessions: Mutex::new(HashMap::new()),
            acct_quota_devices: Mutex::new(HashMap::new()),
            acct_quota_sessions: Mutex::new(HashMap::new()),
        }
    }
}

#[axum::async_trait]
impl CacheStore for MemoryStore {
    async fn get_session_account_id(&self, session_hash: &str) -> Result<Option<i64>, AppError> {
        let mut sessions = self.sessions.lock().await;
        let key = format!("session:{}", session_hash);
        if let Some(entry) = sessions.get(&key) {
            if tokio::time::Instant::now() > entry.expires_at {
                sessions.remove(&key);
                return Ok(None);
            }
            return Ok(Some(entry.account_id));
        }
        Ok(None)
    }

    async fn set_session_account_id(
        &self,
        session_hash: &str,
        account_id: i64,
        ttl: Duration,
    ) -> Result<(), AppError> {
        let mut sessions = self.sessions.lock().await;
        let key = format!("session:{}", session_hash);
        sessions.insert(
            key,
            SessionEntry {
                account_id,
                expires_at: tokio::time::Instant::now() + ttl,
            },
        );
        Ok(())
    }

    async fn delete_session(&self, session_hash: &str) -> Result<(), AppError> {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(&format!("session:{}", session_hash));
        Ok(())
    }

    async fn acquire_slot(&self, key: &str, max: i32, _ttl: Duration) -> Result<bool, AppError> {
        let mut slots = self.slots.lock().await;
        let val = slots.entry(key.to_string()).or_insert(0);
        *val += 1;
        if *val > max as i64 {
            *val -= 1;
            return Ok(false);
        }
        Ok(true)
    }

    async fn release_slot(&self, key: &str) {
        let mut slots = self.slots.lock().await;
        if let Some(val) = slots.get_mut(key) {
            if *val > 0 {
                *val -= 1;
            }
        }
    }

    async fn get_slot_count(&self, key: &str) -> i64 {
        let slots = self.slots.lock().await;
        slots.get(key).copied().unwrap_or(0).max(0)
    }

    async fn session_admit(
        &self,
        account_id: i64,
        session_id: &str,
        max: i32,
        ttl: Duration,
        force: bool,
    ) -> bool {
        let now = tokio::time::Instant::now();
        let mut map = self.acct_sessions.lock().await;
        let set = map.entry(account_id).or_default();
        // 清理过期会话
        set.retain(|_, last| now.duration_since(*last) < ttl);
        if force || max <= 0 || set.contains_key(session_id) || (set.len() as i32) < max {
            set.insert(session_id.to_string(), now);
            true
        } else {
            false
        }
    }

    async fn session_count(&self, account_id: i64, ttl: Duration) -> i64 {
        let now = tokio::time::Instant::now();
        let map = self.acct_sessions.lock().await;
        match map.get(&account_id) {
            Some(set) => set
                .values()
                .filter(|last| now.duration_since(**last) < ttl)
                .count() as i64,
            None => 0,
        }
    }

    async fn account_quota_admit(
        &self,
        account_id: i64,
        device_id: &str,
        session_id: &str,
        max_devices: i32,
        max_sessions: i32,
        ttl: Duration,
        force: bool,
    ) -> bool {
        // 固定窗口编号:win = epoch_secs / window_secs(window_secs=ttl,即按日历日对齐,到点整体清零)。
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let window = ttl.as_secs().max(1);
        // 固定窗口对齐北京时间(UTC+8)0 点:加 8h 偏移再整除,窗口在北京 0 点滚动重置。
        let win = (now_secs + 8 * 3600) / window;
        // 始终先锁 devices 再锁 sessions,避免与其它路径产生死锁。
        let mut devmap = self.acct_quota_devices.lock().await;
        let mut sessmap = self.acct_quota_sessions.lock().await;
        let dset = devmap.entry(account_id).or_default();
        let sset = sessmap.entry(account_id).or_default();
        // 跨窗口:上一窗口的记录整体作废(固定窗口,不是滑动)。
        dset.retain(|_, w| *w == win);
        sset.retain(|_, w| *w == win);
        let dev_ok = force
            || device_id.is_empty()
            || max_devices <= 0
            || dset.contains_key(device_id)
            || (dset.len() as i32) < max_devices;
        let sess_ok = force
            || session_id.is_empty()
            || max_sessions <= 0
            || sset.contains_key(session_id)
            || (sset.len() as i32) < max_sessions;
        if dev_ok && sess_ok {
            if !device_id.is_empty() {
                dset.insert(device_id.to_string(), win);
            }
            if !session_id.is_empty() {
                sset.insert(session_id.to_string(), win);
            }
            true
        } else {
            false
        }
    }

    async fn quota_counts(&self, account_id: i64, ttl: Duration) -> (i64, i64) {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let win = (now_secs + 8 * 3600) / ttl.as_secs().max(1); // 北京时间(UTC+8)对齐
        let cnt = |map: &HashMap<i64, HashMap<String, u64>>| -> i64 {
            map.get(&account_id)
                .map(|s| s.values().filter(|w| **w == win).count() as i64)
                .unwrap_or(0)
        };
        let d = cnt(&*self.acct_quota_devices.lock().await);
        let s = cnt(&*self.acct_quota_sessions.lock().await);
        (d, s)
    }

    async fn acquire_lock(
        &self,
        key: &str,
        owner: &str,
        ttl: Duration,
    ) -> Result<bool, AppError> {
        let mut locks = self.locks.lock().await;
        let now = tokio::time::Instant::now();
        if let Some(existing) = locks.get(key) {
            if now <= existing.expires_at {
                return Ok(false);
            }
        }
        locks.insert(
            key.to_string(),
            LockEntry {
                owner: owner.to_string(),
                expires_at: now + ttl,
            },
        );
        Ok(true)
    }

    async fn release_lock(&self, key: &str, owner: &str) {
        let mut locks = self.locks.lock().await;
        if let Some(existing) = locks.get(key) {
            if existing.owner == owner {
                locks.remove(key);
            }
        }
    }

    async fn incr_rpm(&self, account_id: i64) -> Result<i64, AppError> {
        let key = rpm_key(account_id);
        let mut counters = self.counters.lock().await;
        let now = tokio::time::Instant::now();
        let entry = counters.entry(key).or_insert(CounterEntry {
            count: 0,
            expires_at: now + RPM_TTL,
        });
        if now > entry.expires_at {
            entry.count = 0;
            entry.expires_at = now + RPM_TTL;
        }
        entry.count += 1;
        Ok(entry.count)
    }

    async fn reserve_rpm(&self, account_id: i64, limit: i64) -> Result<bool, AppError> {
        let key = rpm_key(account_id);
        let mut counters = self.counters.lock().await;
        let now = tokio::time::Instant::now();
        let entry = counters.entry(key).or_insert(CounterEntry {
            count: 0,
            expires_at: now + RPM_TTL,
        });
        if now > entry.expires_at {
            entry.count = 0;
            entry.expires_at = now + RPM_TTL;
        }
        if limit > 0 && entry.count >= limit {
            return Ok(false);
        }
        entry.count += 1;
        Ok(true)
    }

    async fn get_rpm(&self, account_id: i64) -> Result<i64, AppError> {
        let key = rpm_key(account_id);
        let mut counters = self.counters.lock().await;
        let now = tokio::time::Instant::now();
        if let Some(entry) = counters.get(&key) {
            if now <= entry.expires_at {
                return Ok(entry.count);
            }
            counters.remove(&key);
        }
        Ok(0)
    }

    async fn get_rpm_batch(&self, account_ids: &[i64]) -> Result<HashMap<i64, i64>, AppError> {
        let mut result = HashMap::new();
        let mut counters = self.counters.lock().await;
        let now = tokio::time::Instant::now();
        for &id in account_ids {
            let key = rpm_key(id);
            if let Some(entry) = counters.get(&key) {
                if now <= entry.expires_at {
                    result.insert(id, entry.count);
                } else {
                    counters.remove(&key);
                    result.insert(id, 0);
                }
            } else {
                result.insert(id, 0);
            }
        }
        Ok(result)
    }
}
