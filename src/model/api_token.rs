use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: i64,
    pub name: String,
    pub token: String,
    /// 逗号分隔的可用账号 ID（空字符串表示不限制）
    pub allowed_accounts: String,
    /// 逗号分隔的不可用账号 ID（空字符串表示不限制）
    pub blocked_accounts: String,
    pub status: ApiTokenStatus,
    /// 令牌分类：customer（默认，客户用）/ warmup（养号专用，一个 key 绑一个账号）。
    pub category: ApiTokenCategory,
    /// 该令牌允许的最大并发请求数（0 表示不限制）。
    pub concurrency: i32,
    /// 令牌过期时间（None 表示永不过期）。
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiTokenStatus {
    Active,
    Disabled,
}

impl std::fmt::Display for ApiTokenStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

impl From<String> for ApiTokenStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "active" => Self::Active,
            _ => Self::Disabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiTokenCategory {
    Customer,
    Warmup,
}

impl std::fmt::Display for ApiTokenCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Customer => write!(f, "customer"),
            Self::Warmup => write!(f, "warmup"),
        }
    }
}

impl From<String> for ApiTokenCategory {
    fn from(s: String) -> Self {
        match s.as_str() {
            "warmup" => Self::Warmup,
            _ => Self::Customer,
        }
    }
}

impl ApiToken {
    /// 解析可用账号 ID 列表
    pub fn allowed_account_ids(&self) -> Vec<i64> {
        parse_id_list(&self.allowed_accounts)
    }

    /// 解析不可用账号 ID 列表
    pub fn blocked_account_ids(&self) -> Vec<i64> {
        parse_id_list(&self.blocked_accounts)
    }

    /// 是否已过期。
    pub fn is_expired(&self) -> bool {
        self.expires_at.map(|e| e < Utc::now()).unwrap_or(false)
    }
}

fn parse_id_list(s: &str) -> Vec<i64> {
    if s.is_empty() {
        return vec![];
    }
    s.split(',')
        .filter_map(|id| id.trim().parse::<i64>().ok())
        .collect()
}

/// 生成 sk-ant- 开头的令牌（sk-ant- + 57 位随机字符）。
pub fn generate_token() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random: String = (0..57)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect();
    format!("sk-ant-{}", random)
}
