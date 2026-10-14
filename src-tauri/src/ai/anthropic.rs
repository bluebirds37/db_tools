use crate::models::*;
use serde_json::{json, Value};

fn clamp_max_tokens(n: u32) -> u32 {
    n.clamp(1, 32_768)
}

fn messages_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/messages")
    } else if b.ends_with("/messages") {
        b.to_string()
    } else {
        format!("{b}/v1/messages")
    }
}

/// data URL → Anthropic 的 base64 image block；非图片前缀直接忽略
fn image_block(url: &str) -> Option<Value> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    if !mime.starts_with("image/") || data.is_empty() {
        return None;
    }
    Some(json!({
        "type": "image",
        "source": { "type": "base64", "media_type": mime, "data": data }
    }))
}

fn user_content(m: &AiMessage) -> Value {
    let mut parts: Vec<Value> = Vec::new();
    if !m.content.trim().is_empty() {
        parts.push(json!({ "type": "text", "text": m.content }));
    }
    for url in &m.images {
        if let Some(b) = image_block(url) {
            parts.push(b);
        }
    }
    if parts.is_empty() {
        return json!("(empty)");
    }
    Value::Array(parts)
}

pub async fn complete(
    settings: &AiSettings,
    messages: &[AiMessage],
    tools_json: Option<Value>,
    force_tools: bool,
) -> Result<AiChatResponse, String> {
    let mut system = String::new();
    let mut api_messages = Vec::new();
    for m in messages {
        let text = if m.content.trim().is_empty() {
            "(empty)".to_string()
        } else {
            m.content.clone()
        };
        match m.role.as_str() {
            "system" => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&text);
            }
            "assistant" => api_messages.push(json!({ "role": "assistant", "content": text })),
            _ => api_messages.push(json!({ "role": "user", "content": user_content(m) })),
        }
    }

    let mut body = json!({
        "model": settings.model,
        "max_tokens": clamp_max_tokens(settings.max_tokens),
        "messages": api_messages,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if (0.0..=1.0).contains(&settings.temperature) {
        body["temperature"] = json!(settings.temperature.min(1.0));
    }
    if settings.response_mode == "json_object" {
        // Anthropic has no standard json_object; leave text only
    }
    if let Some(t) = tools_json {
        if t.as_array().map(|a| !a.is_empty()).unwrap_or(false) && settings.enable_tools {
            body["tools"] = t;
            if force_tools {
                body["tool_choice"] = json!({ "type": "any" });
            }
        }
    }

    let url = messages_url(&settings.base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let mut attempts = vec![body.clone()];
    // fallback without tools / temperature
    let mut lean = body.clone();
    if let Some(obj) = lean.as_object_mut() {
        obj.remove("tools");
        obj.remove("temperature");
    }
    attempts.push(lean);

    let mut last_err = String::new();
    for (i, b) in attempts.iter().enumerate() {
        let resp = client
            .post(&url)
            .header("x-api-key", &settings.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(b)
            .send()
            .await
            .map_err(|e| format!("Anthropic 请求失败: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if status < 400 {
            let val: Value =
                serde_json::from_str(&text).map_err(|e| format!("响应解析失败: {e}"))?;
            let mut content = String::new();
            let mut tool_calls = Vec::new();
            if let Some(blocks) = val.get("content").and_then(|v| v.as_array()) {
                for blk in blocks {
                    let btype = blk.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if btype == "text" {
                        content.push_str(blk.get("text").and_then(|v| v.as_str()).unwrap_or(""));
                    } else if btype == "tool_use" {
                        tool_calls.push(ToolCallRequest {
                            id: blk.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            name: blk.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            arguments: blk.get("input").cloned().unwrap_or(json!({})),
                        });
                    }
                }
            }
            let finish = val
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("end_turn")
                .to_string();
            crate::logger::info("anthropic", format!("ok attempt={i}"));
            return Ok(AiChatResponse {
                content,
                tool_calls,
                finish_reason: finish,
            });
        }
        last_err = format!("Anthropic API {status}: {text}");
        crate::logger::warn("anthropic", format!("attempt={i}: {last_err}"));
        if status != 400 && status != 422 {
            return Err(last_err);
        }
    }
    Err(last_err)
}
