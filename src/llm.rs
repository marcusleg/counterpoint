//! Minimal blocking client for OpenAI-compatible `/chat/completions` endpoints.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const BODY_EXCERPT_CHARS: usize = 500;
const LIST_MODELS_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LlmError {
    Config(String),
    Connection(String),
    Http { status: u16, body: String },
    MalformedResponse(String),
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Config(message) => write!(f, "{message}"),
            LlmError::Connection(message) => {
                write!(f, "Could not reach the LLM endpoint: {message}")
            }
            LlmError::Http { status, body } => {
                write!(f, "The LLM endpoint returned HTTP {status}: {body}")
            }
            LlmError::MalformedResponse(message) => {
                write!(
                    f,
                    "The LLM endpoint returned an unexpected response: {message}"
                )
            }
        }
    }
}

impl std::error::Error for LlmError {}

#[derive(Serialize)]
struct CompletionRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
}

#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// Sends `messages` to `{base_url}/chat/completions` and returns the first choice's content.
pub fn complete(config: &Config, messages: &[ChatMessage]) -> Result<String, LlmError> {
    let model = config.require_model().map_err(LlmError::Config)?;
    let request = http_client(REQUEST_TIMEOUT)?
        .post(endpoint(config, "chat/completions"))
        .json(&CompletionRequest { model, messages });
    let body = send(authorized(request, config))?;
    let parsed: CompletionResponse =
        serde_json::from_str(&body).map_err(|e| LlmError::MalformedResponse(e.to_string()))?;
    parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .ok_or_else(|| {
            LlmError::MalformedResponse("the response contains no message content".to_string())
        })
}

/// Lists the model IDs from `{base_url}/models`, sorted and de-duplicated.
pub fn list_models(config: &Config) -> Result<Vec<String>, LlmError> {
    let request = http_client(LIST_MODELS_TIMEOUT)?.get(endpoint(config, "models"));
    let body = send(authorized(request, config))?;
    let list: ModelList =
        serde_json::from_str(&body).map_err(|e| LlmError::MalformedResponse(e.to_string()))?;
    let mut ids: Vec<String> = list.data.into_iter().map(|model| model.id).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn http_client(timeout: Duration) -> Result<reqwest::blocking::Client, LlmError> {
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| LlmError::Connection(e.to_string()))
}

fn endpoint(config: &Config, path: &str) -> String {
    format!("{}/{path}", config.base_url.trim_end_matches('/'))
}

fn authorized(
    request: reqwest::blocking::RequestBuilder,
    config: &Config,
) -> reqwest::blocking::RequestBuilder {
    match &config.api_key {
        Some(key) => request.bearer_auth(key),
        None => request,
    }
}

/// Sends the request and returns the body of a successful response.
fn send(request: reqwest::blocking::RequestBuilder) -> Result<String, LlmError> {
    let response = request
        .send()
        .map_err(|e| LlmError::Connection(e.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|e| LlmError::Connection(e.to_string()))?;
    if !status.is_success() {
        return Err(LlmError::Http {
            status: status.as_u16(),
            body: excerpt(&body),
        });
    }
    Ok(body)
}

fn excerpt(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= BODY_EXCERPT_CHARS {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(BODY_EXCERPT_CHARS).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    const OK_BODY: &str =
        r#"{"choices":[{"message":{"role":"assistant","content":"Hello there"}}]}"#;

    fn config(server: &mockito::Server, api_key: Option<&str>) -> Config {
        Config {
            base_url: format!("{}/v1", server.url()),
            api_key: api_key.map(String::from),
            model: Some("test-model".to_string()),
        }
    }

    #[test]
    fn sends_model_and_messages_and_returns_first_choice() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_body(Matcher::Json(serde_json::json!({
                "model": "test-model",
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "hi"}
                ]
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(OK_BODY)
            .create();

        let reply = complete(
            &config(&server, None),
            &[ChatMessage::system("sys"), ChatMessage::user("hi")],
        );

        assert_eq!(reply, Ok("Hello there".to_string()));
        mock.assert();
    }

    #[test]
    fn sends_bearer_token_when_api_key_is_set() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_header("authorization", "Bearer secret")
            .with_status(200)
            .with_body(OK_BODY)
            .create();

        complete(&config(&server, Some("secret")), &[ChatMessage::user("hi")]).unwrap();
        mock.assert();
    }

    #[test]
    fn omits_authorization_header_without_api_key() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_header("authorization", Matcher::Missing)
            .with_status(200)
            .with_body(OK_BODY)
            .create();

        complete(&config(&server, None), &[ChatMessage::user("hi")]).unwrap();
        mock.assert();
    }

    #[test]
    fn tolerates_trailing_slash_in_base_url() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(OK_BODY)
            .create();
        let mut config = config(&server, None);
        config.base_url.push('/');

        complete(&config, &[ChatMessage::user("hi")]).unwrap();
        mock.assert();
    }

    #[test]
    fn non_success_status_becomes_http_error() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(401)
            .with_body("invalid key")
            .create();

        let error = complete(&config(&server, None), &[ChatMessage::user("hi")]).unwrap_err();
        assert_eq!(
            error,
            LlmError::Http {
                status: 401,
                body: "invalid key".to_string()
            }
        );
    }

    #[test]
    fn invalid_json_is_malformed_response() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body("not json")
            .create();

        let error = complete(&config(&server, None), &[ChatMessage::user("hi")]).unwrap_err();
        assert!(matches!(error, LlmError::MalformedResponse(_)), "{error:?}");
    }

    #[test]
    fn missing_content_is_malformed_response() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(r#"{"choices":[]}"#)
            .create();

        let error = complete(&config(&server, None), &[ChatMessage::user("hi")]).unwrap_err();
        assert!(matches!(error, LlmError::MalformedResponse(_)), "{error:?}");
    }

    #[test]
    fn missing_model_fails_without_sending_a_request() {
        let mut server = mockito::Server::new();
        let mock = server.mock("POST", Matcher::Any).expect(0).create();
        let mut config = config(&server, None);
        config.model = None;

        let error = complete(&config, &[ChatMessage::user("hi")]).unwrap_err();
        assert!(matches!(error, LlmError::Config(_)), "{error:?}");
        assert!(error.to_string().contains("Preferences in the main menu"));
        mock.assert();
    }

    #[test]
    fn unreachable_endpoint_is_connection_error() {
        let config = Config {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            api_key: None,
            model: Some("test-model".to_string()),
        };

        let error = complete(&config, &[ChatMessage::user("hi")]).unwrap_err();
        assert!(matches!(error, LlmError::Connection(_)), "{error:?}");
    }

    #[test]
    fn roles_serialize_lowercase() {
        let json = serde_json::to_value(ChatMessage::assistant("x")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"role": "assistant", "content": "x"})
        );
    }

    #[test]
    fn list_models_returns_sorted_unique_ids() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/v1/models")
            .match_header("authorization", "Bearer secret")
            .with_status(200)
            .with_body(r#"{"object":"list","data":[{"id":"b"},{"id":"a"},{"id":"b"}]}"#)
            .create();
        let mut config = config(&server, Some("secret"));
        config.model = None;

        assert_eq!(
            list_models(&config),
            Ok(vec!["a".to_string(), "b".to_string()])
        );
        mock.assert();
    }

    #[test]
    fn list_models_reports_http_errors() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/v1/models")
            .with_status(404)
            .with_body("no such route")
            .create();

        assert_eq!(
            list_models(&config(&server, None)),
            Err(LlmError::Http {
                status: 404,
                body: "no such route".to_string()
            })
        );
    }

    #[test]
    fn list_models_rejects_malformed_responses() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_body(r#"{"models": []}"#)
            .create();

        let error = list_models(&config(&server, None)).unwrap_err();
        assert!(matches!(error, LlmError::MalformedResponse(_)), "{error:?}");
    }
}
