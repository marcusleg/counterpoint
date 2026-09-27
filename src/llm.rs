//! Minimal blocking client for OpenAI-compatible `/chat/completions` endpoints.

use std::fmt;
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// How long a chat completion may take in total. A local model rewriting a long document can
/// easily need minutes, so this is generous; the connect timeout below catches unreachable
/// endpoints quickly.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const LIST_MODELS_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The largest response body read from the endpoint; anything longer is an error rather than a
/// memory hog in the chat history.
pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const BODY_EXCERPT_CHARS: usize = 500;

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
    /// The request took longer than the given number of seconds.
    Timeout(u64),
    Http {
        status: u16,
        body: String,
    },
    MalformedResponse(String),
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Config(message) => write!(f, "{message}"),
            LlmError::Connection(message) => {
                write!(f, "Could not reach the LLM endpoint: {message}")
            }
            LlmError::Timeout(seconds) => {
                write!(
                    f,
                    "The LLM endpoint did not answer within {seconds} seconds."
                )
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
    content: Option<Content>,
}

/// Message content is a string, but some servers send an array of typed parts instead.
#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Deserialize)]
struct ContentPart {
    #[serde(default)]
    text: String,
}

impl Content {
    fn into_text(self) -> String {
        match self {
            Content::Text(text) => text,
            Content::Parts(parts) => parts.into_iter().map(|part| part.text).collect(),
        }
    }
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
    let request = http_client()
        .post(endpoint(config, "chat/completions")?)
        .timeout(REQUEST_TIMEOUT)
        .json(&CompletionRequest { model, messages });
    let body = send(authorized(request, config), REQUEST_TIMEOUT)?;
    let parsed: CompletionResponse =
        serde_json::from_str(&body).map_err(|e| LlmError::MalformedResponse(e.to_string()))?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .map(Content::into_text)
        .map(without_nul)
        .ok_or_else(|| {
            LlmError::MalformedResponse("the response contains no message content".to_string())
        })?;
    if content.trim().is_empty() {
        return Err(LlmError::MalformedResponse(
            "the reply is empty".to_string(),
        ));
    }
    Ok(content)
}

/// Lists the model IDs from `{base_url}/models`, sorted and de-duplicated.
pub fn list_models(config: &Config) -> Result<Vec<String>, LlmError> {
    let request = http_client()
        .get(endpoint(config, "models")?)
        .timeout(LIST_MODELS_TIMEOUT);
    let body = send(authorized(request, config), LIST_MODELS_TIMEOUT)?;
    let list: ModelList =
        serde_json::from_str(&body).map_err(|e| LlmError::MalformedResponse(e.to_string()))?;
    let mut ids: Vec<String> = list
        .data
        .into_iter()
        .map(|model| without_nul(model.id))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// One client for the process: each `reqwest::blocking::Client` owns a runtime thread, so
/// building one per request would spawn and tear down a thread every time. Per-request
/// budgets are set on the requests; the client itself only limits connecting.
fn http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(None)
            .build()
            .expect("the default TLS backend is available")
    })
}

/// `{base_url}/{path}`, with the base URL checked as the Preferences dialog checks it.
fn endpoint(config: &Config, path: &str) -> Result<reqwest::Url, LlmError> {
    let mut base = Config::parse_base_url(&config.base_url).map_err(LlmError::Config)?;
    if !base.path().ends_with('/') {
        let with_slash = format!("{}/", base.path());
        base.set_path(&with_slash);
    }
    base.join(path)
        .map_err(|e| LlmError::Config(format!("The base URL is not usable: {e}")))
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

/// GTK labels cannot hold NUL characters (glib panics on them), so they are dropped from
/// everything the endpoint sends, both raw (error bodies) and JSON-decoded (`"\\u0000"`).
fn without_nul(text: String) -> String {
    if text.contains('\0') {
        text.replace('\0', "")
    } else {
        text
    }
}

/// Sends the request and returns the body of a successful response, capped at
/// `MAX_RESPONSE_BYTES` and stripped of NUL characters.
fn send(request: reqwest::blocking::RequestBuilder, timeout: Duration) -> Result<String, LlmError> {
    let mut response = request.send().map_err(|e| connection_error(e, timeout))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            match e
                .into_inner()
                .and_then(|e| e.downcast::<reqwest::Error>().ok())
            {
                Some(e) => connection_error(*e, timeout),
                None => LlmError::Connection("the response could not be read".to_string()),
            }
        })?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(too_large());
    }
    let body = without_nul(String::from_utf8_lossy(&bytes).into_owned());
    if !status.is_success() {
        return Err(LlmError::Http {
            status: status.as_u16(),
            body: excerpt(&body),
        });
    }
    Ok(body)
}

fn too_large() -> LlmError {
    LlmError::MalformedResponse(format!(
        "the reply is larger than {} MiB",
        MAX_RESPONSE_BYTES / (1024 * 1024)
    ))
}

/// Maps a transport error, telling a timeout apart from an unreachable endpoint and hiding any
/// credentials the user may have typed into the URL, which reqwest would otherwise print.
fn connection_error(mut error: reqwest::Error, timeout: Duration) -> LlmError {
    if error.is_timeout() {
        return LlmError::Timeout(timeout.as_secs());
    }
    if let Some(url) = error.url_mut() {
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }
    LlmError::Connection(error.to_string())
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
    #[test]
    fn nul_characters_are_removed_from_replies() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(r#"{"choices":[{"message":{"content":"Hel\u0000lo"}}]}"#)
            .create();

        let reply = complete(&config(&server, None), &[ChatMessage::user("hi")]);
        assert_eq!(reply, Ok("Hello".to_string()));
    }

    #[test]
    fn nul_characters_are_removed_from_error_bodies() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(500)
            .with_body(b"bo\0om".as_slice())
            .create();

        let error = complete(&config(&server, None), &[ChatMessage::user("hi")]).unwrap_err();
        assert_eq!(
            error,
            LlmError::Http {
                status: 500,
                body: "boom".to_string()
            }
        );
    }

    #[test]
    fn content_parts_are_joined() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(
                r#"{"choices":[{"message":{"content":[{"type":"text","text":"Hel"},{"type":"text","text":"lo"}]}}]}"#,
            )
            .create();

        let reply = complete(&config(&server, None), &[ChatMessage::user("hi")]);
        assert_eq!(reply, Ok("Hello".to_string()));
    }

    #[test]
    fn null_content_is_malformed_response() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(r#"{"choices":[{"message":{"content":null}}]}"#)
            .create();

        let error = complete(&config(&server, None), &[ChatMessage::user("hi")]).unwrap_err();
        assert!(matches!(error, LlmError::MalformedResponse(_)), "{error:?}");
    }

    #[test]
    fn empty_content_is_malformed_response() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body(r#"{"choices":[{"message":{"content":"  \n"}}]}"#)
            .create();

        let error = complete(&config(&server, None), &[ChatMessage::user("hi")]).unwrap_err();
        assert_eq!(
            error,
            LlmError::MalformedResponse("the reply is empty".to_string())
        );
    }

    #[test]
    fn oversized_reply_is_rejected_while_reading() {
        let mut server = mockito::Server::new();
        let body = "x".repeat(MAX_RESPONSE_BYTES as usize + 1);
        server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_chunked_body(move |w| w.write_all(body.as_bytes()))
            .create();

        let error = list_models(&config(&server, None)).unwrap_err();
        assert!(error.to_string().contains("larger than 8 MiB"), "{error}");
    }

    #[test]
    fn base_url_with_a_query_is_a_config_error() {
        let config = Config {
            base_url: "http://127.0.0.1:1/v1?x=1".to_string(),
            api_key: None,
            model: Some("m".to_string()),
        };
        let error = complete(&config, &[ChatMessage::user("hi")]).unwrap_err();
        assert!(matches!(error, LlmError::Config(_)), "{error:?}");
    }

    #[test]
    fn credentials_in_the_base_url_are_rejected_before_sending() {
        let config = Config {
            base_url: "http://user:secret@127.0.0.1:1/v1".to_string(),
            api_key: None,
            model: Some("m".to_string()),
        };
        let error = complete(&config, &[ChatMessage::user("hi")]).unwrap_err();
        assert!(matches!(error, LlmError::Config(_)), "{error:?}");
        assert!(!error.to_string().contains("secret"), "{error}");
    }

    #[test]
    fn timeout_has_its_own_message() {
        assert_eq!(
            LlmError::Timeout(30).to_string(),
            "The LLM endpoint did not answer within 30 seconds."
        );
    }
}
