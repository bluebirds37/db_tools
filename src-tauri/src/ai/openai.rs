use crate::models::*;
use serde_json::{json, Value};

/// Many OpenAI-compatible gateways reject huge max_tokens or unknown fields.
fn clamp_max_tokens(n: u32) -> u32 {
    n.clamp(1, 32_768)
}

/// 统一角色：system 合并、tool 折进 user、首条不能是 assistant
fn normalize(messages: &[AiMessage]) -> Vec<AiMessage> {
    let mut out: Vec<AiMessage> = Vec::new();
    for m in messages {
        // Some gateways reject empty content；只发图时留空由 content parts 承载
        let text = if m.content.trim().is_empty() {
            if m.images.is_empty() {
                "(empty)".to_string()
            } else {
                String::new()
            }
        } else {
            m.content.clone()
        };
        match m.role.as_str() {
            "system" => {
                if let Some(last) = out.last_mut() {
                    if last.role == "system" {
                        last.content.push('\n');
                        last.content.push_str(&text);
                        continue;
                    }
                }
                out.push(AiMessage::text("system", text));
            }
            "assistant" => out.push(AiMessage::text("assistant", text)),
            // tool role often requires tool_call_id; fold into user for compatibility
            _ => out.push(AiMessage {
                role: "user".into(),
                content: text,
                images: m.images.clone(),
            }),
        }
    }
    // first message must not be assistant for many providers
    if out.first().map(|m| m.role == "assistant").unwrap_or(false) {
        out.insert(0, AiMessage::text("user", "Continue."));
    }
    out
}

/// Chat Completions 的多模态 content：无图时保持纯字符串，兼容性最好
fn chat_content(m: &AiMessage) -> Value {
    if m.images.is_empty() {
        return json!(m.content);
    }
    let mut parts: Vec<Value> = Vec::new();
    if !m.content.trim().is_empty() {
        parts.push(json!({ "type": "text", "text": m.content }));
    }
    for url in &m.images {
        parts.push(json!({ "type": "image_url", "image_url": { "url": url } }));
    }
    Value::Array(parts)
}

/// Responses API 的 input 内容块
fn responses_content(m: &AiMessage) -> Value {
    if m.images.is_empty() {
        return json!(m.content);
    }
    let mut parts: Vec<Value> = Vec::new();
    if !m.content.trim().is_empty() {
        parts.push(json!({ "type": "input_text", "text": m.content }));
    }
    for url in &m.images {
        parts.push(json!({ "type": "input_image", "image_url": url }));
    }
    Value::Array(parts)
}

fn parse_openai_body(val: &Value) -> Result<AiChatResponse, String> {
    let choice = val
        .pointer("/choices/0")
        .cloned()
        .ok_or_else(|| "响应中无 choices".to_string())?;
    let message = choice.get("message").cloned().unwrap_or_default();
    let content = message
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let mut tool_calls = Vec::new();
    if let Some(arr) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in arr {
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let name = tc
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args_str = tc
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let arguments = serde_json::from_str(args_str).unwrap_or(json!({}));
            if !name.is_empty() {
                tool_calls.push(ToolCallRequest {
                    id,
                    name,
                    arguments,
                });
            }
        }
    }
    // Some providers return content as array of parts
    let content = if content.is_empty() {
        message
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    } else {
        content
    };
    let finish = choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("stop")
        .to_string();
    Ok(AiChatResponse {
        content,
        tool_calls,
        finish_reason: finish,
    })
}

fn parse_responses_body(val: &Value) -> Result<AiChatResponse, String> {
    // OpenAI Responses API: output_text or output[].content[].text
    if let Some(t) = val.get("output_text").and_then(|v| v.as_str()) {
        return Ok(AiChatResponse {
            content: t.to_string(),
            tool_calls: vec![],
            finish_reason: "stop".into(),
        });
    }
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    if let Some(out) = val.get("output").and_then(|v| v.as_array()) {
        for item in out {
            let typ = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if typ == "message" || typ.is_empty() {
                if let Some(parts) = item.get("content").and_then(|v| v.as_array()) {
                    for p in parts {
                        if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                            content.push_str(t);
                        }
                    }
                }
            } else if typ == "function_call" {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !name.is_empty() {
                    tool_calls.push(ToolCallRequest {
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments: serde_json::from_str(args).unwrap_or(json!({})),
                    });
                }
            }
        }
    }
    Ok(AiChatResponse {
        content,
        tool_calls,
        finish_reason: "stop".into(),
    })
}

async fn post_json(
    url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(u16, String), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("OpenAI 请求失败: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok((status, text))
}

fn base_trim(settings: &AiSettings) -> String {
    settings.base_url.trim_end_matches('/').to_string()
}

pub async fn complete(
    settings: &AiSettings,
    messages: &[AiMessage],
    tools_json: Option<Value>,
    force_tools: bool,
) -> Result<AiChatResponse, String> {
    let msgs = normalize(messages);
    let max_tokens = clamp_max_tokens(settings.max_tokens);
    let use_responses = settings.api_style == "responses";

    if use_responses {
        // OpenAI Responses API
        let input: Vec<Value> = msgs
            .iter()
            .map(|m| json!({ "role": m.role, "content": responses_content(m) }))
            .collect();
        let mut body = json!({
            "model": settings.model,
            "input": input,
            "max_output_tokens": max_tokens,
        });
        if (0.0..=2.0).contains(&settings.temperature) {
            body["temperature"] = json!(settings.temperature);
        }
        if settings.response_mode == "json_object" {
            body["text"] = json!({ "format": { "type": "json_object" } });
        }
        let base = base_trim(settings);
        let url = if base.ends_with("/v1") {
            format!("{base}/responses")
        } else {
            format!("{base}/v1/responses")
        };
        let (status, text) = post_json(&url, &settings.api_key, &body).await?;
        if status >= 400 {
            return Err(format!("OpenAI Responses API {status}: {text}"));
        }
        let val: Value =
            serde_json::from_str(&text).map_err(|e| format!("响应解析失败: {e}"))?;
        return parse_responses_body(&val);
    }

    // Chat Completions (default) — with progressive fallback for picky gateways
    let api_messages: Vec<Value> = msgs
        .iter()
        .map(|m| json!({ "role": m.role, "content": chat_content(m) }))
        .collect();
    let mut attempts: Vec<Value> = Vec::new();

    let mut full = json!({
        "model": settings.model,
        "messages": api_messages,
        "max_tokens": max_tokens,
    });
    if (0.0..=2.0).contains(&settings.temperature) {
        full["temperature"] = json!(settings.temperature);
    }
    if settings.response_mode == "json_object" {
        full["response_format"] = json!({ "type": "json_object" });
    }
    if let Some(t) = tools_json {
        if t.as_array().map(|a| !a.is_empty()).unwrap_or(false) && settings.enable_tools {
            full["tools"] = t;
            full["tool_choice"] = if force_tools {
                json!("required")
            } else {
                json!("auto")
            };
        }
    }
    attempts.push(full);

    // without tools / response_format
    let mut lean = json!({
        "model": settings.model,
        "messages": api_messages,
        "max_tokens": max_tokens,
        "temperature": settings.temperature.clamp(0.0, 2.0),
    });
    attempts.push(lean.clone());

    // minimal
    lean.as_object_mut().map(|o| {
        o.remove("temperature");
    });
    attempts.push(json!({
        "model": settings.model,
        "messages": api_messages,
        "max_tokens": max_tokens.min(4096),
    }));

    let url = format!("{}/chat/completions", base_trim(settings));
    let mut last_err = String::new();
    for (i, body) in attempts.iter().enumerate() {
        let (status, text) = post_json(&url, &settings.api_key, body).await?;
        if status < 400 {
            let val: Value =
                serde_json::from_str(&text).map_err(|e| format!("响应解析失败: {e}"))?;
            crate::logger::info(
                "openai",
                format!("ok attempt={i} status={status}"),
            );
            return parse_openai_body(&val);
        }
        last_err = format!("OpenAI API {status}: {text}");
        crate::logger::warn(
            "openai",
            format!("attempt={i} failed: {last_err}"),
        );
        // only fall back on 400 / 422
        if status != 400 && status != 422 {
            return Err(last_err);
        }
    }
    Err(last_err)
}
