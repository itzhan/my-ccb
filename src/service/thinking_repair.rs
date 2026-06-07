//! thinking 块 400 整流：检测上游因 thinking/signature/空块/工具签名导致的 400,
//! 过滤/降级请求体后重发。移植自 sub2api 的请求整流器
//! (FilterThinkingBlocksForRetry / FilterSignatureSensitiveBlocksForRetry)。

use serde_json::{json, Value};

/// 取上游错误体里的 `error.message`(回退顶层 `message`),小写 + trim。
fn upstream_error_message(resp_body: &[u8]) -> String {
    let v: Value = match serde_json::from_slice(resp_body) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    v.get("error")
        .and_then(|e| e.get("message"))
        .or_else(|| v.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// 是否为 thinking 块 / signature / 空内容类 400(可通过过滤 thinking 块修复)。
pub fn is_thinking_signature_error(resp_body: &[u8]) -> bool {
    let msg = upstream_error_message(resp_body);
    if msg.is_empty() {
        return false;
    }
    // "Invalid `signature` in `thinking` block" 等
    if msg.contains("signature") {
        return true;
    }
    // "Expected `thinking` or `redacted_thinking`, but found `text`"
    if msg.contains("expected") && (msg.contains("thinking") || msg.contains("redacted_thinking")) {
        return true;
    }
    // "thinking ... cannot be modified"
    if msg.contains("cannot be modified") && (msg.contains("thinking") || msg.contains("redacted_thinking")) {
        return true;
    }
    // "all messages must have non-empty content" / "text content blocks must be non-empty"
    if msg.contains("non-empty content")
        || msg.contains("empty content")
        || msg.contains("content blocks must be non-empty")
    {
        return true;
    }
    // "each thinking block must contain thinking"
    if msg.contains("thinking block must contain") {
        return true;
    }
    false
}

/// 是否为 tool/function 签名类 400(thinking 过滤后仍 400 时,据此判断是否进二段降级)。
pub fn looks_like_tool_signature_error(resp_body: &[u8]) -> bool {
    let msg = upstream_error_message(resp_body);
    msg.contains("tool_use")
        || msg.contains("tool_result")
        || msg.contains("function_call")
        || msg.contains("functioncall")
        || msg.contains("function_response")
        || msg.contains("functionresponse")
}

/// 一段:过滤/降级 thinking 块(关闭顶层 thinking、thinking→text、删 redacted_thinking/空块)。
pub fn filter_thinking_blocks_for_retry(body: &[u8]) -> Vec<u8> {
    transform_for_retry(body, false)
}

/// 二段:在一段基础上,额外把 tool_use / tool_result 块降级为 text。
pub fn filter_signature_sensitive_blocks_for_retry(body: &[u8]) -> Vec<u8> {
    transform_for_retry(body, true)
}

fn transform_for_retry(body: &[u8], downgrade_tools: bool) -> Vec<u8> {
    let mut v: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return body.to_vec(),
    };
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return body.to_vec(),
    };

    let mut modified = false;

    // 删顶层 thinking(关闭 extended thinking)+ 依赖 thinking 的 context_management 策略。
    if obj.remove("thinking").is_some() {
        modified = true;
        remove_thinking_context_strategies(obj);
    }

    if let Some(msgs) = obj.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in msgs.iter_mut() {
            if transform_message(msg, downgrade_tools) {
                modified = true;
            }
        }
    }

    if !modified {
        return body.to_vec();
    }
    serde_json::to_vec(&v).unwrap_or_else(|_| body.to_vec())
}

/// 删除依赖 thinking 的 context_management 策略(如 clear_thinking_20251015),
/// 否则去掉 thinking 后上游会回 "strategy requires thinking to be enabled or adaptive"。
fn remove_thinking_context_strategies(obj: &mut serde_json::Map<String, Value>) {
    if let Some(cm) = obj.get_mut("context_management").and_then(|c| c.as_object_mut()) {
        if let Some(edits) = cm.get_mut("edits").and_then(|e| e.as_array_mut()) {
            edits.retain(|edit| {
                edit.get("type").and_then(|t| t.as_str()) != Some("clear_thinking_20251015")
            });
            if edits.is_empty() {
                cm.remove("edits");
            }
        }
    }
}

/// 改写一条 message 的 content 块;返回是否改动过。
fn transform_message(msg: &mut Value, downgrade_tools: bool) -> bool {
    let role = msg
        .get("role")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();
    // content 非数组(字符串等)直接不动。
    let content = match msg.get("content").and_then(|c| c.as_array()) {
        Some(c) => c.clone(),
        None => return false,
    };

    let mut new_content: Vec<Value> = Vec::with_capacity(content.len());
    let mut changed = false;

    for block in content.into_iter() {
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match block_type {
            // 空 text 块 → 删(上游会拒 "text content blocks must be non-empty")
            "text" => {
                if block.get("text").and_then(|t| t.as_str()).unwrap_or("").is_empty() {
                    changed = true;
                    continue;
                }
                new_content.push(block);
            }
            // thinking 块 → 转 text(保留内容);空则删
            "thinking" => {
                changed = true;
                let t = block.get("thinking").and_then(|x| x.as_str()).unwrap_or("");
                if !t.is_empty() {
                    new_content.push(json!({"type": "text", "text": t}));
                }
            }
            // redacted_thinking → 删(无法转 text)
            "redacted_thinking" => {
                changed = true;
            }
            // 二段:工具块降级为 text
            "tool_use" if downgrade_tools => {
                changed = true;
                new_content.push(tool_use_to_text(&block));
            }
            "tool_result" if downgrade_tools => {
                changed = true;
                new_content.push(tool_result_to_text(&block));
            }
            // 一段:tool_result 的嵌套 content 递归删空 text 块
            "tool_result" => {
                if let Some(nested) = block.get("content").and_then(|c| c.as_array()) {
                    let (cleaned, nested_changed) = strip_empty_text_blocks(nested);
                    if nested_changed {
                        changed = true;
                        let mut bc = block.clone();
                        bc["content"] = Value::Array(cleaned);
                        new_content.push(bc);
                        continue;
                    }
                }
                new_content.push(block);
            }
            // 无 type 但带 thinking 字段 → 转 text
            "" => {
                if let Some(raw) = block.get("thinking") {
                    changed = true;
                    let text = match raw {
                        Value::String(s) => s.clone(),
                        other => serde_json::to_string(other).unwrap_or_default(),
                    };
                    if !text.is_empty() {
                        new_content.push(json!({"type": "text", "text": text}));
                    }
                } else {
                    new_content.push(block);
                }
            }
            _ => new_content.push(block),
        }
    }

    if !changed {
        return false;
    }

    // content 被清空 → 填占位,避免 "messages must have non-empty content"
    if new_content.is_empty() {
        let placeholder = if role == "assistant" {
            "(assistant content removed)"
        } else {
            "(content removed)"
        };
        msg["content"] = json!([{"type": "text", "text": placeholder}]);
    } else {
        msg["content"] = Value::Array(new_content);
    }
    true
}

fn strip_empty_text_blocks(blocks: &[Value]) -> (Vec<Value>, bool) {
    let mut out = Vec::with_capacity(blocks.len());
    let mut changed = false;
    for b in blocks {
        if b.get("type").and_then(|t| t.as_str()) == Some("text")
            && b.get("text").and_then(|t| t.as_str()).unwrap_or("").is_empty()
        {
            changed = true;
            continue;
        }
        out.push(b.clone());
    }
    (out, changed)
}

fn tool_use_to_text(block: &Value) -> Value {
    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
    let input = block.get("input").map(|i| i.to_string()).unwrap_or_default();
    json!({"type": "text", "text": format!("[tool_use name={} id={} input={}]", name, id, input)})
}

fn tool_result_to_text(block: &Value) -> Value {
    let id = block.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("");
    let content_text = match block.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    json!({"type": "text", "text": format!("[tool_result tool_use_id={} {}]", id, content_text)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_signature_errors() {
        let cases = [
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"Invalid `signature` in `thinking` block"}}"#,
            r#"{"error":{"message":"Expected `thinking` or `redacted_thinking`, but found `text`"}}"#,
            r#"{"error":{"message":"messages.1.content.0.thinking: each thinking block must contain thinking"}}"#,
            r#"{"error":{"message":"text content blocks must be non-empty"}}"#,
        ];
        for c in cases {
            assert!(is_thinking_signature_error(c.as_bytes()), "should match: {c}");
        }
        assert!(!is_thinking_signature_error(br#"{"error":{"message":"model not found"}}"#));
    }

    #[test]
    fn filters_thinking_blocks() {
        let body = br#"{"thinking":{"type":"enabled"},"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"hmm","signature":"x"},{"type":"text","text":"hi"}]}]}"#;
        let out = filter_thinking_blocks_for_retry(body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("thinking").is_none(), "top-level thinking removed");
        let blocks = v["messages"][0]["content"].as_array().unwrap();
        // thinking → text("hmm"), 原 text("hi") 保留
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "hmm");
        assert_eq!(blocks[1]["text"], "hi");
    }

    #[test]
    fn empty_content_gets_placeholder() {
        let body = br#"{"messages":[{"role":"assistant","content":[{"type":"redacted_thinking","data":"x"}]}]}"#;
        let out = filter_thinking_blocks_for_retry(body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        let blocks = v["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["text"], "(assistant content removed)");
    }
}
