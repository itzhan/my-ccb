use serde::Serialize;

/// 一条调用用量记录（写入 usage_logs 的载荷）。
#[derive(Debug, Clone, Default)]
pub struct UsageRecord {
    pub token_id: i64,
    pub account_id: i64,
    pub request_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_5m_tokens: i64,
    pub cache_creation_1h_tokens: i64,
    pub stream: bool,
    pub status_code: i32,
    pub duration_ms: i64,
    /// 失败请求(非 2xx)的上游错误正文(截断)；成功为空。
    pub error: String,
}

impl UsageRecord {
    /// 是否有可记录的用量（全 0 则不落库）。
    pub fn has_usage(&self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_creation_tokens > 0
            || self.cache_read_tokens > 0
    }
}

/// 查询返回的明细行。
#[derive(Debug, Clone, Serialize)]
pub struct UsageLogRow {
    pub id: i64,
    pub token_id: i64,
    pub account_id: i64,
    pub request_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_5m_tokens: i64,
    pub cache_creation_1h_tokens: i64,
    pub stream: bool,
    pub status_code: i64,
    pub duration_ms: i64,
    pub error: String,
    pub created_at: String,
}

/// 聚合统计行（按 group_by 维度求和）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct UsageStatRow {
    /// 分组键（按 token=token_id，按 account=account_id，按 model=model 名，按 day=日期）
    pub key: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub req_count: i64,
}
