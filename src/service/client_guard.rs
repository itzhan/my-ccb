//! 客户端限制：校验请求是否来自真实 Claude Code 客户端。
//! 移植自 sub2api / claude-relay-service 的 ClaudeCodeValidator：
//! User-Agent 正则 + 系统提示词 Dice 相似度 + 必需 header。

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

/// 客户端限制级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientRestriction {
    /// 不限制（默认，保留原有"伪装 API 客户端"能力）。
    Off,
    /// 仅校验 User-Agent。
    Ua,
    /// 仅交互式 Claude Code：UA 是 claude-cli/code 且客户端类型 ∈ {cli, claude-vscode}，
    /// 挡掉 Agent SDK 程序化调用（sdk-cli/sdk-ts/local-agent）和 desktop-3p。
    CliOnly,
    /// 严格：UA + 系统提示相似度 + 必需 header。
    Strict,
}

impl ClientRestriction {
    pub fn from_env(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "ua" => Self::Ua,
            "cli" => Self::CliOnly,
            "strict" => Self::Strict,
            _ => Self::Off,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Ua => "ua",
            Self::CliOnly => "cli",
            Self::Strict => "strict",
        }
    }
}

/// User-Agent 匹配 `claude-code/x.x.x` 或 `claude-cli/x.x.x`（大小写不敏感）。
static UA_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^claude-(code|cli)/\d+\.\d+\.\d+").unwrap());

/// 从 `(external, <type>, ...)` 提取客户端类型 token，如 cli/sdk-cli/claude-vscode。
static CLIENT_TYPE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\(external,\s*([a-z0-9.\-]+)").unwrap());

/// CliOnly 模式放行的交互式 Claude Code 客户端类型。
const CLI_ALLOWED_TYPES: &[&str] = &["cli", "claude-vscode"];

/// 取 UA 里 `(external, <type>)` 的类型 token（小写）；取不到返回 None。
pub fn ua_client_type(ua: &str) -> Option<String> {
    CLIENT_TYPE_PATTERN
        .captures(ua)
        .map(|c| c[1].to_ascii_lowercase())
}

/// 把 UA 归到客户端类型分组：cli / vscode / sdk / desktop / other（账号级放行用）。
pub fn client_type_category(ua: &str) -> &'static str {
    match ua_client_type(ua).as_deref() {
        Some("cli") => "cli",
        Some("claude-vscode") | Some("vscode") => "vscode",
        Some("sdk-cli") | Some("sdk-ts") | Some("local-agent") => "sdk",
        Some(t) if t.starts_with("claude-desktop") => "desktop",
        _ => "other",
    }
}

/// 系统提示相似度阈值（与 claude-relay-service 一致）。
const SIMILARITY_THRESHOLD: f64 = 0.5;

/// Claude Code 官方系统提示词模板。
const SYSTEM_PROMPTS: &[&str] = &[
    "You are Claude Code, Anthropic's official CLI for Claude.",
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.",
    "You are Claude Code, Anthropic's official CLI for Claude, running within the Claude Agent SDK.",
    "You are a file search specialist for Claude Code, Anthropic's official CLI for Claude.",
    "You are a helpful AI assistant tasked with summarizing conversations.",
    "You are an interactive CLI tool that helps users",
];

pub fn ua_is_claude_code(ua: &str) -> bool {
    UA_PATTERN.is_match(ua)
}

/// 大小写不敏感地取 header（extract_headers 的 key 一般为小写，做个兜底）。
fn header<'a>(headers: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    if let Some(v) = headers.get(name) {
        return Some(v.as_str());
    }
    let lower = name.to_ascii_lowercase();
    headers
        .iter()
        .find(|(k, _)| k.to_ascii_lowercase() == lower)
        .map(|(_, v)| v.as_str())
}

/// 校验请求是否允许通过。
pub fn validate(
    mode: ClientRestriction,
    path: &str,
    headers: &HashMap<String, String>,
    body: &Value,
) -> bool {
    if mode == ClientRestriction::Off {
        return true;
    }

    // Step 1: User-Agent 必须是官方 CLI
    let ua = header(headers, "user-agent").unwrap_or("");
    if !ua_is_claude_code(ua) {
        return false;
    }

    if mode == ClientRestriction::Ua {
        return true;
    }

    // CliOnly: 进一步要求客户端类型是交互式 Claude Code（cli/vscode），挡 SDK 自动化
    if mode == ClientRestriction::CliOnly {
        return match ua_client_type(ua) {
            Some(t) => CLI_ALLOWED_TYPES.contains(&t.as_str()),
            None => false,
        };
    }

    // Step 2（Strict）：非 messages 路径仅校验 UA
    if !path.contains("messages") {
        return true;
    }
    // count_tokens 是官方辅助请求，不携带完整 system prompt
    if path.ends_with("/messages/count_tokens") {
        return true;
    }

    // Step 3（Strict）：系统提示相似度
    if !has_claude_code_system_prompt(body) {
        return false;
    }
    // Step 4（Strict）：必需 header 非空
    if header(headers, "x-app").unwrap_or("").is_empty() {
        return false;
    }
    if header(headers, "anthropic-beta").unwrap_or("").is_empty() {
        return false;
    }
    if header(headers, "anthropic-version").unwrap_or("").is_empty() {
        return false;
    }
    // Step 5（Strict）：metadata.user_id 必须存在
    let user_id = body
        .get("metadata")
        .and_then(|m| m.get("user_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    !user_id.is_empty()
}

fn has_claude_code_system_prompt(body: &Value) -> bool {
    if !body.get("model").map(|m| m.is_string()).unwrap_or(false) {
        return false;
    }
    let entries = match body.get("system").and_then(|s| s.as_array()) {
        Some(e) => e,
        None => return false,
    };
    for entry in entries {
        let text = entry.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if text.is_empty() {
            continue;
        }
        if best_similarity(text) >= SIMILARITY_THRESHOLD {
            return true;
        }
    }
    false
}

fn best_similarity(text: &str) -> f64 {
    let norm = normalize(text);
    SYSTEM_PROMPTS
        .iter()
        .map(|t| dice_coefficient(&norm, &normalize(t)))
        .fold(0.0, f64::max)
}

/// 把所有空白折叠为单个空格并去除首尾空白。
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sørensen–Dice 系数：2 * |交集| / (|bigrams(a)| + |bigrams(b)|)。
fn dice_coefficient(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let ra: Vec<char> = a.to_lowercase().chars().collect();
    let rb: Vec<char> = b.to_lowercase().chars().collect();
    if ra.len() < 2 || rb.len() < 2 {
        return 0.0;
    }
    let bigrams_a = bigrams(&ra);
    let bigrams_b = bigrams(&rb);
    let mut intersection = 0usize;
    for (bg, &ca) in &bigrams_a {
        if let Some(&cb) = bigrams_b.get(bg) {
            intersection += ca.min(cb);
        }
    }
    let total_a: usize = bigrams_a.values().sum();
    let total_b: usize = bigrams_b.values().sum();
    if total_a + total_b == 0 {
        return 0.0;
    }
    (2 * intersection) as f64 / (total_a + total_b) as f64
}

fn bigrams(runes: &[char]) -> HashMap<(char, char), usize> {
    let mut m = HashMap::new();
    for w in runes.windows(2) {
        *m.entry((w[0], w[1])).or_insert(0) += 1;
    }
    m
}
