use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{self, BufRead};

pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    Text(String),
}

#[derive(Clone, Debug)]
pub enum Message {
    Response {
        id: u64,
        result: Result<Value, String>,
    },
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

pub fn parse(line: &[u8]) -> Result<Message, String> {
    let value: Value = serde_json::from_slice(line)
        .map_err(|e| format!("协议不是有效 JSON：{e}。启动器 stdout 必须只输出 JSON-RPC。"))?;
    if !value.is_object() {
        return Err("协议消息必须是对象".into());
    }
    if let Some(method) = value.get("method").and_then(Value::as_str) {
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        return match value.get("id") {
            Some(id) if id.is_string() || id.is_i64() => Ok(Message::Request {
                id: id.clone(),
                method: method.into(),
                params,
            }),
            Some(_) => Err("服务端请求 ID 必须是字符串或整数".into()),
            None => Ok(Message::Notification {
                method: method.into(),
                params,
            }),
        };
    }
    let id = value
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("响应没有有效的客户端请求 ID")?;
    let result = if let Some(error) = value.get("error") {
        Err(format!(
            "RPC {}：{}",
            error["code"],
            error["message"].as_str().unwrap_or("后端请求失败")
        ))
    } else {
        Ok(value
            .get("result")
            .cloned()
            .ok_or("响应缺少 result/error")?)
    };
    Ok(Message::Response { id, result })
}

pub fn read_message(reader: &mut impl BufRead) -> Result<Option<Message>, String> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(|e| e.to_string())?;
        if chunk.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            return Err("后端在 JSON 行结束前断开".into());
        }
        let count = chunk
            .iter()
            .position(|&b| b == b'\n')
            .map_or(chunk.len(), |n| n + 1);
        if bytes.len() + count > MAX_MESSAGE_BYTES {
            return Err("单条协议消息超过 16 MiB，已停止连接；未把截断消息当作完整结果".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        reader.consume(count);
        if bytes.last() == Some(&b'\n') {
            if bytes.iter().all(u8::is_ascii_whitespace) {
                bytes.clear();
                continue;
            }
            return parse(&bytes).map(Some);
        }
    }
}

pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "id": id, "method": method, "params": params })
}

pub fn initialize() -> Value {
    json!({
        "clientInfo": { "name": "magicodex", "title": "Magicodex", "version": env!("CARGO_PKG_VERSION") },
        "capabilities": { "experimentalApi": false }
    })
}

pub fn io_error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn split_utf8_and_coalesced_lines() {
        let wire =
            "{\"method\":\"test\",\"params\":{\"text\":\"魔法\"}}\n{\"id\":1,\"result\":{}}\n";
        let mut reader = BufReader::with_capacity(1, Cursor::new(wire));
        assert!(
            matches!(read_message(&mut reader).unwrap(), Some(Message::Notification { params, .. }) if params["text"] == "魔法")
        );
        assert!(matches!(
            read_message(&mut reader).unwrap(),
            Some(Message::Response { id: 1, .. })
        ));
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn rejects_pollution_and_partial_messages() {
        assert!(parse(b"Starting bridge...").is_err());
        assert!(parse(b"[]").is_err());
        assert!(read_message(&mut Cursor::new(b"{\"id\":1}")).is_err());
    }

    #[test]
    fn server_request_keeps_string_id() {
        assert!(
            matches!(parse(br#"{"id":"ask-1","method":"item/tool/requestUserInput","params":{}}"#).unwrap(),
            Message::Request { id, .. } if id == "ask-1")
        );
    }
}
