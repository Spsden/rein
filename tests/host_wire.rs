use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Host {
    child: Child,
    input: ChildStdin,
    output: Receiver<Value>,
}

impl Host {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rein"))
            .arg("host-v1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx
                    .send(serde_json::from_str(&line.unwrap()).unwrap())
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut host = Self {
            child,
            input,
            output,
        };
        host.send(json!({
            "type":"start","run_id":"test-run","instructions":"System instruction",
            "history":[{"role":"user","text":"Earlier context"}],
            "prompt":"Inspect","max_tokens":64,
            "tools":[{"name":"read","description":"A host tool","parameters":{"type":"object"}}],
        }));
        assert_eq!(host.recv("ready")["version"], 1);
        host
    }

    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").unwrap();
        self.input.flush().unwrap();
    }

    fn recv(&self, kind: &str) -> Value {
        let value = self.output.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(value["type"], kind, "{value}");
        value
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn host_wire_runs_rein_loop_without_executing_unadvertised_tools() {
    let mut host = Host::start();
    let first = host.recv("model");
    let messages = first["payload"]["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[1]["text"], "Earlier context");
    assert_eq!(messages[2]["text"], "Inspect");
    host.send(json!({
        "type":"model_result","id":first["id"],"text":"","reasoning":"Think",
        "output_tokens":5,"finish":"tool_calls","calls":[
            {"id":"denied","name":"shell","arguments":{}},
            {"id":"approved","name":"read","arguments":{"path":"fixture"}},
        ]
    }));
    host.recv("accepted");
    let tool = host.recv("tool");
    assert_eq!(tool["payload"]["id"], "approved");
    assert_eq!(tool["payload"]["arguments"]["path"], "fixture");
    host.send(json!({"type":"steer","id":"steer-1","text":"Focus on the fixture"}));
    host.send(json!({"type":"tool_result","id":tool["id"],"content":"file content"}));
    assert_eq!(host.recv("steer_accepted")["id"], "steer-1");
    let second = host.recv("model");
    assert_eq!(second["payload"]["max_tokens"], 59);
    let messages = second["payload"]["messages"].as_array().unwrap();
    assert_eq!(messages[3]["response_id"], first["id"].to_string());
    assert_eq!(messages[3]["reasoning"], "Think");
    assert_eq!(messages[4]["text"], "Tool is not authorized by the host");
    assert_eq!(messages[5]["call_id"], "approved");
    assert_eq!(messages[5]["text"], "file content");
    assert_eq!(messages[6]["text"], "Focus on the fixture");
    host.send(json!({
        "type":"model_result","id":second["id"],"text":"Done","reasoning":"",
        "calls":[],"output_tokens":4,"finish":"stop"
    }));
    host.recv("accepted");
    assert_eq!(host.recv("finished")["output_tokens"], 9);
}

#[test]
fn cancel_interrupts_pending_model_response() {
    let mut host = Host::start();
    host.recv("model");
    host.send(json!({"type":"cancel"}));
    assert_eq!(host.recv("failed")["code"], "Cancelled");
}

#[test]
fn miscorrelated_tool_result_never_releases_a_later_effect() {
    let mut host = Host::start();
    let model = host.recv("model");
    host.send(json!({
        "type":"model_result","id":model["id"],"text":"","reasoning":"",
        "output_tokens":5,"finish":"tool_calls","calls":[
            {"id":"first","name":"read","arguments":{}},
            {"id":"later","name":"read","arguments":{}},
        ]
    }));
    host.recv("accepted");
    let tool = host.recv("tool");
    assert_eq!(tool["payload"]["id"], "first");
    host.send(json!({"type":"tool_result","id":999,"content":"not correlated"}));
    assert_eq!(host.recv("failed")["code"], "InvalidResponse");
}
