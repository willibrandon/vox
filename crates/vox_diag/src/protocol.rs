use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC-inspired error codes for the diagnostics protocol.
pub mod error_code {
    /// Malformed JSON request.
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method name not recognized.
    pub const UNKNOWN_METHOD: i32 = -32601;
    /// Missing or invalid parameters.
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error (pipeline crash, I/O failure).
    pub const INTERNAL_ERROR: i32 = -32603;
    /// App still downloading or loading models.
    pub const NOT_READY: i32 = -32000;
    /// Recording start requested while already recording.
    pub const ALREADY_RECORDING: i32 = -32001;
    /// Recording stop requested while not recording.
    pub const NOT_RECORDING: i32 = -32002;
    /// Maximum concurrent connections reached (4).
    pub const CONNECTION_LIMIT: i32 = -32003;
}

/// A diagnostics protocol request received over UDS.
///
/// Wire format: `{"id":1,"method":"status","params":{...}}\n`
#[derive(Debug, Deserialize)]
pub struct Request {
    /// Correlation ID echoed in the response.
    pub id: u64,
    /// Method name identifying the operation.
    pub method: String,
    /// Optional method-specific parameters.
    #[serde(default)]
    pub params: Option<Value>,
}

/// A diagnostics protocol response sent over UDS.
///
/// Contains either `result` (success) or `error` (failure), never both.
#[derive(Debug, Serialize)]
pub struct Response {
    /// Correlation ID from the request.
    pub id: u64,
    /// Success payload — present only on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error payload — present only on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

impl Response {
    /// Create a success response with the given result value.
    pub fn success(id: u64, value: Value) -> Self {
        Self {
            id,
            result: Some(value),
            error: None,
        }
    }

    /// Create an error response with the given code and message.
    pub fn error(id: u64, code: i32, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(ErrorInfo {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Error details in a diagnostics response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// Numeric error code (see `error_code` constants).
    pub code: i32,
    /// Human-readable error description.
    pub message: String,
}

/// A server-pushed event notification for subscribe connections.
///
/// Wire format: `{"event":"pipeline_state","data":{"state":"listening"}}\n`
/// Events have no `id` field — they are unidirectional notifications.
#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    /// Event type name (e.g. "pipeline_state", "audio_rms", "transcript").
    pub event: String,
    /// Event-specific payload.
    pub data: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_request_with_params() {
        let json = r#"{"id":1,"method":"settings","params":{"action":"get","key":"vad_threshold"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, 1);
        assert_eq!(req.method, "settings");
        assert!(req.params.is_some());
    }

    #[test]
    fn deserialize_request_without_params() {
        let json = r#"{"id":2,"method":"status"}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, 2);
        assert_eq!(req.method, "status");
        assert!(req.params.is_none());
    }

    #[test]
    fn serialize_success_response() {
        let resp = Response::success(1, serde_json::json!({"pid": 12345}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"pid\":12345"));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn serialize_error_response() {
        let resp = Response::error(3, error_code::UNKNOWN_METHOD, "unknown method: bad_method");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"id\":3"));
        assert!(json.contains("-32601"));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn serialize_event() {
        let event = Event {
            event: "audio_rms".into(),
            data: serde_json::json!({"rms": 0.045}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"audio_rms\""));
        assert!(json.contains("\"rms\":0.045"));
    }
}
