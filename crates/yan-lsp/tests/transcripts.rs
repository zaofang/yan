//! Golden transcripts: each `tests/golden/*.json` lists client messages and the server messages
//! they must produce. `YAN_BLESS=1` rewrites the expected output from the server's answers.

use lsp_server::Message;
use serde_json::{Value, json};
use yan_lsp::Server;

fn replay(name: &str) {
    let path = format!("{}/tests/golden/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let bless = std::env::var_os("YAN_BLESS").is_some();
    let mut server = Server::new();
    for step in doc["steps"].as_array_mut().unwrap() {
        let input = step["in"].clone();
        let msg: Message = serde_json::from_value(input.clone()).unwrap();
        let out: Vec<Value> = match msg {
            Message::Request(r) if r.method == "initialize" => {
                let result = server.initialize(&r.params);
                vec![json!({ "jsonrpc": "2.0", "id": r.id, "result": result })]
            }
            msg => server
                .on_message(msg)
                .into_iter()
                .map(|m| serde_json::to_value(m).unwrap())
                .collect(),
        };
        if bless {
            step["out"] = Value::Array(out);
        } else {
            assert_eq!(step["out"], Value::Array(out), "{name}: {input}");
        }
    }
    if bless {
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    }
}

#[test]
fn yag_session() {
    replay("yag_session");
}

#[test]
fn yag_utf16() {
    replay("yag_utf16");
}

#[test]
fn yn_session() {
    replay("yn_session");
}
