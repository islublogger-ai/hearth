use hearth_lib::{
    models::{EndpointConfig, PromptMessage},
    network,
};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn request(stream: &mut TcpStream) -> serde_json::Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut data = vec![];
    let mut part = [0; 1024];
    loop {
        let count = stream.read(&mut part).unwrap();
        if count == 0 {
            break;
        }
        data.extend_from_slice(&part[..count]);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..end]);
            let len = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + len {
                return serde_json::from_slice(&data[end + 4..end + 4 + len]).unwrap();
            }
        }
    }
    panic!("No request body")
}
fn endpoint(port: u16) -> EndpointConfig {
    EndpointConfig {
        id: "mock".into(),
        name: "Scripted test server".into(),
        kind: "custom".into(),
        url: format!("http://127.0.0.1:{port}/v1"),
        enabled: true,
    }
}
fn messages() -> Vec<PromptMessage> {
    vec![PromptMessage {
        role: "user".into(),
        content: "hello".into(),
    }]
}

#[tokio::test]
async fn real_http_fragmented_sse_thinking_and_usage() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = endpoint(listener.local_addr().unwrap().port());
    let captured = Arc::new(Mutex::new(serde_json::Value::Null));
    let capture = captured.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        *capture.lock().unwrap() = request(&mut stream);
        let payload=concat!("data: {\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\r\n\r\n","data: {\"choices\":[{\"delta\":{\"content\":\"nk>private</think>Hello 🔥\"}}]}\r\n\r\n","data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":21,\"completion_tokens\":9}}\n\n","data: [DONE]\n\n");
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",payload.len()).unwrap();
        for chunk in payload.as_bytes().chunks(3) {
            stream.write_all(chunk).unwrap();
        }
    });
    let mut text = String::new();
    let mut thought = String::new();
    let result = network::chat(
        &ep,
        "test-model",
        &messages(),
        network::ChatOptions {
            temperature: 0.6,
            max_tokens: 128,
            ..Default::default()
        },
        &CancellationToken::new(),
        |a, b| {
            text.push_str(&a);
            thought.push_str(&b)
        },
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(text, "Hello 🔥");
    assert_eq!(thought, "private");
    assert_eq!(result.content, text);
    assert_eq!(result.prompt_tokens, Some(21));
    assert_eq!(result.completion_tokens, Some(9));
    let captured = captured.lock().unwrap();
    assert_eq!(captured["model"], "test-model");
    assert_eq!(captured["max_tokens"], 128);
    assert_eq!(captured["temperature"], 0.6);
}
#[tokio::test]
async fn a_disconnect_keeps_partial_text_but_is_an_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = endpoint(listener.local_addr().unwrap().port());
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        request(&mut s);
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n";
        write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
        )
        .unwrap();
    });
    let mut partial = String::new();
    let result = network::chat(
        &ep,
        "test",
        &messages(),
        network::ChatOptions {
            max_tokens: 128,
            ..Default::default()
        },
        &CancellationToken::new(),
        |a, _| partial.push_str(&a),
    )
    .await;
    server.join().unwrap();
    assert!(result.unwrap_err().contains("before finishing"));
    assert_eq!(partial, "partial");
}
#[tokio::test]
async fn cancellation_interrupts_prefill_before_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = endpoint(listener.local_addr().unwrap().port());
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        request(&mut s);
        thread::sleep(Duration::from_millis(250));
    });
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        token.cancel();
    });
    let start = std::time::Instant::now();
    let result = network::chat(
        &ep,
        "test",
        &messages(),
        network::ChatOptions {
            max_tokens: 128,
            ..Default::default()
        },
        &cancel,
        |_, _| {},
    )
    .await;
    assert_eq!(result.unwrap_err(), "cancelled");
    assert!(start.elapsed() < Duration::from_millis(200));
    server.join().unwrap();
}
#[tokio::test]
async fn malformed_packets_and_native_tool_calls_fail_closed() {
    for payload in ["data: {bad json}\n\n", "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"{}\"}}]}}]}\n\n"] {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let ep=endpoint(listener.local_addr().unwrap().port());let payload=payload.to_string();let server=thread::spawn(move||{let(mut s,_)=listener.accept().unwrap();request(&mut s);write!(s,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{payload}",payload.len()).unwrap();});assert!(network::chat(&ep,"test",&messages(),network::ChatOptions{max_tokens:128,..Default::default()},&CancellationToken::new(),|_,_|{}).await.is_err());server.join().unwrap();
    }
}

#[tokio::test]
async fn json_actions_preserve_literal_thinking_tags_across_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = endpoint(listener.local_addr().unwrap().port());
    let expected = serde_json::json!({"tool":"write_file","args":{"path":"note.txt","content":"<think>literal</think>"}}).to_string();
    let content = expected.clone();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let body = request(&mut socket);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        let mut payload = String::new();
        for part in content.as_bytes().chunks(3) {
            let frame = serde_json::json!({"choices":[{"delta":{"content":std::str::from_utf8(part).unwrap()}}]});
            payload.push_str(&format!("data: {frame}\n\n"));
        }
        payload.push_str(
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        );
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).unwrap();
    });
    let result = network::chat(
        &ep,
        "qwen3-test",
        &messages(),
        network::ChatOptions {
            json_actions: true,
            max_tokens: 128,
            ..Default::default()
        },
        &CancellationToken::new(),
        |_, _| {},
    )
    .await
    .unwrap();
    server.join().unwrap();
    assert_eq!(result.content, expected);
    assert!(result.thinking.is_empty());
}

#[tokio::test]
async fn ollama_qwen_uses_supported_reasoning_controls() {
    for thinking in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut ep = endpoint(listener.local_addr().unwrap().port());
        ep.kind = "ollama".into();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let body = request(&mut socket);
            assert_eq!(
                body["reasoning_effort"],
                if thinking { "high" } else { "none" }
            );
            assert!(body.get("chat_template_kwargs").is_none());
            assert!(body.get("top_k").is_none());
            let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            )
            .unwrap();
        });
        network::chat(
            &ep,
            "qwen3-test",
            &messages(),
            network::ChatOptions {
                thinking,
                ..Default::default()
            },
            &CancellationToken::new(),
            |_, _| {},
        )
        .await
        .unwrap();
        server.join().unwrap();
    }
}

#[tokio::test]
async fn output_limit_is_reported_with_partial_text_preserved() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = endpoint(listener.local_addr().unwrap().port());
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        request(&mut socket);
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"partial answer\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
        .unwrap();
    });
    let mut text = String::new();
    let error = network::chat(
        &ep,
        "test",
        &messages(),
        network::ChatOptions::default(),
        &CancellationToken::new(),
        |part, _| text.push_str(&part),
    )
    .await
    .unwrap_err();
    server.join().unwrap();
    assert_eq!(text, "partial answer");
    assert!(error.contains("output-token limit"));
}

#[tokio::test]
async fn oversized_frame_never_emits_an_unpersistable_partial() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = endpoint(listener.local_addr().unwrap().port());
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap(); request(&mut socket);
        let first = serde_json::json!({"choices":[{"delta":{"content":"kept prefix"}}]});
        let oversized = serde_json::json!({"choices":[{"delta":{"content":"x".repeat(1_000_000)},"finish_reason":"stop"}]});
        let payload = format!("data: {first}\n\ndata: {oversized}\n\ndata: [DONE]\n\n");
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).unwrap();
    });
    let mut text = String::new();
    let error = network::chat(&ep,"test",&messages(),network::ChatOptions::default(),&CancellationToken::new(),|part,_|text.push_str(&part)).await.unwrap_err();
    server.join().unwrap(); assert!(error.contains("1 MB")); assert_eq!(text,"kept prefix");
}
