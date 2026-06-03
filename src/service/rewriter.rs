use base64::Engine;
use once_cell::sync::Lazy;
use rand::Rng;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::model::account::{
    Account, BillingMode, CanonicalEnvData, CanonicalProcessData, CanonicalPromptEnvData,
};

/// header wire 大小写映射。
/// Go 的 HTTP 服务器规范化 header，此映射还原 Claude CLI 抓包原始大小写。
static HEADER_WIRE_CASING: Lazy<HashMap<&str, &str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("accept", "Accept");
    m.insert("user-agent", "User-Agent");
    m.insert("x-stainless-retry-count", "X-Stainless-Retry-Count");
    m.insert("x-stainless-timeout", "X-Stainless-Timeout");
    m.insert("x-stainless-lang", "X-Stainless-Lang");
    m.insert("x-stainless-package-version", "X-Stainless-Package-Version");
    m.insert("x-stainless-os", "X-Stainless-OS");
    m.insert("x-stainless-arch", "X-Stainless-Arch");
    m.insert("x-stainless-runtime", "X-Stainless-Runtime");
    m.insert("x-stainless-runtime-version", "X-Stainless-Runtime-Version");
    m.insert("x-stainless-helper-method", "x-stainless-helper-method");
    m.insert(
        "anthropic-dangerous-direct-browser-access",
        "anthropic-dangerous-direct-browser-access",
    );
    m.insert("anthropic-version", "anthropic-version");
    m.insert("anthropic-beta", "anthropic-beta");
    m.insert("x-app", "x-app");
    m.insert("content-type", "content-type");
    m.insert("accept-language", "accept-language");
    m.insert("sec-fetch-mode", "sec-fetch-mode");
    m.insert("accept-encoding", "accept-encoding");
    m.insert("authorization", "authorization");
    m.insert("x-claude-code-session-id", "X-Claude-Code-Session-Id");
    m.insert("x-client-request-id", "x-client-request-id");
    m.insert("content-length", "content-length");
    m
});

/// 将规范化 key 转换为真实 wire 大小写。
fn resolve_wire_casing(key: &str) -> String {
    let lower = key.to_lowercase();
    if let Some(wk) = HEADER_WIRE_CASING.get(lower.as_str()) {
        wk.to_string()
    } else {
        key.to_string()
    }
}

/// 请求来源类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientType {
    ClaudeCode,
    API,
}

const DEFAULT_VERSION: &str = "2.1.156";
/// 与 DEFAULT_VERSION 对应的 Anthropic SDK 版本（x-stainless-package-version）。
const STAINLESS_PACKAGE_VERSION: &str = "0.94.0";
const DEFAULT_RUNTIME_VERSION: &str = "v24.3.0";

/// 从客户端首请求吸取的「版本坐标」(只吸必要的版本三项,不碰 OS/device 等通用部分)。
pub struct CapturedCoords {
    pub cc_version: String,
    pub package_version: String,
    pub runtime_version: String,
}

/// 从 User-Agent 解析 CC 版本号(claude-cli/X.Y.Z … 或 claude-code/X.Y.Z)。
fn parse_cli_version(ua: &str) -> String {
    let rest = ua
        .strip_prefix("claude-cli/")
        .or_else(|| ua.strip_prefix("claude-code/"));
    match rest {
        Some(r) => r
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect(),
        None => String::new(),
    }
}

/// 从请求头组装要吸取的版本坐标。
pub fn extract_captured_coords(
    ua: &str,
    package_version: &str,
    runtime_version: &str,
) -> CapturedCoords {
    CapturedCoords {
        cc_version: parse_cli_version(ua),
        package_version: package_version.to_string(),
        runtime_version: runtime_version.to_string(),
    }
}

/// 三级兜底取第一个非空。
fn pick3(a: &str, b: &str, c: &str) -> String {
    if !a.is_empty() {
        a.to_string()
    } else if !b.is_empty() {
        b.to_string()
    } else {
        c.to_string()
    }
}

/// 根据模型返回正确的 anthropic-beta 值。
fn beta_header_for_model(model_id: &str) -> &'static str {
    // 取自真实 Claude Code 2.1.156 抓包（POST /v1/messages?beta=true）。
    // 注意：真实 CC 不发送 oauth-2025-04-20。
    let lower = model_id.to_lowercase();
    if lower.contains("haiku") {
        // haiku 辅助请求用较小集合（不含 1M 上下文等重特性）
        "claude-code-20250219,interleaved-thinking-2025-05-14"
    } else {
        // 去掉 context-1m-2025-08-07：1M 长上下文是计费功能,订阅号无额度会 429
        // "Usage credits are required for long context requests"。去掉后 CC 视窗口为 20 万,
        // 长对话自动压缩、不再卡死(与 sub2api 模板一致)。
        "claude-code-20250219,interleaved-thinking-2025-05-14,thinking-token-count-2026-05-13,context-management-2025-06-27,prompt-caching-scope-2026-01-05,mid-conversation-system-2026-04-07,advisor-tool-2026-03-01,effort-2025-11-24"
    }
}

/// 处理所有请求的反检测改写。
pub struct Rewriter;

impl Rewriter {
    pub fn new() -> Self {
        Self
    }

    // --- Header 改写 ---

    /// 处理出站 header 的反检测改写。
    pub fn rewrite_headers(
        &self,
        headers: &HashMap<String, String>,
        account: &Account,
        client_type: ClientType,
        model_id: &str,
        body_map: &serde_json::Value,
    ) -> HashMap<String, String> {
        let env = self.parse_env(account);
        let version = if env.version.is_empty() {
            DEFAULT_VERSION
        } else {
            &env.version
        };

        let mut out = HashMap::new();

        if client_type == ClientType::API {
            // API 模式：使用与真实 Claude CLI 匹配的固定 header 集合。
            out.insert("Accept".into(), "application/json".into());
            out.insert(
                "User-Agent".into(),
                format!("claude-cli/{} (external, sdk-cli)", version),
            );
            out.insert(
                "anthropic-beta".into(),
                beta_header_for_model(model_id).into(),
            );
            out.insert("anthropic-version".into(), "2023-06-01".into());
            out.insert(
                "anthropic-dangerous-direct-browser-access".into(),
                "true".into(),
            );
            out.insert("x-app".into(), "cli".into());
            out.insert("content-type".into(), "application/json".into());
            out.insert(
                "accept-encoding".into(),
                "gzip, deflate, br, zstd".into(),
            );
            let stainless_os = stainless_os_from_platform(&env.platform);
            out.insert("X-Stainless-Lang".into(), "js".into());
            out.insert("X-Stainless-Package-Version".into(), "0.94.0".into());
            out.insert("X-Stainless-OS".into(), stainless_os.into());
            out.insert("X-Stainless-Arch".into(), env.arch.clone());
            out.insert("X-Stainless-Runtime".into(), "node".into());
            out.insert(
                "X-Stainless-Runtime-Version".into(),
                env.node_version.clone(),
            );
            out.insert("X-Stainless-Retry-Count".into(), "0".into());
            out.insert("X-Stainless-Timeout".into(), "600".into());

            let session_id = extract_session_id_from_body(body_map)
                .unwrap_or_else(generate_session_uuid);
            out.insert("X-Claude-Code-Session-Id".into(), session_id);
        } else {
            // CC 客户端模式：白名单 + 改写
            let allowed: std::collections::HashSet<&str> = [
                "accept",
                "user-agent",
                "content-type",
                "accept-encoding",
                "accept-language",
                "anthropic-beta",
                "anthropic-version",
                "anthropic-dangerous-direct-browser-access",
                "x-app",
                "sec-fetch-mode",
                "x-stainless-retry-count",
                "x-stainless-timeout",
                "x-stainless-lang",
                "x-stainless-package-version",
                "x-stainless-os",
                "x-stainless-arch",
                "x-stainless-runtime",
                "x-stainless-runtime-version",
                "x-stainless-helper-method",
                "x-claude-code-session-id",
                "x-client-request-id",
            ]
            .into_iter()
            .collect();

            for (k, v) in headers {
                let lower = k.to_lowercase();
                if !allowed.contains(lower.as_str()) {
                    continue;
                }
                // 真实 Claude Code 客户端发来的 header 本就正确（UA / anthropic-beta /
                // X-Stainless-* 都是当前版本的真实值），原样转发，不再用账号预设覆盖，
                // 避免改写引入与真实客户端的偏差（这正是被风控识别的根源）。
                out.insert(resolve_wire_casing(k), v.clone());
            }

            // 仅兜底：确保必需 header 存在（真实 CC 一般已携带）
            out.entry("anthropic-dangerous-direct-browser-access".into())
                .or_insert_with(|| "true".into());
        }

        out
    }

    // --- Body 改写 ---

    /// 根据端点和客户端类型改写请求体。
    pub fn rewrite_body(
        &self,
        body: &[u8],
        path: &str,
        account: &Account,
        client_type: ClientType,
    ) -> Vec<u8> {
        if body.is_empty() {
            return body.to_vec();
        }

        let mut parsed: serde_json::Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => return body.to_vec(), // 非 JSON，直接透传
        };

        if path.starts_with("/v1/messages") {
            strip_empty_text_blocks(&mut parsed);
            self.rewrite_messages(&mut parsed, account, client_type);
        } else if path.contains("/event_logging/batch") {
            self.rewrite_event_batch(&mut parsed, account);
        } else if path.starts_with("/api/eval/") {
            self.rewrite_growthbook_eval(&mut parsed, account);
        } else {
            self.rewrite_generic_identity(&mut parsed, account);
        }

        let mut output = serde_json::to_vec(&parsed).unwrap_or_else(|_| body.to_vec());

        // Rewrite 模式下对 /v1/messages 请求计算 cch attestation
        if path.starts_with("/v1/messages") && account.billing_mode == BillingMode::Rewrite {
            output = compute_cch_attestation(output);
        }

        output
    }

    /// 处理 /v1/messages 请求体。
    fn rewrite_messages(
        &self,
        body: &mut serde_json::Value,
        account: &Account,
        client_type: ClientType,
    ) {
        let env = self.parse_env(account);
        let prompt_env = self.parse_prompt_env(account);

        if client_type == ClientType::ClaudeCode {
            // 替换模式
            self.rewrite_metadata_user_id(body, account);
            self.rewrite_system_prompt(body, &prompt_env, &env.version, &account.billing_mode);
            scrub_git_user_in_reminders(body, &account.name);
        } else {
            // 注入模式
            let session_id = self.inject_metadata_user_id(body, account);
            if let Some(sid) = &session_id {
                if let Some(metadata) = body.get_mut("metadata").and_then(|m| m.as_object_mut()) {
                    metadata.insert(
                        "_session_id".into(),
                        serde_json::Value::String(sid.clone()),
                    );
                }
            }

            // 剥离 Claude Code 不会发送的字段
            if let Some(obj) = body.as_object_mut() {
                obj.remove("temperature");
                obj.remove("top_k");
                obj.remove("top_p");
                obj.remove("stop_sequences");
                obj.remove("tool_choice");

                // 确保 tools 字段存在
                obj.entry("tools")
                    .or_insert(serde_json::Value::Array(vec![]));

                // 确保 stream 为 true
                obj.insert("stream".into(), serde_json::Value::Bool(true));
            }

            // 剥离 system 块中的 cache_control
            strip_cache_control(body);

            // 规范化 max_tokens
            if let Some(max_tokens) = body.get("max_tokens").and_then(|v| v.as_f64()) {
                if max_tokens > 32768.0 {
                    body.as_object_mut()
                        .unwrap()
                        .insert("max_tokens".into(), serde_json::json!(16384));
                }
            }

            // 注入 Claude Code 系统提示词
            self.inject_system_prompt(body);
        }
    }

    /// 替换已有 metadata.user_id 中的 device_id（CC 客户端模式）。
    fn rewrite_metadata_user_id(&self, body: &mut serde_json::Value, account: &Account) {
        let user_id_str = {
            let metadata = match body.get("metadata").and_then(|m| m.as_object()) {
                Some(m) => m,
                None => return,
            };
            match metadata.get("user_id").and_then(|u| u.as_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => return,
            }
        };

        // 尝试 JSON 格式
        if let Ok(mut uid) = serde_json::from_str::<serde_json::Value>(&user_id_str) {
            if let Some(obj) = uid.as_object_mut() {
                obj.insert(
                    "device_id".into(),
                    serde_json::Value::String(account.device_id.clone()),
                );
                // account_uuid 统一成本账号(或留空),避免多人各自的真实账号 UUID 泄漏/不一致
                if obj.contains_key("account_uuid") {
                    obj.insert(
                        "account_uuid".into(),
                        serde_json::Value::String(account.account_uuid.clone().unwrap_or_default()),
                    );
                }
                let new_str = serde_json::to_string(&uid).unwrap_or_default();
                if let Some(metadata) =
                    body.get_mut("metadata").and_then(|m| m.as_object_mut())
                {
                    metadata.insert(
                        "user_id".into(),
                        serde_json::Value::String(new_str),
                    );
                }
                return;
            }
        }

        // 旧格式：user_{device}_account_{uuid}_session_{uuid}
        if let Some(idx) = user_id_str.find("_account_") {
            let new_val = format!(
                "user_{}_account_{}",
                account.device_id,
                &user_id_str[idx + 9..]
            );
            if let Some(metadata) = body.get_mut("metadata").and_then(|m| m.as_object_mut()) {
                metadata.insert("user_id".into(), serde_json::Value::String(new_val));
            }
        }
    }

    /// 为纯 API 调用创建 metadata.user_id。返回使用的 session_id。
    fn inject_metadata_user_id(
        &self,
        body: &mut serde_json::Value,
        account: &Account,
    ) -> Option<String> {
        // 确保 metadata 存在
        if body.get("metadata").is_none() {
            body.as_object_mut()
                .unwrap()
                .insert("metadata".into(), serde_json::json!({}));
        }

        // 已有 user_id，改为改写
        if body
            .get("metadata")
            .and_then(|m| m.get("user_id"))
            .is_some()
        {
            self.rewrite_metadata_user_id(body, account);
            return None;
        }

        let session_id = generate_session_uuid();
        let account_uuid = account.account_uuid.clone()
            .unwrap_or_else(|| derive_account_uuid(account));
        let uid = serde_json::json!({
            "device_id": account.device_id,
            "account_uuid": account_uuid,
            "session_id": session_id,
        });
        let uid_str = serde_json::to_string(&uid).unwrap_or_default();
        if let Some(metadata) = body.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            metadata.insert("user_id".into(), serde_json::Value::String(uid_str));
        }
        Some(session_id)
    }

    /// 将 Claude Code 系统提示词添加到请求体前面（仅 API 注入模式）。
    fn inject_system_prompt(&self, body: &mut serde_json::Value) {
        let banner_block = serde_json::json!({
            "type": "text",
            "text": CLAUDE_CODE_SYSTEM_PROMPT,
            "cache_control": { "type": "ephemeral" }
        });

        match body.get("system") {
            None => {
                body.as_object_mut().unwrap().insert(
                    "system".into(),
                    serde_json::Value::Array(vec![banner_block]),
                );
            }
            Some(serde_json::Value::String(sys)) => {
                if sys.starts_with(CLAUDE_CODE_SYSTEM_PROMPT) {
                    return;
                }
                let user_block = serde_json::json!({
                    "type": "text",
                    "text": sys,
                });
                body.as_object_mut().unwrap().insert(
                    "system".into(),
                    serde_json::Value::Array(vec![banner_block, user_block]),
                );
            }
            Some(serde_json::Value::Array(arr)) => {
                if let Some(first) = arr.first() {
                    if let Some(text) = first.get("text").and_then(|t| t.as_str()) {
                        if text.starts_with(CLAUDE_CODE_SYSTEM_PROMPT) {
                            return;
                        }
                    }
                }
                let mut new_arr = vec![banner_block];
                new_arr.extend(arr.iter().cloned());
                body.as_object_mut()
                    .unwrap()
                    .insert("system".into(), serde_json::Value::Array(new_arr));
            }
            _ => {}
        }
    }

    // --- 系统提示词改写（仅 CC 客户端模式）---

    /// normalize 模式下把 X-Stainless-OS/Arch 统一成账号的虚拟机器（与系统提示里的
    /// Platform/OS 保持一致），在原位替换值、保留顺序。Runtime-Version 是 Bun 模拟的
    /// normalize 模式下把机器级身份(OS/Arch)+ CC 版本(UA / package-version /
    /// runtime-version)统一成账号固定值。多人共号时,若只统一 device 却放任 15 个
    /// 不同 CC 版本透传,Anthropic 会看到"一台机器同时跑 15 个版本"——这是不可能的,
    /// 等于自报共号。把版本也钉死,一个号只呈现"一台机器+一个版本+多会话"=合法单人多 agent。
    pub fn normalize_os_headers_ordered(
        &self,
        headers: &mut [(String, String)],
        account: &Account,
        model: &str,
        captured: Option<&CapturedCoords>,
    ) {
        let env = self.parse_env(account);
        let os = stainless_os_from_platform(&env.platform);
        // 版本三项:优先本次吸取的 → 账号已存的 → 全局默认
        let (cap_v, cap_p, cap_r) = match captured {
            Some(c) => (
                c.cc_version.as_str(),
                c.package_version.as_str(),
                c.runtime_version.as_str(),
            ),
            None => ("", "", ""),
        };
        let version = pick3(cap_v, &env.version, DEFAULT_VERSION);
        let package_version = pick3(cap_p, &env.package_version, STAINLESS_PACKAGE_VERSION);
        let runtime_ver = pick3(cap_r, &env.node_version, DEFAULT_RUNTIME_VERSION);
        let ua = format!("claude-cli/{} (external, cli)", version);
        // anthropic-beta 统一成"该模型"的标准集合(通用部分,不吸取)
        let beta = beta_header_for_model(model);
        for (k, v) in headers.iter_mut() {
            match k.to_ascii_lowercase().as_str() {
                "x-stainless-os" => *v = os.to_string(),
                "x-stainless-arch" => *v = env.arch.clone(),
                "user-agent" => *v = ua.clone(),
                "x-stainless-package-version" => *v = package_version.clone(),
                "x-stainless-runtime-version" => *v = runtime_ver.clone(),
                "anthropic-beta" => *v = beta.to_string(),
                "accept" => *v = "application/json".to_string(),
                _ => {}
            }
        }
    }

    /// 多人共号身份归一化：把"是谁/哪台机器"统一成账号的固定虚拟身份，
    /// 让一个号始终像同一个人在用。只改身份字段（home 用户名 / git / 平台 / OS / device_id），
    /// 不动项目子路径和对话内容（真人本就会在多个项目里干活）。
    pub fn normalize_cc_identity(&self, body: &mut serde_json::Value, account: &Account) {
        let pe = self.parse_prompt_env(account);
        let (vuser, vgit) = account.effective_virtual_identity();

        // 虚拟环境只有 Mac/Linux。home 前缀按账号 OS（linux→/home，否则 /Users）。
        let home_plain = if pe.platform == "linux" {
            format!("/home/{}", vuser)
        } else {
            format!("/Users/{}", vuser)
        };
        let home_slug = format!(
            "{}{}",
            if pe.platform == "linux" { "-home-" } else { "-Users-" },
            vuser
        );
        let git_repl = format!("Git user: {}", vgit);
        let plat_repl = format!("Platform: {}", pe.platform);
        let shell_repl = format!("Shell: {}", pe.shell);
        let os_repl = format!("OS Version: {}", pe.os_version);

        // 兜底虚拟项目：按会话 id 稳定派生（同一对话始终是同一个虚拟项目）
        let session_id = body
            .get("metadata")
            .and_then(|m| m.get("user_id"))
            .and_then(|u| u.as_str())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|j| j.get("session_id").and_then(|x| x.as_str()).map(String::from))
            .unwrap_or_default();
        let vproj = crate::model::identity::virtual_project(&session_id);
        let vproj_dir = format!("{}/{}", home_plain, vproj); // /Users/vuser/<vproj>
        let vproj_slug = format!("{}-{}", home_slug, vproj); // -Users-vuser-<vproj>
        let anchor_repl = format!("{}/.claude/", home_plain);

        let scrub = |text: &str| -> String {
            // Windows 整条路径 C:\Users\bob\dev\app → /Users/vuser/dev/app
            //（去盘符、反斜杠转正斜杠、首段用户名替换为虚拟用户名）
            let mut t = HOME_WIN_FULL_REGEX
                .replace_all(text, |caps: &regex::Captures| {
                    let rest = &caps[1]; // bob\dev\app
                    let mut segs = rest.split('\\');
                    segs.next(); // 丢弃原用户名段
                    let sub: Vec<&str> = segs.filter(|s| !s.is_empty()).collect();
                    if sub.is_empty() {
                        home_plain.clone()
                    } else {
                        format!("{}/{}", home_plain, sub.join("/"))
                    }
                })
                .to_string();
            // Windows memory slug C--Users-bob → -Users-vuser（去盘符）
            t = WIN_SLUG_REGEX.replace_all(&t, home_slug.as_str()).to_string();
            // Unix home 明文 + slug
            t = HOME_PLAIN_REGEX.replace_all(&t, home_plain.as_str()).to_string();
            t = HOME_SLUG_REGEX.replace_all(&t, home_slug.as_str()).to_string();
            t = GIT_USER_REGEX.replace_all(&t, git_repl.as_str()).to_string();
            t = PLATFORM_REGEX.replace_all(&t, plat_repl.as_str()).to_string();
            t = SHELL_REGEX.replace_all(&t, shell_repl.as_str()).to_string();
            t = OS_VERSION_REGEX.replace_all(&t, os_repl.as_str()).to_string();

            // 兜底①：任意非标准 HOME（以 /.claude/ 为锚）一律归一到虚拟 home
            t = HOME_ANCHOR_REGEX
                .replace_all(&t, anchor_repl.as_str())
                .to_string();
            // 兜底②：工作目录若不在虚拟 home 下（未捕捉到），固定到会话虚拟项目
            t = WORKING_DIR_REGEX
                .replace_all(&t, |caps: &regex::Captures| {
                    let prefix = &caps[1];
                    let path = &caps[0][prefix.len()..];
                    if path.starts_with(home_plain.as_str()) {
                        caps[0].to_string()
                    } else {
                        format!("{}{}", prefix, vproj_dir)
                    }
                })
                .to_string();
            // 兜底③：memory 项目 slug 若未归一化，固定到会话虚拟项目 slug
            t = PROJECTS_SLUG_REGEX
                .replace_all(&t, |caps: &regex::Captures| {
                    if caps[2].starts_with(home_slug.as_str()) {
                        caps[0].to_string()
                    } else {
                        format!("{}{}{}", &caps[1], vproj_slug, &caps[3])
                    }
                })
                .to_string();
            t
        };

        // system 块
        match body.get_mut("system") {
            Some(serde_json::Value::Array(arr)) => {
                for item in arr.iter_mut() {
                    if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                        let nt = scrub(t);
                        if let Some(o) = item.as_object_mut() {
                            o.insert("text".into(), serde_json::Value::String(nt));
                        }
                    }
                }
            }
            Some(serde_json::Value::String(s)) => {
                let nt = scrub(s);
                *s = nt;
            }
            _ => {}
        }

        // messages 里的文本块 / system-reminder（cwd、CLAUDE.md 路径、git 也可能在这里）
        if let Some(msgs) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            for m in msgs.iter_mut() {
                match m.get_mut("content") {
                    Some(serde_json::Value::String(s)) => {
                        let nt = scrub(s);
                        *s = nt;
                    }
                    Some(serde_json::Value::Array(blocks)) => {
                        for b in blocks.iter_mut() {
                            if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                                let nt = scrub(t);
                                if let Some(o) = b.as_object_mut() {
                                    o.insert("text".into(), serde_json::Value::String(nt));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // device_id 归一化为账号固定值
        self.rewrite_metadata_user_id(body, account);
    }

    fn rewrite_system_prompt(
        &self,
        body: &mut serde_json::Value,
        pe: &CanonicalPromptEnvData,
        version: &str,
        billing_mode: &BillingMode,
    ) {
        let version = if version.is_empty() {
            DEFAULT_VERSION
        } else {
            version
        };

        // CCH hash 计算
        let cch_hash = if *billing_mode == BillingMode::Rewrite {
            let first_msg = extract_first_user_message(body);
            if !first_msg.is_empty() {
                compute_cch(&first_msg, version)
            } else {
                let mut bytes = [0u8; 2];
                rand::thread_rng().fill(&mut bytes);
                format!("{:x}", u16::from_be_bytes(bytes))[..3].to_string()
            }
        } else {
            String::new()
        };

        let rewrite = |text: &str| -> String {
            let mut text = text.to_string();
            if *billing_mode == BillingMode::Rewrite {
                text = BILLING_VERSION_REGEX
                    .replace_all(&text, &format!("cc_version={}.{}", version, cch_hash))
                    .to_string();
                // 将已有的 cch 值重置为占位符，后续在序列化后通过 xxhash64 重新计算
                text = CCH_VALUE_REGEX
                    .replace_all(&text, "cch=00000")
                    .to_string();
            } else {
                text = BILLING_LINE_REGEX.replace_all(&text, "").to_string();
                text = BILLING_REGEX.replace_all(&text, "").to_string();
            }
            text = PLATFORM_REGEX
                .replace_all(&text, &format!("Platform: {}", pe.platform))
                .to_string();
            text = SHELL_REGEX
                .replace_all(&text, &format!("Shell: {}", pe.shell))
                .to_string();
            text = OS_VERSION_REGEX
                .replace_all(&text, &format!("OS Version: {}", pe.os_version))
                .to_string();
            text = WORKING_DIR_REGEX
                .replace_all(&text, &format!("${{1}}{}", pe.working_dir))
                .to_string();
            let home_prefix = if let Some(idx) = nth_index(&pe.working_dir, '/', 3) {
                &pe.working_dir[..idx + 1]
            } else {
                &pe.working_dir
            };
            text = HOME_PATH_REGEX
                .replace_all(&text, home_prefix)
                .to_string();
            text
        };

        let rewrite_in_reminders = |text: &str| -> String {
            SYSTEM_REMINDER_REGEX
                .replace_all(text, |caps: &regex::Captures| rewrite(&caps[0]))
                .to_string()
        };

        // 改写 body.system
        match body.get("system").cloned() {
            Some(serde_json::Value::String(sys)) => {
                body.as_object_mut().unwrap().insert(
                    "system".into(),
                    serde_json::Value::String(rewrite(&sys)),
                );
            }
            Some(serde_json::Value::Array(sys)) => {
                let filtered: Vec<serde_json::Value> = if *billing_mode == BillingMode::Strip {
                    sys.iter()
                        .filter(|item| {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                if BILLING_LINE_REGEX.is_match(text) {
                                    let cleaned =
                                        BILLING_LINE_REGEX.replace_all(text, "").to_string();
                                    if cleaned.trim().is_empty() {
                                        return false;
                                    }
                                }
                            }
                            true
                        })
                        .cloned()
                        .collect()
                } else {
                    sys.clone()
                };

                let rewritten: Vec<serde_json::Value> = filtered
                    .into_iter()
                    .map(|mut item| {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            let new_text = rewrite(text);
                            item.as_object_mut()
                                .unwrap()
                                .insert("text".into(), serde_json::Value::String(new_text));
                        }
                        item
                    })
                    .collect();

                body.as_object_mut()
                    .unwrap()
                    .insert("system".into(), serde_json::Value::Array(rewritten));
            }
            _ => {}
        }

        // 改写消息 — 仅在 <system-reminder> 标签内替换
        if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            for msg in messages.iter_mut() {
                rewrite_message_content(msg, &rewrite_in_reminders);
            }
        }
    }

    // --- 事件日志批量改写 ---

    fn rewrite_event_batch(&self, body: &mut serde_json::Value, account: &Account) {
        let env = self.parse_env(account);
        let proc = self.parse_process(account);

        let events = match body.get_mut("events").and_then(|e| e.as_array_mut()) {
            Some(e) => e,
            None => return,
        };

        let canonical_env = build_canonical_env_map(&env);

        for event in events.iter_mut() {
            let e = match event.as_object_mut() {
                Some(e) => e,
                None => continue,
            };

            if e.contains_key("device_id") {
                e.insert(
                    "device_id".into(),
                    serde_json::Value::String(account.device_id.clone()),
                );
            }
            if e.contains_key("email") {
                e.insert(
                    "email".into(),
                    serde_json::Value::String(account.email.clone()),
                );
            }

            e.remove("baseUrl");
            e.remove("base_url");
            e.remove("gateway");

            // 改写 account_uuid / organization_uuid
            if e.contains_key("account_uuid") {
                let uuid = account.account_uuid.clone()
                    .unwrap_or_else(|| derive_account_uuid(account));
                e.insert("account_uuid".into(), serde_json::Value::String(uuid));
            }
            if e.contains_key("organization_uuid") {
                if let Some(ref org) = account.organization_uuid {
                    e.insert("organization_uuid".into(), serde_json::Value::String(org.clone()));
                } else {
                    e.remove("organization_uuid");
                }
            }

            if e.contains_key("env") {
                e.insert("env".into(), canonical_env.clone());
            }

            if let Some(p) = e.remove("process") {
                e.insert("process".into(), rewrite_process(&p, &proc));
            }

            if let Some(am) = e.get("additional_metadata").and_then(|v| v.as_str()) {
                let rewritten = rewrite_additional_metadata(am);
                e.insert(
                    "additional_metadata".into(),
                    serde_json::Value::String(rewritten),
                );
            }

            // 改写 user_attributes（GrowthBook 实验事件中的 JSON 字符串）
            if let Some(ua_str) = e.get("user_attributes").and_then(|v| v.as_str()) {
                let rewritten = rewrite_user_attributes_json(ua_str, account);
                e.insert(
                    "user_attributes".into(),
                    serde_json::Value::String(rewritten),
                );
            }
        }
    }

    // --- GrowthBook remoteEval 改写 (POST /api/eval/{clientKey}) ---

    fn rewrite_growthbook_eval(&self, body: &mut serde_json::Value, account: &Account) {
        let env = self.parse_env(account);
        let attrs = match body.get_mut("attributes").and_then(|a| a.as_object_mut()) {
            Some(a) => a,
            None => return,
        };

        // 身份字段
        attrs.insert("id".into(), serde_json::Value::String(account.device_id.clone()));
        attrs.insert("deviceID".into(), serde_json::Value::String(account.device_id.clone()));

        if attrs.contains_key("email") {
            attrs.insert("email".into(), serde_json::Value::String(account.email.clone()));
        }
        if attrs.contains_key("accountUUID") {
            let uuid = account.account_uuid.clone()
                .unwrap_or_else(|| derive_account_uuid(account));
            attrs.insert("accountUUID".into(), serde_json::Value::String(uuid));
        }
        if let Some(ref org) = account.organization_uuid {
            attrs.insert("organizationUUID".into(), serde_json::Value::String(org.clone()));
        } else {
            attrs.remove("organizationUUID");
        }
        if let Some(ref sub) = account.subscription_type {
            attrs.insert("subscriptionType".into(), serde_json::Value::String(sub.clone()));
        }

        // 移除代理暴露字段
        attrs.remove("apiBaseUrlHost");

        // 环境对齐
        attrs.insert("platform".into(), serde_json::Value::String(env.platform.clone()));
        if attrs.contains_key("appVersion") {
            attrs.insert("appVersion".into(), serde_json::Value::String(env.version.clone()));
        }
    }

    // --- 通用身份改写 ---

    fn rewrite_generic_identity(&self, body: &mut serde_json::Value, account: &Account) {
        if let Some(obj) = body.as_object_mut() {
            if obj.contains_key("device_id") {
                obj.insert(
                    "device_id".into(),
                    serde_json::Value::String(account.device_id.clone()),
                );
            }
            if obj.contains_key("email") {
                obj.insert(
                    "email".into(),
                    serde_json::Value::String(account.email.clone()),
                );
            }
        }
    }

    // --- 辅助解析 ---

    fn parse_env(&self, account: &Account) -> CanonicalEnvData {
        serde_json::from_value(account.canonical_env.clone()).unwrap_or_default()
    }

    fn parse_prompt_env(&self, account: &Account) -> CanonicalPromptEnvData {
        serde_json::from_value(account.canonical_prompt.clone()).unwrap_or_default()
    }

    fn parse_process(&self, account: &Account) -> CanonicalProcessData {
        serde_json::from_value(account.canonical_process.clone()).unwrap_or_default()
    }
}

// --- 正则表达式 ---

static PLATFORM_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Platform:\s*\S+").unwrap());
static SHELL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Shell:\s*\S+").unwrap());
static OS_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"OS Version:\s*[^\n<]+").unwrap());
static WORKING_DIR_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"((?:Primary )?[Ww]orking directory:\s*)/\S+").unwrap());
static HOME_PATH_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"/(?:Users|home)/[^/\s]+/").unwrap());
static BILLING_LINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^\s*x-anthropic-billing-header:[^\n]*\n?").unwrap());
static BILLING_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"cc_version=[\d.]+\.[a-f0-9]{3};[^;]*;?").unwrap());
/// 仅匹配 cc_version 值部分，用于 Rewrite 模式保留 cc_entrypoint。
static BILLING_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"cc_version=[\d.]+\.[a-f0-9]{3}").unwrap());
static CCH_VALUE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"cch=[a-f0-9]{5}").unwrap());
static GIT_USER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Git user:\s*[^\n]+").unwrap());
/// home 路径明文形态 /Users/<name> 或 /home/<name>（捕获用户名段）
static HOME_PLAIN_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(/(?:Users|home)/)([^/\s"<]+)"#).unwrap());
/// home 路径 slug 形态 -Users-<name> 或 -home-<name>（出现在 .claude/projects 的目录名里）
static HOME_SLUG_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(-(?:Users|home)-)([^-\s"/<]+)"#).unwrap());
/// Windows home 整条路径 C:\Users\<name>\<子路径>（捕获 Users 之后的部分，用于整体转成 Unix 风格）
static HOME_WIN_FULL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"[A-Za-z]:\\Users\\([^"\s<>\n]*)"#).unwrap());
/// Windows memory slug 形态 C--Users-<name>（带盘符前缀），转成 Unix slug
static WIN_SLUG_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"[A-Za-z]--Users-([^-\s"/<]+)"#).unwrap());
/// 兜底：任意 home 路径（以 /.claude/ 为锚），覆盖非标准 HOME
static HOME_ANCHOR_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"([/\w.~-]+)/\.claude/").unwrap());
/// .claude/projects/<slug>/ 中的项目 slug（兜底替换用）
static PROJECTS_SLUG_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(\.claude/projects/)([^/\s"]+)(/)"#).unwrap());
static SYSTEM_REMINDER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)<system-reminder>(.*?)</system-reminder>").unwrap());

// --- CCH Attestation (xxhash64) ---

const CCH_ATTESTATION_SEED: u64 = 0x6E52736AC806831E;
const CCH_PLACEHOLDER: &[u8] = b"cch=00000";

/// 对序列化后的 body 字节计算 cch attestation 并原地替换占位符。
/// 算法：xxhash64(body_with_placeholder, seed) 取低 20 bits → 5 位十六进制。
fn compute_cch_attestation(mut body: Vec<u8>) -> Vec<u8> {
    if let Some(pos) = body
        .windows(CCH_PLACEHOLDER.len())
        .position(|w| w == CCH_PLACEHOLDER)
    {
        let hash = xxhash_rust::xxh64::xxh64(&body, CCH_ATTESTATION_SEED);
        let cch = format!("{:05x}", hash & 0xFFFFF);
        // "cch=" 占 4 字节，后续 5 字节是 "00000"
        body[pos + 4..pos + 9].copy_from_slice(cch.as_bytes());
    }
    body
}

// --- CCH fingerprint (SHA256) ---

const CCH_SALT: &str = "59cf53e54c78";
const CCH_POSITIONS: [usize; 3] = [4, 7, 20];

fn compute_cch(first_user_message_text: &str, version: &str) -> String {
    let bytes = first_user_message_text.as_bytes();
    let mut chars = Vec::new();
    for &pos in &CCH_POSITIONS {
        if pos < bytes.len() {
            chars.push(bytes[pos]);
        } else {
            chars.push(b'0');
        }
    }
    let input = format!("{}{}{}", CCH_SALT, String::from_utf8_lossy(&chars), version);
    let hash = Sha256::digest(input.as_bytes());
    format!("{:x}", hash)[..3].to_string()
}

/// 从 messages 数组中提取首条用户消息文本。
fn extract_first_user_message(body: &serde_json::Value) -> String {
    let messages = match body.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return String::new(),
    };
    for msg in messages {
        let m = match msg.as_object() {
            Some(m) => m,
            None => continue,
        };
        if m.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        match m.get("content") {
            Some(serde_json::Value::String(c)) => return c.clone(),
            Some(serde_json::Value::Array(arr)) => {
                for item in arr {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        return text.to_string();
                    }
                }
            }
            _ => {}
        }
    }
    String::new()
}

fn rewrite_message_content<F>(msg: &mut serde_json::Value, rewrite_fn: &F)
where
    F: Fn(&str) -> String,
{
    match msg.get("content").cloned() {
        Some(serde_json::Value::String(s)) => {
            msg.as_object_mut().unwrap().insert(
                "content".into(),
                serde_json::Value::String(rewrite_fn(&s)),
            );
        }
        Some(serde_json::Value::Array(arr)) => {
            let rewritten: Vec<serde_json::Value> = arr
                .into_iter()
                .map(|mut item| {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        let new_text = rewrite_fn(text);
                        item.as_object_mut()
                            .unwrap()
                            .insert("text".into(), serde_json::Value::String(new_text));
                    }
                    item
                })
                .collect();
            msg.as_object_mut()
                .unwrap()
                .insert("content".into(), serde_json::Value::Array(rewritten));
        }
        _ => {}
    }
}

fn build_canonical_env_map(env: &CanonicalEnvData) -> serde_json::Value {
    serde_json::json!({
        "platform": env.platform,
        "platform_raw": env.platform_raw,
        "arch": env.arch,
        "node_version": env.node_version,
        "terminal": env.terminal,
        "package_managers": env.package_managers,
        "runtimes": env.runtimes,
        "is_running_with_bun": false,
        "is_ci": false,
        "is_claubbit": false,
        "is_claude_code_remote": false,
        "is_local_agent_mode": false,
        "is_conductor": false,
        "is_github_action": false,
        "is_claude_code_action": false,
        "is_claude_ai_auth": env.is_claude_ai_auth,
        "version": env.version,
        "version_base": env.version_base,
        "build_time": env.build_time,
        "deployment_environment": env.deployment_environment,
        "vcs": env.vcs,
    })
}

// --- 进程指纹改写 ---

fn rewrite_process(original: &serde_json::Value, proc: &CanonicalProcessData) -> serde_json::Value {
    let engine = base64::engine::general_purpose::STANDARD;
    match original {
        serde_json::Value::String(s) => {
            let decoded = match engine.decode(s) {
                Ok(d) => d,
                Err(_) => return original.clone(),
            };
            let mut obj: serde_json::Value = match serde_json::from_slice(&decoded) {
                Ok(v) => v,
                Err(_) => return original.clone(),
            };
            rewrite_process_fields(&mut obj, proc);
            let out = serde_json::to_vec(&obj).unwrap_or_default();
            serde_json::Value::String(engine.encode(&out))
        }
        serde_json::Value::Object(_) => {
            let mut obj = original.clone();
            rewrite_process_fields(&mut obj, proc);
            obj
        }
        _ => original.clone(),
    }
}

fn rewrite_process_fields(obj: &mut serde_json::Value, proc: &CanonicalProcessData) {
    if let Some(map) = obj.as_object_mut() {
        map.insert(
            "constrainedMemory".into(),
            serde_json::json!(proc.constrained_memory),
        );
        map.insert(
            "rss".into(),
            serde_json::json!(random_in_range(proc.rss_range[0], proc.rss_range[1])),
        );
        map.insert(
            "heapTotal".into(),
            serde_json::json!(random_in_range(
                proc.heap_total_range[0],
                proc.heap_total_range[1]
            )),
        );
        map.insert(
            "heapUsed".into(),
            serde_json::json!(random_in_range(
                proc.heap_used_range[0],
                proc.heap_used_range[1]
            )),
        );
    }
}

// --- Base64 additional_metadata 改写 ---

fn rewrite_additional_metadata(encoded: &str) -> String {
    let engine = base64::engine::general_purpose::STANDARD;
    let decoded = match engine.decode(encoded) {
        Ok(d) => d,
        Err(_) => return encoded.to_string(),
    };
    let mut obj: serde_json::Value = match serde_json::from_slice(&decoded) {
        Ok(v) => v,
        Err(_) => return encoded.to_string(),
    };
    if let Some(map) = obj.as_object_mut() {
        map.remove("baseUrl");
        map.remove("base_url");
        map.remove("gateway");
    }
    let out = serde_json::to_vec(&obj).unwrap_or_default();
    engine.encode(&out)
}

/// 改写 GrowthBook 实验事件中 user_attributes JSON 字符串内的身份字段。
fn rewrite_user_attributes_json(json_str: &str, account: &Account) -> String {
    let mut obj: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return json_str.to_string(),
    };
    if let Some(map) = obj.as_object_mut() {
        if map.contains_key("id") {
            map.insert("id".into(), serde_json::Value::String(account.device_id.clone()));
        }
        if map.contains_key("deviceID") {
            map.insert("deviceID".into(), serde_json::Value::String(account.device_id.clone()));
        }
        if map.contains_key("email") {
            map.insert("email".into(), serde_json::Value::String(account.email.clone()));
        }
        if map.contains_key("accountUUID") {
            let uuid = account.account_uuid.clone()
                .unwrap_or_else(|| derive_account_uuid(account));
            map.insert("accountUUID".into(), serde_json::Value::String(uuid));
        }
        if let Some(ref org) = account.organization_uuid {
            map.insert("organizationUUID".into(), serde_json::Value::String(org.clone()));
        } else {
            map.remove("organizationUUID");
        }
        if let Some(ref sub) = account.subscription_type {
            map.insert("subscriptionType".into(), serde_json::Value::String(sub.clone()));
        }
        map.remove("apiBaseUrlHost");
    }
    serde_json::to_string(&obj).unwrap_or_else(|_| json_str.to_string())
}

/// 移除 system 和消息内容块中的 cache_control。
fn strip_cache_control(body: &mut serde_json::Value) {
    if let Some(sys) = body.get_mut("system").and_then(|s| s.as_array_mut()) {
        for item in sys.iter_mut() {
            if let Some(block) = item.as_object_mut() {
                block.remove("cache_control");
            }
        }
    }
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                for item in content.iter_mut() {
                    if let Some(block) = item.as_object_mut() {
                        block.remove("cache_control");
                    }
                }
            }
        }
    }
}

/// 移除消息和 system 中的空文本内容块。
fn strip_empty_text_blocks(body: &mut serde_json::Value) {
    fn filter_blocks(blocks: &mut Vec<serde_json::Value>) {
        blocks.retain(|item| {
            if let Some(block) = item.as_object() {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    let text = block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    if text.is_empty() {
                        return false;
                    }
                }
            }
            true
        });
        // Handle tool_result nested content
        for item in blocks.iter_mut() {
            if let Some(block) = item.as_object_mut() {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    if let Some(content) = block.get_mut("content").and_then(|c| c.as_array_mut())
                    {
                        filter_blocks(content);
                    }
                }
            }
        }
    }

    if let Some(sys) = body.get_mut("system").and_then(|s| s.as_array_mut()) {
        filter_blocks(sys);
    }
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                filter_blocks(content);
            }
        }
    }
}

/// 从注入模式 body 中获取暂存的 _session_id。
pub fn extract_session_id_from_body(body: &serde_json::Value) -> Option<String> {
    body.get("metadata")
        .and_then(|m| m.get("_session_id"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

/// 真实 Claude Code 的 header 顺序（小写规范名，取自 2.1.156 抓包）。
/// auth(authorization/x-api-key) 单独注入到 x-app 之前；传输头由 undici 自动追加到末尾。
const CANONICAL_HEADER_ORDER: &[&str] = &[
    "accept",
    "content-type",
    "user-agent",
    "x-claude-code-session-id",
    "x-stainless-arch",
    "x-stainless-lang",
    "x-stainless-os",
    "x-stainless-package-version",
    "x-stainless-retry-count",
    "x-stainless-runtime",
    "x-stainless-runtime-version",
    "x-stainless-timeout",
    "anthropic-beta",
    "anthropic-dangerous-direct-browser-access",
    "anthropic-version",
    "x-app",
];

/// 转发时需丢弃的头：auth 由调用方注入；传输头(host/content-length/connection/
/// accept-encoding 等)由 undici 自动按真 CC 顺序追加到末尾。
fn is_drop_header(k: &str) -> bool {
    matches!(
        k.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-api-key"
            | "host"
            | "content-length"
            | "connection"
            | "accept-encoding"
            | "proxy-connection"
            | "transfer-encoding"
            | "keep-alive"
            | "te"
            | "upgrade"
    )
}

/// 透传(有序)：保留客户端原始顺序+大小写，去掉 auth/hop-by-hop/accept-encoding。
/// 返回有序 Vec，供边车(undici)按真 CC 顺序原样发出。auth 由调用方注入。
pub fn passthrough_headers_ordered(ordered: &[(String, String)]) -> Vec<(String, String)> {
    ordered
        .iter()
        .filter(|(k, _)| !is_drop_header(k))
        .map(|(k, v)| (resolve_wire_casing(k), v.clone()))
        .collect()
}

/// 把(无序 HashMap)请求头按真 CC 规范顺序排成有序 Vec(用于 API 注入模式)。
/// 已知头按 CANONICAL_HEADER_ORDER 排，未知头按字母序追加在 x-app 之前。
pub fn order_headers_canonical(map: &HashMap<String, String>) -> Vec<(String, String)> {
    let lower_map: HashMap<String, &String> = map
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v))
        .collect();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for canon in CANONICAL_HEADER_ORDER {
        if *canon == "x-app" {
            continue; // x-app 最后单独放(auth 注入在它前面)
        }
        if let Some(v) = lower_map.get(*canon) {
            out.push((resolve_wire_casing(canon), (*v).clone()));
            used.insert((*canon).to_string());
        }
    }
    // 未知头(不在规范表、非 drop、非 x-app)按字母序追加
    let mut extras: Vec<(&String, &String)> = map
        .iter()
        .filter(|(k, _)| {
            let lk = k.to_ascii_lowercase();
            !is_drop_header(&lk) && !used.contains(&lk) && lk != "x-app"
        })
        .collect();
    extras.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
    for (k, v) in extras {
        out.push((resolve_wire_casing(k), v.clone()));
    }
    if let Some(v) = lower_map.get("x-app") {
        out.push((resolve_wire_casing("x-app"), (*v).clone()));
    }
    out
}

/// 把账号 token 以 authorization 头注入到 x-app 之前(真 CC 的 auth 槽位)，无 x-app 则追加末尾。
pub fn inject_auth_before_xapp(
    mut headers: Vec<(String, String)>,
    token: &str,
) -> Vec<(String, String)> {
    let auth = ("authorization".to_string(), format!("Bearer {}", token));
    if let Some(pos) = headers
        .iter()
        .position(|(k, _)| k.eq_ignore_ascii_case("x-app"))
    {
        headers.insert(pos, auth);
    } else {
        headers.push(auth);
    }
    headers
}

/// 清理 body 中的内部 _session_id 标记。
pub fn clean_session_id_from_body(body: &mut serde_json::Value) {
    if let Some(metadata) = body.get_mut("metadata").and_then(|m| m.as_object_mut()) {
        metadata.remove("_session_id");
    }
}

/// 判断请求来自 Claude Code 还是纯 API。
pub fn detect_client_type(user_agent: &str, body: &serde_json::Value) -> ClientType {
    let ua_lower = user_agent.to_lowercase();
    if ua_lower.starts_with("claude-code/") || ua_lower.starts_with("claude-cli/") {
        return ClientType::ClaudeCode;
    }
    if let Some(metadata) = body.get("metadata").and_then(|m| m.as_object()) {
        if metadata.contains_key("user_id") {
            return ClientType::ClaudeCode;
        }
    }
    ClientType::API
}

const CLAUDE_CODE_SYSTEM_PROMPT: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// 通过账号信息生成稳定的 UUID 标识符。
fn derive_account_uuid(account: &Account) -> String {
    let seed = if account.email.is_empty() {
        format!("account-{}", account.id)
    } else {
        account.email.clone()
    };
    let hash = Sha256::digest(seed.as_bytes());
    format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&hash[0..4]),
        hex::encode(&hash[4..6]),
        hex::encode(&hash[6..8]),
        hex::encode(&hash[8..10]),
        hex::encode(&hash[10..16])
    )
}

pub fn generate_session_uuid() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&b[0..4]),
        hex::encode(&b[4..6]),
        hex::encode(&b[6..8]),
        hex::encode(&b[8..10]),
        hex::encode(&b[10..16])
    )
}

fn random_in_range(min: i64, max: i64) -> i64 {
    if max <= min {
        return min;
    }
    rand::thread_rng().gen_range(min..max)
}

/// 仅在 `<system-reminder>` 标签内替换 `Git user:` 行。
/// 不影响 messages、tools 和 `<system-reminder>` 外部的文本，避免破坏 git 操作。
fn scrub_git_user_in_reminders(body: &mut serde_json::Value, replacement_name: &str) {
    let replacement = format!("Git user: {}", replacement_name);
    let scrub = |text: &str| -> String {
        SYSTEM_REMINDER_REGEX
            .replace_all(text, |caps: &regex::Captures| {
                GIT_USER_REGEX.replace_all(&caps[0], replacement.as_str()).to_string()
            })
            .to_string()
    };

    if let Some(system) = body.get_mut("system") {
        match system {
            serde_json::Value::String(s) => {
                *s = scrub(s);
            }
            serde_json::Value::Array(arr) => {
                for item in arr.iter_mut() {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        let new_text = scrub(text);
                        item.as_object_mut()
                            .unwrap()
                            .insert("text".into(), serde_json::Value::String(new_text));
                    }
                }
            }
            _ => {}
        }
    }
}

/// 将 canonical env 的 platform 映射为 X-Stainless-OS 值。
fn stainless_os_from_platform(platform: &str) -> &str {
    match platform {
        "darwin" => "MacOS", // 真实 Claude Code 发送的是 MacOS（抓包对齐）
        "win32" => "Windows",
        _ => "Linux",
    }
}

fn nth_index(s: &str, c: char, n: usize) -> Option<usize> {
    let mut count = 0;
    for (i, ch) in s.chars().enumerate() {
        if ch == c {
            count += 1;
            if count == n {
                return Some(i);
            }
        }
    }
    None
}
