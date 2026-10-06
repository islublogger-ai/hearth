//! Full generation-service tests using a named scripted HTTP fixture, never a model.
use super::*;
use serde_json::Value;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Mutex,
    thread,
};

fn fixture(actions: Vec<Value>) -> (EndpointConfig, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = EndpointConfig {
        id: "fixture".into(),
        name: "Scripted test fixture".into(),
        kind: "custom".into(),
        url: format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        ),
        enabled: true,
    };
    let handle = thread::spawn(move || {
        for index in 0..=actions.len() {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let count = socket.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let (content_type, body) = if index == 0 {
                (
                    "application/json",
                    json!({"data":[{"id":"scripted-fixture-model"}]}).to_string(),
                )
            } else {
                let frame = json!({"choices":[{"delta":{"content":actions[index-1].to_string()},"finish_reason":"stop"}],"usage":{"prompt_tokens":99,"completion_tokens":12}});
                (
                    "text/event-stream",
                    format!("data: {frame}\n\ndata: [DONE]\n\n"),
                )
            };
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (endpoint, handle)
}

fn state_request(
    endpoint: EndpointConfig,
    incognito: bool,
) -> (tempfile::TempDir, AppState, GenerationRequest) {
    let dir = tempfile::tempdir().unwrap();
    let store = db::Store::open(&dir.path().join("test.db")).unwrap();
    let settings = Settings {
        workspace: dir.path().to_str().unwrap().into(),
        backends: vec![endpoint],
        default_backend_id: "fixture".into(),
        default_model_id: "scripted-fixture-model".into(),
        ..Settings::default()
    };
    store.save_settings(&settings).unwrap();
    let conversation = Conversation {
        id: uuid::Uuid::new_v4().to_string(),
        title: "Test fixture".into(),
        mode: "agent".into(),
        model_id: settings.default_model_id.clone(),
        backend_id: settings.default_backend_id.clone(),
        pinned: false,
        incognito,
        memory_enabled: !incognito,
        thinking: false,
        created_at: 1,
        updated_at: 1,
        messages: vec![Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: "user".into(),
            content: "Carry out this test task.".into(),
            thinking: String::new(),
            created_at: 1,
            status: "complete".into(),
            stats: None,
            tools: vec![],
            memories_used: vec![],
        }],
    };
    if !incognito {
        store.save_conversation(&conversation).unwrap();
    }
    let state = AppState {
        store: Arc::new(store),
        runs: Arc::new(runs::Runs::default()),
        data_dir: dir.path().into(),
    };
    let request = GenerationRequest {
        run_id: uuid::Uuid::new_v4().to_string(),
        conversation,
        settings,
    };
    (dir, state, request)
}

fn channel(
    state: &AppState,
    request: &GenerationRequest,
    approval: Option<bool>,
    stop_on_approval: bool,
) -> (Channel<StreamEvent>, Arc<Mutex<Vec<Value>>>) {
    let events = Arc::new(Mutex::new(vec![]));
    let output = events.clone();
    let runs = state.runs.clone();
    let id = request.run_id.clone();
    let channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(body) = body {
            let event: Value = serde_json::from_str(&body).unwrap();
            if event["type"] == "approval" {
                if stop_on_approval {
                    runs.stop(&id).unwrap();
                } else if let Some(allow) = approval {
                    runs.approve(&id, event["approvalId"].as_str().unwrap(), allow)
                        .unwrap();
                }
            }
            output.lock().unwrap().push(event);
        }
        Ok(())
    });
    (channel, events)
}

#[tokio::test]
async fn agent_executes_calculator_and_finishes_with_actual_usage() {
    let (endpoint, server) = fixture(vec![
        json!({"tool":"calculator","args":{"expression":"2+2"}}),
        json!({"final":"4"}),
    ]);
    let (_dir, state, mut request) = state_request(endpoint, false);
    request.conversation.backend_id.clear();
    request.conversation.model_id.clear();
    request.settings.default_backend_id = "stale-request-backend".into();
    request.settings.default_model_id = "stale-request-model".into();
    let cancel = state.runs.begin(&request.run_id).unwrap();
    let (channel, events) = channel(&state, &request, None, false);
    let mut partial = Partial::default();
    generate_inner(
        &state,
        &request,
        &channel,
        &cancel,
        Instant::now(),
        &mut partial,
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(partial.content, "4");
    assert_eq!(partial.backend_id, "fixture");
    assert_eq!(partial.model_id, "scripted-fixture-model");
    assert_eq!(partial.prompt_tokens, 99);
    assert_eq!(partial.completion_tokens, 24);
    assert!(!partial.estimated);
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|e| e["type"] == "tool" && e["step"]["output"] == "4"));
    assert_eq!(
        state.store.audit(&request.conversation.id).unwrap().len(),
        1
    );
}
#[tokio::test]
async fn writes_require_exact_approval_and_denials_have_no_effect() {
    for allowed in [false, true] {
        let (endpoint, server) = fixture(vec![
            json!({"tool":"write_file","args":{"path":"output.md","content":"approved content <think>literal tags</think>"}}),
            json!({"final":"Done"}),
        ]);
        let (dir, state, request) = state_request(endpoint, false);
        let cancel = state.runs.begin(&request.run_id).unwrap();
        let (channel, events) = channel(&state, &request, Some(allowed), false);
        let mut partial = Partial::default();
        generate_inner(
            &state,
            &request,
            &channel,
            &cancel,
            Instant::now(),
            &mut partial,
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(dir.path().join("output.md").exists(), allowed);
        if allowed {
            assert_eq!(
                std::fs::read_to_string(dir.path().join("output.md")).unwrap(),
                "approved content <think>literal tags</think>"
            );
        }
        let events = events.lock().unwrap();
        let approval = events.iter().find(|e| e["type"] == "approval").unwrap();
        assert!(approval["preview"]
            .as_str()
            .unwrap()
            .contains("approved content"));
        assert!(state
            .runs
            .approve(
                &request.run_id,
                approval["approvalId"].as_str().unwrap(),
                true
            )
            .is_err());
    }
}
#[tokio::test]
async fn stopping_a_permission_wait_never_writes() {
    let (endpoint, server) = fixture(vec![
        json!({"tool":"write_file","args":{"path":"output.md","content":"should not exist"}}),
    ]);
    let (dir, state, request) = state_request(endpoint, false);
    let cancel = state.runs.begin(&request.run_id).unwrap();
    let (channel, _) = channel(&state, &request, None, true);
    let mut partial = Partial::default();
    assert_eq!(
        generate_inner(
            &state,
            &request,
            &channel,
            &cancel,
            Instant::now(),
            &mut partial
        )
        .await
        .unwrap_err(),
        "cancelled"
    );
    server.join().unwrap();
    assert!(!dir.path().join("output.md").exists());
    assert!(state
        .store
        .audit(&request.conversation.id)
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn incognito_agent_has_no_durable_conversation_memory_or_audit() {
    let (endpoint, server) = fixture(vec![
        json!({"tool":"calculator","args":{"expression":"4*4"}}),
        json!({"final":"16"}),
    ]);
    let (_dir, state, request) = state_request(endpoint, true);
    let cancel = state.runs.begin(&request.run_id).unwrap();
    let (channel, _) = channel(&state, &request, None, false);
    let mut partial = Partial::default();
    generate_inner(
        &state,
        &request,
        &channel,
        &cancel,
        Instant::now(),
        &mut partial,
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert!(state.store.conversations().unwrap().is_empty());
    assert!(state.store.memories().unwrap().is_empty());
    assert!(state
        .store
        .audit(&request.conversation.id)
        .unwrap()
        .is_empty());
}
