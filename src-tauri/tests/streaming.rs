use hearth_lib::{models::{EndpointConfig,PromptMessage},network};
use std::{io::{Read,Write},net::{TcpListener,TcpStream},sync::{Arc,Mutex},thread,time::Duration};
use tokio_util::sync::CancellationToken;

fn request(stream:&mut TcpStream)->serde_json::Value{
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();let mut data=vec![];let mut part=[0;1024];
    loop{let count=stream.read(&mut part).unwrap();if count==0{break;}data.extend_from_slice(&part[..count]);
        if let Some(end)=data.windows(4).position(|w|w==b"\r\n\r\n"){
            let headers=String::from_utf8_lossy(&data[..end]);let len=headers.lines().find_map(|line|line.to_lowercase().strip_prefix("content-length:").and_then(|s|s.trim().parse::<usize>().ok())).unwrap_or(0);
            if data.len()>=end+4+len{return serde_json::from_slice(&data[end+4..end+4+len]).unwrap();}
        }
    }panic!("No request body")
}
fn endpoint(port:u16)->EndpointConfig{EndpointConfig{id:"mock".into(),name:"Scripted test server".into(),kind:"custom".into(),url:format!("http://127.0.0.1:{port}/v1"),enabled:true}}
fn messages()->Vec<PromptMessage>{vec![PromptMessage{role:"user".into(),content:"hello".into()}]}

#[tokio::test]
async fn real_http_fragmented_sse_thinking_and_usage(){
    let listener=TcpListener::bind("127.0.0.1:0").unwrap();let ep=endpoint(listener.local_addr().unwrap().port());let captured=Arc::new(Mutex::new(serde_json::Value::Null));let capture=captured.clone();
    let server=thread::spawn(move||{let(mut stream,_)=listener.accept().unwrap();*capture.lock().unwrap()=request(&mut stream);
        let payload=concat!("data: {\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\r\n\r\n","data: {\"choices\":[{\"delta\":{\"content\":\"nk>private</think>Hello 🔥\"}}]}\r\n\r\n","data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":21,\"completion_tokens\":9}}\n\n","data: [DONE]\n\n");
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",payload.len()).unwrap();for chunk in payload.as_bytes().chunks(3){stream.write_all(chunk).unwrap();}
    });
    let mut text=String::new();let mut thought=String::new();let result=network::chat(&ep,"test-model",&messages(),0.6,128,None,&CancellationToken::new(),|a,b|{text.push_str(&a);thought.push_str(&b)}).await.unwrap();server.join().unwrap();
    assert_eq!(text,"Hello 🔥");assert_eq!(thought,"private");assert_eq!(result.content,text);assert_eq!(result.prompt_tokens,Some(21));assert_eq!(result.completion_tokens,Some(9));let captured=captured.lock().unwrap();assert_eq!(captured["model"],"test-model");assert_eq!(captured["max_tokens"],128);assert_eq!(captured["temperature"],0.6);
}
#[tokio::test]
async fn a_disconnect_keeps_partial_text_but_is_an_error(){
    let listener=TcpListener::bind("127.0.0.1:0").unwrap();let ep=endpoint(listener.local_addr().unwrap().port());let server=thread::spawn(move||{let(mut s,_)=listener.accept().unwrap();request(&mut s);let body="data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n";write!(s,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}").unwrap();});let mut partial=String::new();let result=network::chat(&ep,"test",&messages(),0.7,128,None,&CancellationToken::new(),|a,_|partial.push_str(&a)).await;server.join().unwrap();assert!(result.unwrap_err().contains("before finishing"));assert_eq!(partial,"partial");
}
#[tokio::test]
async fn cancellation_interrupts_prefill_before_headers(){
    let listener=TcpListener::bind("127.0.0.1:0").unwrap();let ep=endpoint(listener.local_addr().unwrap().port());let server=thread::spawn(move||{let(mut s,_)=listener.accept().unwrap();request(&mut s);thread::sleep(Duration::from_millis(250));});let cancel=CancellationToken::new();let token=cancel.clone();tokio::spawn(async move{tokio::time::sleep(Duration::from_millis(30)).await;token.cancel();});let start=std::time::Instant::now();let result=network::chat(&ep,"test",&messages(),0.7,128,None,&cancel,|_,_|{}).await;assert_eq!(result.unwrap_err(),"cancelled");assert!(start.elapsed()<Duration::from_millis(200));server.join().unwrap();
}
#[tokio::test]
async fn malformed_packets_and_native_tool_calls_fail_closed(){
    for payload in ["data: {bad json}\n\n", "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"{}\"}}]}}]}\n\n"] {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let ep=endpoint(listener.local_addr().unwrap().port());let payload=payload.to_string();let server=thread::spawn(move||{let(mut s,_)=listener.accept().unwrap();request(&mut s);write!(s,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{payload}",payload.len()).unwrap();});assert!(network::chat(&ep,"test",&messages(),0.7,128,None,&CancellationToken::new(),|_,_|{}).await.is_err());server.join().unwrap();
    }
}
