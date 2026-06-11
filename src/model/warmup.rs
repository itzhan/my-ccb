use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 养号任务：批量驱动多个 warmup 令牌对应的 Claude Code 客户端持续交互。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarmupTask {
    pub id: i64,
    pub name: String,
    /// 逗号分隔的 warmup 令牌 ID（批量养号对象，每个 token 对应一个账号）。
    pub token_ids: String,
    /// 单条消息之间的间隔秒数。
    pub msg_interval_secs: i64,
    /// 任务总运行时长（秒）。
    pub total_duration_secs: i64,
    /// 大间隔：连续工作满 work 秒后长休 rest 秒，循环；0 表示不休息。
    pub work_duration_secs: i64,
    pub rest_duration_secs: i64,
    /// 间隔抖动百分比（0-100），让节奏更像真人；0 表示不抖动。
    pub jitter_pct: i64,
    /// 单个对话最大轮数,达到后发 /clear 开新对话(控制上下文/消费);0 表示不限、永不清。
    pub max_turns: i64,
    /// 可选模型别名（如 opus / sonnet），空表示用账号默认。
    pub model: String,
    pub status: WarmupStatus,
    /// 最近一次错误信息（启动失败等）。
    pub error: String,
    /// 已发送消息计数。
    pub messages_sent: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WarmupStatus {
    /// 已创建，未启动。
    Pending,
    /// 运行中。
    Running,
    /// 到时正常结束。
    Completed,
    /// 用户手动停止。
    Stopped,
    /// 启动/运行出错。
    Error,
}

impl std::fmt::Display for WarmupStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Stopped => write!(f, "stopped"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl From<String> for WarmupStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "stopped" => Self::Stopped,
            "error" => Self::Error,
            _ => Self::Pending,
        }
    }
}

impl WarmupTask {
    /// 解析 warmup 令牌 ID 列表。
    pub fn token_id_list(&self) -> Vec<i64> {
        if self.token_ids.is_empty() {
            return vec![];
        }
        self.token_ids
            .split(',')
            .filter_map(|id| id.trim().parse::<i64>().ok())
            .collect()
    }
}
