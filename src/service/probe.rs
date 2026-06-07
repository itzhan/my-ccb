use std::collections::HashMap;

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// 探针识别 header：探活系统在请求头加 `X-Probe-Check: 1` 即被网关短路,
/// 不转发上游、不选号、不计费。
const PROBE_HEADER: &str = "X-Probe-Check";

/// 号池健康时返回的固定文案。
const PROBE_MESSAGE: &str =
    "👋 已收到探针请求，API 网关运行正常 / API gateway is healthy. 如需开始对话，请直接发送您的具体问题。";

/// 是否为探针请求（大小写不敏感判断 `X-Probe-Check: 1`）。
pub fn is_probe(headers: &HashMap<String, String>) -> bool {
    headers
        .get(PROBE_HEADER)
        .or_else(|| headers.get("x-probe-check"))
        .map(|v| v.trim() == "1")
        .unwrap_or(false)
}

/// 探针响应：`healthy=true`（号池有可调度账号）回 200 + 固定文案；
/// `healthy=false`（一个可用账号都没有）回 503 错误体,供探活系统判定不健康。
pub fn probe_response(is_stream: bool, model: &str, healthy: bool) -> Response {
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

    let model = if model.is_empty() { "claude-sonnet-4-5" } else { model };

    if is_stream {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .body(Body::from(healthy_stream_sse(model)))
            .expect("build probe sse response")
    } else {
        axum::Json(json!({
            "id": "msg_probe",
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{ "type": "text", "text": PROBE_MESSAGE }],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
        .into_response()
    }
}

/// 构建 Anthropic `/v1/messages` 流式 SSE（6 个标准事件,文案放在单个 text_delta 里）。
fn healthy_stream_sse(model: &str) -> String {
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
                "delta": { "type": "text_delta", "text": PROBE_MESSAGE }
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
