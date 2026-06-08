use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod optional_timestamp_millis {
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(
        value: &Option<DateTime<Utc>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(dt) => serializer.serialize_i64(dt.timestamp_millis()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<i64>::deserialize(deserializer)?;
        value
            .map(|ms| {
                Utc.timestamp_millis_opt(ms)
                    .single()
                    .ok_or_else(|| serde::de::Error::custom("invalid timestamp millis"))
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Active,
    Error,
    Disabled,
}

impl Default for AccountStatus {
    fn default() -> Self {
        Self::Active
    }
}

impl std::fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Error => write!(f, "error"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

impl From<String> for AccountStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "active" => Self::Active,
            "error" => Self::Error,
            "disabled" => Self::Disabled,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BillingMode {
    Strip,
    Rewrite,
}

impl Default for BillingMode {
    fn default() -> Self {
        Self::Strip
    }
}

impl std::fmt::Display for BillingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Strip => write!(f, "strip"),
            Self::Rewrite => write!(f, "rewrite"),
        }
    }
}

impl From<String> for BillingMode {
    fn from(s: String) -> Self {
        match s.as_str() {
            "rewrite" => Self::Rewrite,
            _ => Self::Strip,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountAuthType {
    SetupToken,
    Oauth,
}

impl Default for AccountAuthType {
    fn default() -> Self {
        Self::SetupToken
    }
}

impl std::fmt::Display for AccountAuthType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetupToken => write!(f, "setup_token"),
            Self::Oauth => write!(f, "oauth"),
        }
    }
}

impl From<String> for AccountAuthType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "oauth" => Self::Oauth,
            _ => Self::SetupToken,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub status: AccountStatus,
    #[serde(default)]
    pub auth_type: AccountAuthType,
    #[serde(default)]
    pub setup_token: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default, with = "optional_timestamp_millis")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_refreshed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub auth_error: String,
    #[serde(default)]
    pub proxy_url: String,
    pub device_id: String,
    pub canonical_env: Value,
    #[serde(rename = "canonical_prompt_env")]
    pub canonical_prompt: Value,
    pub canonical_process: Value,
    pub billing_mode: BillingMode,
    /// OAuth account UUID（强烈推荐填写，用于遥测改写）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_uuid: Option<String>,
    /// OAuth organization UUID（强烈推荐填写，用于遥测改写）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_uuid: Option<String>,
    /// 订阅类型：max / pro / team / enterprise（强烈推荐填写，用于遥测改写）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
    #[serde(default = "default_concurrency")]
    pub concurrency: i32,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limited_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_reset_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub disable_reason: String,
    /// 是否启用自动遥测。
    #[serde(default)]
    pub auto_telemetry: bool,
    /// 累计发送的遥测请求次数。
    #[serde(default)]
    pub telemetry_count: i64,
    /// 每分钟请求限制（0 或 None = 不限）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpm_limit: Option<i32>,
    #[serde(default)]
    pub usage_data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_fetched_at: Option<DateTime<Utc>>,
    /// 身份模式：passthrough（默认，原样透传）/ normalize（多人共号，归一化为虚拟身份）。
    #[serde(default)]
    pub identity_mode: String,
    /// normalize 模式下的虚拟用户名（home 目录名；留空则按账号自动派生）。
    #[serde(default)]
    pub virtual_user: String,
    /// normalize 模式下的虚拟 git 用户名（留空则按账号自动派生）。
    #[serde(default)]
    pub virtual_git_name: String,
    /// normalize 模式下的路径处理：空=回退全局默认 / "simulate"=改写真实路径用户名 / "passthrough"=真实路径原样透传。
    #[serde(default)]
    pub path_mode: String,
    /// session_id 归一化模式(账号级,仅 normalize+CC 下生效):
    /// ""/"off"=透传不归并(默认) / "pool"=3-4 槽位池 / "single"=全部坍缩成 1 个。
    /// 槽位池:真实会话哈希分流到 3-4 个虚拟 session,每槽 15-20min 轮换。详见 session_pool_size()。
    #[serde(default)]
    pub session_mode: String,
    /// 设备配额:每个 24h 固定窗口内该账号最多服务的不同(客户端真实)设备数;超过则新设备改选别的号,
    /// 到下一窗口清零重置。默认 10;<=0 不限。发往上游的 device_id 仍是账号固定虚拟值,此配额只控"承接面"。
    #[serde(default = "default_device_quota")]
    pub device_quota: i32,
    /// 会话配额:每个 24h 固定窗口内该账号最多服务的不同会话(session_id)数;超过则新会话改选别的号,
    /// 到下一窗口清零重置。默认 20;<=0 不限。与瞬时并发上限 max_sessions 叠加(双层保护)。
    #[serde(default = "default_session_quota")]
    pub session_quota: i32,
    /// 版本坐标(CC 版本/package/runtime)从首个真实请求吸取的时间；None=尚未吸取。
    #[serde(default)]
    pub identity_captured_at: Option<DateTime<Utc>>,
    /// normalize 模式下当前对上游呈现的 3-4 个虚拟 session(逗号分隔,每槽 15-20min 轮换)。展示用。
    #[serde(default)]
    pub captured_session_id: String,
    /// 上面这些 session 最近一次轮换的时间。
    #[serde(default)]
    pub captured_session_at: Option<DateTime<Utc>>,
    /// 重新吸取版本坐标的周期(天)；0=永久(只吸一次)。
    #[serde(default)]
    pub recapture_days: i64,
    /// 该账号允许的最大并发会话数(不同 x-claude-code-session-id)；0=不限。
    #[serde(default = "default_max_sessions")]
    pub max_sessions: i32,
    /// 允许的客户端类型(逗号分隔: cli/vscode/sdk/desktop/other)；空=全部放行(默认)。
    #[serde(default)]
    pub allowed_client_types: String,
    /// 5 小时滚动窗口的最大消费(USD,按官方价格表算)；0 或 None = 不限。
    /// 触发后该账号自动跳过(其他号顶上),5h 窗口滚走后自动恢复。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_5h_cost_cap_usd: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_concurrency() -> i32 { 3 }
fn default_priority() -> i32 { 50 }
fn default_max_sessions() -> i32 { 3 }
fn default_device_quota() -> i32 { 10 }
fn default_session_quota() -> i32 { 20 }

impl Account {
    /// 是否启用 normalize 身份归一化。
    pub fn identity_normalize(&self) -> bool {
        self.identity_mode == "normalize"
    }

    /// normalize 模式下是否透传真实文件系统路径(不改写 home用户名/cwd/memory slug/Windows 路径)。
    /// 账号自身 path_mode 优先；留空时回退全局默认 `global_passthrough`。
    pub fn effective_path_passthrough(&self, global_passthrough: bool) -> bool {
        match self.path_mode.as_str() {
            "passthrough" => true,
            "simulate" => false,
            _ => global_passthrough,
        }
    }

    /// session_id 归一化的「虚拟 session 槽数」(账号级,仅 normalize+CC 下有意义):
    /// - "off"            → None(不归并,session_id 原样透传)
    /// - "single"         → Some(1)(所有并发会话全部坍缩成 1 个)
    /// - "" / "pool" / 其它 → Some(3~4)(默认:3-4 槽位池,按账号错开;像几个窗口的重度真人)
    /// 返回 None 表示该账号不做 session 归一化。
    pub fn session_pool_size(&self) -> Option<usize> {
        match self.session_mode.as_str() {
            "pool" => Some(3 + (self.id.unsigned_abs() % 2) as usize), // 3 或 4
            "single" => Some(1),
            _ => None, // ""/"off"/其它 → 透传不归并(默认)
        }
    }

    /// 该账号是否放行某客户端类型分组(cli/vscode/sdk/desktop/other)。空配置=全部放行。
    pub fn allows_client_type(&self, category: &str) -> bool {
        let s = self.allowed_client_types.trim();
        if s.is_empty() {
            return true;
        }
        s.split(',').map(|x| x.trim()).any(|x| x == category)
    }

    /// 是否需要(重新)从客户端吸取版本坐标：从未吸过,或周期>0 且已超期。
    pub fn needs_identity_capture(&self) -> bool {
        match self.identity_captured_at {
            None => true,
            Some(at) => {
                self.recapture_days > 0
                    && (Utc::now() - at).num_days() >= self.recapture_days
            }
        }
    }

    /// 该账号生效的虚拟身份 (虚拟用户名, 虚拟 git 名)：优先自定义值，留空则按账号稳定派生。
    pub fn effective_virtual_identity(&self) -> (String, String) {
        let seed = if self.email.is_empty() {
            self.id.to_string()
        } else {
            self.email.clone()
        };
        let derived = crate::model::identity::virtual_identity(&seed);
        let user = if self.virtual_user.trim().is_empty() {
            derived.user
        } else {
            self.virtual_user.trim().to_string()
        };
        let git = if self.virtual_git_name.trim().is_empty() {
            derived.git_name
        } else {
            self.virtual_git_name.trim().to_string()
        };
        (user, git)
    }

    pub fn is_schedulable(&self) -> bool {
        if self.status != AccountStatus::Active {
            return false;
        }
        if let Some(reset) = self.rate_limit_reset_at {
            if Utc::now() < reset {
                return false;
            }
        }
        true
    }

    pub fn has_valid_oauth_access_token(&self, buffer_seconds: i64) -> bool {
        if self.auth_type != AccountAuthType::Oauth || self.access_token.is_empty() {
            return false;
        }
        self.expires_at
            .map(|expires_at| expires_at > Utc::now() + chrono::Duration::seconds(buffer_seconds))
            .unwrap_or(false)
    }
}

/// 存储 20+ 维度的环境指纹数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CanonicalEnvData {
    pub platform: String,
    pub platform_raw: String,
    pub arch: String,
    pub node_version: String,
    /// x-stainless-package-version(Anthropic SDK 版本)；首请求吸取,留空则用全局默认。
    #[serde(default)]
    pub package_version: String,
    pub terminal: String,
    pub package_managers: String,
    pub runtimes: String,
    #[serde(default)]
    pub is_running_with_bun: bool,
    #[serde(default)]
    pub is_ci: bool,
    #[serde(default)]
    pub is_claubbit: bool,
    #[serde(default)]
    pub is_claude_code_remote: bool,
    #[serde(default)]
    pub is_local_agent_mode: bool,
    #[serde(default)]
    pub is_conductor: bool,
    #[serde(default)]
    pub is_github_action: bool,
    #[serde(default)]
    pub is_claude_code_action: bool,
    #[serde(default)]
    pub is_claude_ai_auth: bool,
    pub version: String,
    pub version_base: String,
    pub build_time: String,
    pub deployment_environment: String,
    pub vcs: String,
}

/// 系统提示词中的环境改写数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CanonicalPromptEnvData {
    pub platform: String,
    pub shell: String,
    pub os_version: String,
    pub working_dir: String,
}

/// 硬件指纹配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CanonicalProcessData {
    pub constrained_memory: i64,
    pub rss_range: [i64; 2],
    pub heap_total_range: [i64; 2],
    pub heap_used_range: [i64; 2],
}
