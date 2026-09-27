//! Drives `dev/mock_llm_server.py` through the real HTTP client, so the mock the README
//! recommends for trying the editor is known to speak the protocol the editor expects.

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use counterpoint::config::Config;
use counterpoint::llm;
use counterpoint::prompt::{self, Mode};
use counterpoint::proposal;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dev/mock_llm_server.py");
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const DOCUMENT: &str =
    "# T\n\nA sentence long enough to be chosen.\n\nAnother long sentence for the mock.\n";

/// The running mock server; killed on drop, so a failed assertion cannot leave it behind.
struct MockServer {
    child: Child,
    base_url: String,
}

impl MockServer {
    fn start() -> Self {
        let port = free_port();
        let child = Command::new("python3")
            .arg(SCRIPT)
            .env("COUNTERPOINT_MOCK_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("python3 was found on PATH a moment ago");
        let server = Self {
            child,
            base_url: format!("http://127.0.0.1:{port}/v1"),
        };
        server.wait_until_ready();
        server
    }

    fn wait_until_ready(&self) {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(response) = client.get(format!("{}/models", self.base_url)).send() {
                if response.status().is_success() {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "{SCRIPT} did not answer GET {}/models within {STARTUP_TIMEOUT:?}",
            self.base_url
        );
    }

    fn config(&self, model: &str) -> Config {
        Config {
            base_url: self.base_url.clone(),
            api_key: None,
            model: Some(model.to_string()),
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A port nobody is listening on right now; the listener is dropped so the mock can bind it.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn mock_server_lists_models_and_proposes_a_shouted_edit() {
    if !python3_available() {
        println!("SKIPPED: python3 is not on PATH");
        return;
    }
    let server = MockServer::start();

    let models = llm::list_models(&server.config("mock"))
        .unwrap_or_else(|error| panic!("list_models failed: {error}"));
    assert!(models.contains(&"mock".to_string()), "{models:?}");
    assert!(models.contains(&"mock-stale".to_string()), "{models:?}");

    let messages = prompt::build_messages(Mode::Ghostwriting, DOCUMENT, None, &[], "shout");
    let reply = llm::complete(&server.config("mock"), &messages)
        .unwrap_or_else(|error| panic!("complete failed: {error}"));

    let parsed = proposal::parse(&reply).unwrap_or_else(|error| panic!("{error}: {reply}"));
    assert_eq!(parsed.edits.len(), 2, "{reply}");
    let applied = proposal::apply(DOCUMENT, &parsed.edits)
        .unwrap_or_else(|error| panic!("apply failed: {error}"));
    assert_eq!(
        applied,
        "# T\n\nA SENTENCE LONG ENOUGH TO BE CHOSEN.\n\nANOTHER LONG SENTENCE FOR THE MOCK.\n"
    );
}
