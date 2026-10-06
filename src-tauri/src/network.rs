use crate::models::{BackendStatus, EndpointConfig, ModelInfo, PromptMessage};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use url::Url;

pub fn endpoint_url(endpoint: &EndpointConfig) -> Result<Url, String> {
    let mut url = Url::parse(&endpoint.url).map_err(|_| "Enter a valid local backend URL.".to_string())?;
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some()
        || url.query().is_some() || url.fragment().is_some() {
        return Err("This release supports HTTP loopback endpoints without credentials, queries or fragments.".into());
    }
    match url.host_str() {
        Some("localhost") => { url.set_host(Some("127.0.0.1")).map_err(|e|e.to_string())?; }
        Some("127.0.0.1") | Some("[::1]") | Some("::1") => {}
        _ => return Err("Use a loopback backend (127.0.0.1, localhost or ::1). LAN support is deferred.".into()),
    }
    if url.port_or_known_default() == Some(0) { return Err("Backend port cannot be zero.".into()); }
    let path = url.path().trim_end_matches('/');
    if path != "/v1" && !path.is_empty() { return Err("Use the backend's /v1 OpenAI-compatible URL.".into()); }
    url.set_path("/v1/");
    Ok(url)
}

pub fn client() -> Result<Client, String> {
    Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2)).build().map_err(|e| e.to_string())
}

pub async fn discover(endpoint: EndpointConfig) -> BackendStatus {
    let mut status = BackendStatus { id: endpoint.id.clone(), name:endpoint.name.clone(),url:endpoint.url.clone(),online:false,error:None,models:vec![] };
    let result = async {
        let base = endpoint_url(&endpoint)?;
        let client = client()?;
        let response = client.get(base.join("models").map_err(|e|e.to_string())?).timeout(Duration::from_secs(3)).send().await.map_err(|_|"Server isn't responding. Start it in your backend app.".to_string())?;
        if !response.status().is_success() { return Err(format!("Model discovery returned HTTP {}.",response.status().as_u16())); }
        let body: Value = response.json().await.map_err(|_|"Backend returned an invalid model list.".to_string())?;
        let entries = body.get("data").and_then(Value::as_array).ok_or_else(||"Backend model list is missing data.".to_string())?;
        let mut models:Vec<ModelInfo> = entries.iter().filter_map(|m| {
            let id=m.get("id")?.as_str()?.to_string();
            Some(ModelInfo { name:id.split('/').next_back().unwrap_or(&id).into(),canonical_key:canonical_key(&id),id,backend_id:endpoint.id.clone(),loaded:None,size_bytes:None,context_window:None,format:None })
        }).collect();
        if endpoint.kind == "lmstudio" {
            for path in ["/api/v1/models", "/api/v0/models"] {
                let mut url=base.clone();url.set_path(path);
                if let Ok(res)=client.get(url).timeout(Duration::from_secs(2)).send().await {
                    if !res.status().is_success(){continue;}
                    if let Ok(extra)=res.json::<Value>().await {
                        if let Some(items)=extra.get("models").or_else(||extra.get("data")).and_then(Value::as_array) {
                            for model in &mut models {
                                if let Some(info)=items.iter().find(|info|info.get("id").or_else(||info.get("key")).and_then(Value::as_str)==Some(&model.id)) {
                                    model.loaded = info.get("state").and_then(Value::as_str).map(|s|s=="loaded").or_else(||info.get("loaded_instances").and_then(Value::as_array).map(|a|!a.is_empty()));
                                    model.size_bytes=info.get("size_bytes").and_then(Value::as_u64);
                                    model.context_window=info.get("max_context_length").and_then(Value::as_u64).map(|n|n as usize);
                                    model.format=info.get("compatibility_type").or_else(||info.get("format")).and_then(Value::as_str).map(str::to_string);
                                }
                            }
                            break;
                        }
                    }
                }
            }
        } else if endpoint.kind=="ollama" {
            let mut url=base.clone();url.set_path("/api/ps");
            if let Ok(res)=client.get(url).timeout(Duration::from_secs(2)).send().await {
                if res.status().is_success(){if let Ok(extra)=res.json::<Value>().await {
                    if let Some(items)=extra.get("models").and_then(Value::as_array){for model in &mut models{
                        let loaded=items.iter().find(|m|m.get("name").or_else(||m.get("model")).and_then(Value::as_str)==Some(&model.id));
                        model.loaded=Some(loaded.is_some());model.size_bytes=loaded.and_then(|m|m.get("size")).and_then(Value::as_u64);
                        model.context_window=loaded.and_then(|m|m.get("context_length")).and_then(Value::as_u64).map(|n|n as usize);
                    }}
                }}
            }
        }
        Ok::<_,String>(models)
    }.await;
    match result { Ok(models)=>{status.online=true;status.models=models;},Err(error)=>status.error=Some(error) };
    status
}

pub fn canonical_key(id:&str)->String {
    id.rsplit('/').next().unwrap_or(id).to_lowercase().replace('_',"-")
        .replace("-4bit","-q4").replace("-8bit","-q8").replace(":","-")
}

#[derive(Default, Debug)]
pub struct SseDecoder { pending:Vec<u8>, data:Vec<String> }
impl SseDecoder {
    pub fn feed(&mut self, bytes:&[u8])->Result<Vec<String>,String>{
        self.pending.extend_from_slice(bytes);
        if self.pending.len()>2_000_000{return Err("Backend event exceeds the 2 MB limit.".into());}
        let mut events=vec![];
        while let Some(end)=self.pending.iter().position(|b|*b==b'\n'){
            let raw:Vec<u8>=self.pending.drain(..=end).collect();
            let line=std::str::from_utf8(&raw).map_err(|_|"Backend sent invalid UTF-8.".to_string())?.trim_end_matches(['\r','\n']);
            if line.is_empty(){if !self.data.is_empty(){events.push(self.data.join("\n"));self.data.clear();}}
            else if let Some(value)=line.strip_prefix("data:"){self.data.push(value.strip_prefix(' ').unwrap_or(value).into());}
        }
        Ok(events)
    }
}

#[derive(Default)]
struct ThinkingFilter { pending:String, thinking:bool }
impl ThinkingFilter {
    fn push(&mut self,text:&str,flush:bool)->(String,String){
        self.pending.push_str(text);let mut visible=String::new();let mut thought=String::new();
        loop{
            let tag=if self.thinking{"</think>"}else{"<think>"};
            if let Some(pos)=self.pending.find(tag){
                let before=self.pending[..pos].to_string();if self.thinking{thought.push_str(&before)}else{visible.push_str(&before)};
                self.pending.drain(..pos+tag.len());self.thinking=!self.thinking;continue;
            }
            let retain=if flush{0}else{(1..tag.len()).rev().find(|n|self.pending.ends_with(&tag[..*n])).unwrap_or(0)};
            let ready=self.pending.len()-retain;
            let chunk=self.pending[..ready].to_string();self.pending.drain(..ready);
            if self.thinking{thought.push_str(&chunk)}else{visible.push_str(&chunk)};break;
        }
        (visible,thought)
    }
}

#[derive(Default,Debug)]
pub struct StreamResult { pub content:String,pub thinking:String,pub prompt_tokens:Option<usize>,pub completion_tokens:Option<usize>,pub ttft_ms:u64,pub duration_ms:u64 }

pub async fn chat<F>(endpoint:&EndpointConfig,model:&str,messages:&[PromptMessage],temperature:f64,max_tokens:usize,schema:Option<Value>,cancel:&CancellationToken,mut delta:F)->Result<StreamResult,String>
where F:FnMut(String,String) {
    let base=endpoint_url(endpoint)?;let client=client()?;
    let mut body=json!({"model":model,"messages":messages,"temperature":temperature,"top_p":0.95,"max_tokens":max_tokens,"stream":true,"stream_options":{"include_usage":true}});
    if let Some(schema)=schema {body["response_format"]=json!({"type":"json_schema","json_schema":{"name":"hearth_action","strict":true,"schema":schema}});}
    let start=Instant::now();
    let response=tokio::select!{
        _=cancel.cancelled()=>return Err("cancelled".into()),
        response=client.post(base.join("chat/completions").map_err(|e|e.to_string())?).json(&body).timeout(Duration::from_secs(180)).send()=>response.map_err(|e|format!("Couldn't connect to {}: {}",endpoint.name,e))?,
    };
    if !response.status().is_success(){let status=response.status().as_u16();return Err(format!("{} returned HTTP {status}. Check the model and server settings.",endpoint.name));}
    let mut stream=response.bytes_stream();let mut parser=SseDecoder::default();let mut filter=ThinkingFilter::default();let mut result=StreamResult::default();let mut ended=false;let mut finish=false;let mut saw_token=false;
    loop{
        let next=tokio::select!{
            _=cancel.cancelled()=>return Err("cancelled".into()),
            next=tokio::time::timeout(Duration::from_secs(90),stream.next())=>next.map_err(|_|"Backend made no progress for 90 seconds.".to_string())?,
        };
        let Some(next)=next else{break;};
        let bytes=next.map_err(|_|"Backend disconnected during the response. Partial text has been kept.".to_string())?;
        for event in parser.feed(&bytes)?{
            if event.trim()=="[DONE]"{ended=true;break;}
            let frame:Value=serde_json::from_str(&event).map_err(|_|"Backend sent a malformed streaming event.".to_string())?;
            if frame.get("error").is_some(){return Err("Backend reported an inference error. Check its server logs.".into());}
            if let Some(usage)=frame.get("usage"){
                result.prompt_tokens=usage.get("prompt_tokens").and_then(Value::as_u64).map(|n|n as usize);
                result.completion_tokens=usage.get("completion_tokens").and_then(Value::as_u64).map(|n|n as usize);
            }
            if let Some(choice)=frame.get("choices").and_then(Value::as_array).and_then(|a|a.first()){
                if choice.get("finish_reason").and_then(Value::as_str).is_some(){finish=true;}
                let d=&choice["delta"];
                if d.get("tool_calls").is_some(){return Err("This run uses JSON actions; unexpected native tool calls were rejected.".into());}
                let text=d.get("content").and_then(Value::as_str).unwrap_or("");
                let reasoning=d.get("reasoning_content").or_else(||d.get("reasoning")).and_then(Value::as_str).unwrap_or("");
                let (visible,mut thought)=filter.push(text,false);thought.push_str(reasoning);
                if !visible.is_empty()||!thought.is_empty(){
                    if !saw_token{result.ttft_ms=start.elapsed().as_millis() as u64;saw_token=true;}
                    result.content.push_str(&visible);result.thinking.push_str(&thought);delta(visible,thought);
                    if result.content.len()+result.thinking.len()>4_000_000{return Err("Response exceeded the 4 MB limit.".into());}
                }
            }
        }
        if ended{break;}
    }
    if !ended&&!finish{return Err("Backend disconnected before finishing. Partial text has been kept.".into());}
    let (visible,thought)=filter.push("",true);result.content.push_str(&visible);result.thinking.push_str(&thought);
    if !visible.is_empty()||!thought.is_empty(){delta(visible,thought);}
    result.duration_ms=start.elapsed().as_millis() as u64;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn endpoint(url:&str)->EndpointConfig{EndpointConfig{id:"test".into(),name:"Test".into(),kind:"custom".into(),url:url.into(),enabled:true}}
    #[test] fn endpoint_is_loopback_and_unambiguous(){
        for bad in ["https://example.com/v1","http://192.168.1.2:1234/v1","http://user:secret@localhost/v1","http://127.0.0.1/v1?x=y","file:///tmp/x","http://localhost:1234/control"]{assert!(endpoint_url(&endpoint(bad)).is_err(),"{bad}");}
        assert_eq!(endpoint_url(&endpoint("http://localhost:1234/v1")).unwrap().host_str(),Some("127.0.0.1"));
        assert!(endpoint_url(&endpoint("http://[::1]:1234/v1")).is_ok());
    }
    #[test] fn fragmented_unicode_and_crlf_sse(){
        let bytes="data: {\"choices\":[{\"delta\":{\"content\":\"🔥\"}}]}\r\n\r\ndata: [DONE]\n\n".as_bytes();
        let mut p=SseDecoder::default();let mut out=vec![];for b in bytes{out.extend(p.feed(&[*b]).unwrap());}assert_eq!(out.len(),2);assert!(out[0].contains('🔥'));assert_eq!(out[1],"[DONE]");
    }
    #[test] fn multiline_comments_and_invalid_utf8(){let mut p=SseDecoder::default();assert_eq!(p.feed(b": ping\ndata: first\ndata: second\n\n").unwrap(),vec!["first\nsecond"]);assert!(p.feed(&[0xff,b'\n']).is_err());}
    #[test] fn thinking_tags_can_cross_chunks(){
        let mut p=ThinkingFilter::default();let mut visible=String::new();let mut thought=String::new();for s in ["Hi <thi","nk>plan 🔥</th","ink>there"]{let(a,b)=p.push(s,false);visible.push_str(&a);thought.push_str(&b);}let(a,b)=p.push("",true);visible.push_str(&a);thought.push_str(&b);assert_eq!(visible,"Hi there");assert_eq!(thought,"plan 🔥");
    }
}
