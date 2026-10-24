//! 启动器 v2 的独立 WS 信封；不接受旧 SSE 报文或任意字段。
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Incoming {
    Request {
        id: String,
        method: String,
        params: Value,
        deadline: Option<f64>,
    },
    Response {
        id: String,
        ok: bool,
        result: Option<Value>,
        error: Option<ApiFailure>,
    },
    Event {
        id: String,
        topic: String,
        data: Value,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiFailure {
    pub code: String,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutostartParams {
    pub enabled: bool,
}

pub fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 100
}

pub fn valid_ticket(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}
