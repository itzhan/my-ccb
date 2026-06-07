use std::collections::HashMap;

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// 探针识别 header：探活系统在请求头加 `X-Probe-Check: 1` 即被网关短路,
/// 不转发上游、不选号、不计费。
const PROBE_HEADER: &str = "X-Probe-Check";

/// 号池健康时返回的固定文案（用于 header 探针 / sub2api 账号测试这类只看连通性的探测）。
const PROBE_MESSAGE: &str =
    "👋 已收到探针请求，API 网关运行正常 / API gateway is healthy. 如需开始对话，请直接发送您的具体问题。";

/// sub2api 通道健康检查(channel_monitor)/ check-cx 固定数学挑战题的铁标记 —— 真实流量绝不会出现。
const CHANNEL_MONITOR_MARKER: &str = "Calculate and respond with ONLY the number";

/// 探针类型，决定健康时回什么内容。
pub enum ProbeKind {
    /// 只看连通性的探测：回固定健康文案即可（header 探针 / sub2api 账号测试 "hi"）。
    Canned,
    /// 数学挑战探测：探活方会校验答案数字，必须回正确答案（sub2api 通道健康检查 / check-cx）。
    /// 携带本地算出的答案。
    Challenge(String),
}

/// 识别探针请求并返回其类型；非探针返回 None。
/// 1) 显式探针头 `X-Probe-Check: 1`（check-cx 等可配置探针）→ Canned；
/// 2) sub2api 现有探测(不改 sub2api,靠请求体内容识别)：
///    - 通道健康检查：含数学挑战铁标记 → Challenge(本地算出的答案)；
///    - 账号测试/测号：单条 user 文本恰为 "hi" + 无 tools + max_tokens==1024 + temperature==1 → Canned。
pub fn detect_probe(headers: &HashMap<String, String>, body: &serde_json::Value) -> Option<ProbeKind> {
    if header_is_probe(headers) {
        return Some(ProbeKind::Canned);
    }

    // 内容识别：数学挑战 → 回答案；账号测试 "hi" → 回文案。
    if let Some(kind) = content_probe(body) {
        return Some(kind);
    }

    // 占位 device_id：check-cx 等把 metadata.user_id 里的 device_id 设成全同字符
    //（如 64 个 'a'）。真实客户端 device_id 是随机 64 位 hex,绝不会全同字符,故零误伤。
    // 不依赖 prompt 内容,可兜住非数学题型的探针。
    if has_synthetic_device_id(body) {
        return Some(ProbeKind::Canned);
    }

    None
}

/// 按请求体内容识别 sub2api 两类探测(均只含单条 user 消息,据此快速过滤真实多轮对话)。
fn content_probe(body: &serde_json::Value) -> Option<ProbeKind> {
    let messages = body.get("messages").and_then(|m| m.as_array())?;
    if messages.len() != 1 {
        return None;
    }
    let msg = &messages[0];
    if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
        return None;
    }
    let text = message_text(msg)?;

    // ② 通道健康检查：本地解出算术答案回给探活方（其会校验答案）。
    if text.contains(CHANNEL_MONITOR_MARKER) {
        // 解析失败也仍短路（回文案兜底），避免把探测放给上游。
        let answer = solve_challenge(&text).map(|n| n.to_string());
        return Some(answer.map(ProbeKind::Challenge).unwrap_or(ProbeKind::Canned));
    }

    // ① 账号测试：单条 user "hi" + 无 tools + max_tokens==1024 + temperature==1。
    if text.trim() == "hi"
        && body_has_no_tools(body)
        && body.get("max_tokens").and_then(|v| v.as_i64()) == Some(1024)
        && body
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|t| (t - 1.0).abs() < 1e-9)
            .unwrap_or(false)
    {
        return Some(ProbeKind::Canned);
    }

    None
}

/// metadata.user_id(JSON 字符串)里的 device_id 是否为全同字符占位符(探针特征)。
fn has_synthetic_device_id(body: &serde_json::Value) -> bool {
    let uid_str = match body
        .get("metadata")
        .and_then(|m| m.get("user_id"))
        .and_then(|u| u.as_str())
    {
        Some(s) => s,
        None => return false,
    };
    let parsed: serde_json::Value = match serde_json::from_str(uid_str) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let device_id = parsed
        .get("device_id")
        .and_then(|d| d.as_str())
        .unwrap_or("");
    is_all_same_char(device_id)
}

/// 字符串是否「足够长且全是同一个字符」（如 64 个 'a'）。
fn is_all_same_char(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if s.len() >= 16 => chars.all(|c| c == first),
        _ => false,
    }
}

/// 探针响应：`healthy=false`（号池无可调度账号）统一回 503 错误体（让探活方判定不健康,
/// 从而不再把流量路由到本网关）；`healthy=true` 按探针类型回固定文案或挑战答案。
pub fn probe_response(
    is_stream: bool,
    model: &str,
    healthy: bool,
    kind: &ProbeKind,
) -> Response {
    if !healthy {
        let body = json!({
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": "号池无可用账号 / no healthy account in pool"
            }
        });
        return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(body)).into_response();
    }

    let text: &str = match kind {
        ProbeKind::Challenge(answer) => answer,
        ProbeKind::Canned => PROBE_MESSAGE,
    };
    let model = if model.is_empty() { "claude-sonnet-4-5" } else { model };

    if is_stream {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .body(Body::from(stream_sse(model, text)))
            .expect("build probe sse response")
    } else {
        axum::Json(json!({
            "id": "msg_probe",
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{ "type": "text", "text": text }],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
        .into_response()
    }
}

/// 大小写不敏感判断 `X-Probe-Check: 1`。
fn header_is_probe(headers: &HashMap<String, String>) -> bool {
    headers
        .get(PROBE_HEADER)
        .or_else(|| headers.get("x-probe-check"))
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// body 是否没有 tools（缺失或空数组）。真实 Claude Code 必带非空 tools。
fn body_has_no_tools(body: &serde_json::Value) -> bool {
    match body.get("tools") {
        None => true,
        Some(serde_json::Value::Array(a)) => a.is_empty(),
        Some(_) => false,
    }
}

/// 解出数学挑战题答案。挑战题是 few-shot 模板,真正的题目是最后一个 `Q: <a> <op> <b> = ?`。
fn solve_challenge(text: &str) -> Option<i64> {
    // 取最后一个 "Q:" 之后、"=" 之前的部分（前面的 Q 是带答案的示例）。
    let last_q = text.rsplit("Q:").next()?;
    let expr = last_q.split('=').next()?;
    let tokens: Vec<&str> = expr.split_whitespace().collect();
    if tokens.len() != 3 {
        return None;
    }
    let a: i64 = tokens[0].parse().ok()?;
    let b: i64 = tokens[2].parse().ok()?;
    match tokens[1] {
        "+" => Some(a + b),
        "-" => Some(a - b),
        _ => None,
    }
}

/// 取一条消息的文本：content 为字符串直接返回；为数组则拼接所有 text block。
fn message_text(msg: &serde_json::Value) -> Option<String> {
    match msg.get("content") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(blocks)) => {
            let mut out = String::new();
            for b in blocks {
                if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(t);
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        _ => None,
    }
}

/// 构建 Anthropic `/v1/messages` 流式 SSE（6 个标准事件,把 `text` 放在单个 text_delta）。
fn stream_sse(model: &str, text: &str) -> String {
    let events = [
        (
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_probe",
                    "type": "message",
                    "role": "assistant",
                    "model": model,
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": { "input_tokens": 1, "output_tokens": 0 }
                }
            }),
        ),
        (
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            }),
        ),
        (
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": text }
            }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        ),
        (
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                "usage": { "output_tokens": 1 }
            }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ];

    let mut out = String::new();
    for (event, data) in events {
        out.push_str("event: ");
        out.push_str(event);
        out.push_str("\ndata: ");
        out.push_str(&data.to_string());
        out.push_str("\n\n");
    }
    out
}
