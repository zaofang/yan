//! `yan lsp` (18.6): the language server over stdio. This crate is only the protocol shell:
//! it maps LSP messages to `yan-ide` calls and back. No analysis lives here, and no LSP type
//! reaches `yan-ide`.

mod pos;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use pos::Encoding;
use serde_json::{Value, json};
use std::collections::HashMap;
use yan_diag::{Diagnostic, Severity, Span};
use yan_ide::{Database, SourceFile, SymbolKind};

/// Serve one client on stdio until it asks to exit.
pub fn run() -> Result<(), String> {
    let (conn, io) = Connection::stdio();
    let mut server = Server::new();
    let (id, params) = conn.initialize_start().map_err(|e| e.to_string())?;
    let result = server.initialize(&params);
    conn.initialize_finish(id, result)
        .map_err(|e| e.to_string())?;
    while let Ok(msg) = conn.receiver.recv() {
        if let Message::Request(r) = &msg
            && conn.handle_shutdown(r).map_err(|e| e.to_string())?
        {
            break;
        }
        for out in server.on_message(msg) {
            conn.sender.send(out).map_err(|e| e.to_string())?;
        }
    }
    drop(conn);
    io.join().map_err(|e| e.to_string())
}

struct Doc {
    file: SourceFile,
}

/// Session state: the query database and the open documents, keyed by URI.
pub struct Server {
    db: Database,
    docs: HashMap<String, Doc>,
    encoding: Encoding,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self {
            db: Database::new(false, false),
            docs: HashMap::new(),
            encoding: Encoding::Utf16,
        }
    }

    /// The `initialize` result. Negotiates `utf-8` positions when the client offers them (3.3).
    pub fn initialize(&mut self, params: &Value) -> Value {
        let offered = &params["capabilities"]["general"]["positionEncodings"];
        if offered
            .as_array()
            .is_some_and(|a| a.iter().any(|e| e == "utf-8"))
        {
            self.encoding = Encoding::Utf8;
        }
        json!({
            "capabilities": {
                "positionEncoding": self.encoding.name(),
                "textDocumentSync": { "openClose": true, "change": 1 },
                "hoverProvider": true,
                "documentFormattingProvider": true,
                "documentSymbolProvider": true,
            },
            "serverInfo": { "name": "yan", "version": env!("CARGO_PKG_VERSION") },
        })
    }

    /// The messages the server sends in answer to one from the client.
    pub fn on_message(&mut self, msg: Message) -> Vec<Message> {
        match msg {
            Message::Request(r) => vec![Message::Response(self.request(r))],
            Message::Notification(n) => self.notification(n),
            Message::Response(_) => Vec::new(),
        }
    }

    fn notification(&mut self, n: Notification) -> Vec<Message> {
        let p = &n.params;
        match n.method.as_str() {
            "textDocument/didOpen" => {
                let d = &p["textDocument"];
                match (d["uri"].as_str(), d["text"].as_str()) {
                    (Some(uri), Some(text)) => self.update(uri, text),
                    _ => Vec::new(),
                }
            }
            "textDocument/didChange" => {
                let uri = p["textDocument"]["uri"].as_str();
                // Full sync: the last change carries the whole text.
                let text = p["contentChanges"]
                    .as_array()
                    .and_then(|c| c.last())
                    .and_then(|c| c["text"].as_str());
                match (uri, text) {
                    (Some(uri), Some(text)) => self.update(uri, text),
                    _ => Vec::new(),
                }
            }
            "textDocument/didClose" => match p["textDocument"]["uri"].as_str() {
                Some(uri) if self.docs.remove(uri).is_some() => {
                    vec![publish(uri, Vec::new())]
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn update(&mut self, uri: &str, text: &str) -> Vec<Message> {
        let path = pos::uri_to_path(uri);
        let file = self.db.file(&path, text);
        self.docs.insert(uri.to_owned(), Doc { file });
        let diags = match yan_ide::diagnostics(&self.db, file) {
            Ok(d) => d
                .iter()
                .map(|d| diagnostic(d, uri, text, self.encoding))
                .collect(),
            Err(msg) => vec![tool_failure(&msg)],
        };
        vec![publish(uri, diags)]
    }

    fn request(&mut self, r: Request) -> Response {
        let Request { id, method, params } = r;
        let result = match method.as_str() {
            "textDocument/hover" => self.hover(&params),
            "textDocument/formatting" => self.formatting(&params),
            "textDocument/documentSymbol" => self.symbols(&params),
            _ => {
                return Response::new_err(
                    id,
                    ErrorCode::MethodNotFound as i32,
                    format!("unsupported: {method}"),
                );
            }
        };
        match result {
            Ok(v) => Response::new_ok(id, v),
            Err(msg) => Response::new_err(id, ErrorCode::RequestFailed as i32, msg),
        }
    }

    /// The open document a request names: its text and database input.
    fn doc(&self, params: &Value) -> Result<(String, SourceFile), String> {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .ok_or("no textDocument.uri")?;
        let doc = self
            .docs
            .get(uri)
            .ok_or_else(|| format!("not open: {uri}"))?;
        Ok((uri.to_owned(), doc.file))
    }

    fn hover(&self, params: &Value) -> Result<Value, String> {
        let (_, file) = self.doc(params)?;
        let text = file.text(&self.db);
        let (line, col) = pos::from_lsp(text, &params["position"], self.encoding);
        Ok(match yan_ide::hover(&self.db, file, line, col) {
            None => Value::Null,
            Some(h) => json!({
                "contents": { "kind": "plaintext", "value": h.text },
                "range": range(h.span, text, self.encoding),
            }),
        })
    }

    fn formatting(&self, params: &Value) -> Result<Value, String> {
        let (_, file) = self.doc(params)?;
        let text = file.text(&self.db);
        match yan_ide::canonical(file.path(&self.db), text) {
            Ok(new) if new == *text => Ok(json!([])),
            Ok(new) => {
                let end = pos::end_of(text, self.encoding);
                Ok(json!([{
                    "range": { "start": { "line": 0, "character": 0 }, "end": end },
                    "newText": new,
                }]))
            }
            // The module has errors: diagnostics already say so, and there is nothing to format.
            Err(yan_ide::ViewError::Diagnostics(_)) => Ok(Value::Null),
            Err(yan_ide::ViewError::Outside(m) | yan_ide::ViewError::Tool(m)) => Err(m),
        }
    }

    fn symbols(&self, params: &Value) -> Result<Value, String> {
        let (_, file) = self.doc(params)?;
        let text = file.text(&self.db);
        let out: Vec<Value> = yan_ide::symbols(&self.db, file)
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "kind": symbol_kind(s.kind),
                    "range": range(s.span, text, self.encoding),
                    "selectionRange": range(s.name_span, text, self.encoding),
                })
            })
            .collect();
        Ok(Value::Array(out))
    }
}

fn symbol_kind(k: SymbolKind) -> u32 {
    match k {
        SymbolKind::Function => 12,
        SymbolKind::Record => 23,
        SymbolKind::Type => 5,
        SymbolKind::Trait => 11,
        SymbolKind::Impl => 3,
        SymbolKind::Const => 14,
        SymbolKind::Extern => 2,
    }
}

fn range(s: Span, text: &str, enc: Encoding) -> Value {
    json!({
        "start": pos::to_lsp(text, s.line, s.col, enc),
        "end": pos::to_lsp(text, s.end_line, s.end_col, enc),
    })
}

fn diagnostic(d: &Diagnostic, uri: &str, text: &str, enc: Encoding) -> Value {
    let mut message = d.message.clone();
    for n in &d.notes {
        message.push_str("\nnote: ");
        message.push_str(n);
    }
    let related: Vec<Value> = d
        .labels
        .iter()
        .map(|l| {
            json!({
                "location": { "uri": uri, "range": range(l.span, text, enc) },
                "message": l.text,
            })
        })
        .collect();
    json!({
        "range": range(d.span, text, enc),
        "severity": match d.severity {
            Severity::Error => 1,
            Severity::Warning => 2,
            Severity::Note => 3,
        },
        "code": d.code,
        "source": "yan",
        "message": message,
        "relatedInformation": related,
    })
}

/// A failure of the tool itself (no front end, an unsupported construct), shown at the top.
fn tool_failure(msg: &str) -> Value {
    json!({
        "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
        "severity": 1,
        "source": "yan",
        "message": msg,
    })
}

fn publish(uri: &str, diagnostics: Vec<Value>) -> Message {
    Message::Notification(Notification::new(
        "textDocument/publishDiagnostics".to_owned(),
        json!({ "uri": uri, "diagnostics": diagnostics }),
    ))
}
