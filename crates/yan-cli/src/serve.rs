//! `yan mcp package [DIR]` (RFC 0062): serve a package's public functions as MCP tools over
//! stdio. A tool's arguments and result are its signature's json forms (R875); a call binds only
//! the capabilities the operator granted with `--allow`, and `fs` resolves beneath the package.

use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Map, Value, json};
use yan_core::ast as y;
use yan_db::{Database, Loaded};
use yan_diag::{DEFAULT_MAX_ERRORS, Severity, finish};
use yan_interp::{Limits, Stop};

use crate::mcp::{Budget, PER_CALL, PER_SESSION, PROTOCOL, error};

/// One served function: its tool entry and what it uses.
struct Tool {
    entry: Value,
    uses: Vec<String>,
}

pub struct Package {
    root: PathBuf,
    module: y::Module,
    tools: Vec<(String, Tool)>,
    allow: Vec<String>,
    left: RefCell<Budget>,
}

/// The module a package serves: `src/main.yag`, else `src/lib.yag`.
fn entry_module(root: &Path) -> Result<PathBuf, String> {
    ["src/main.yag", "src/lib.yag"]
        .iter()
        .map(|p| root.join(p))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            format!(
                "{}: no src/main.yag or src/lib.yag to serve",
                root.display()
            )
        })
}

impl Package {
    /// Check the package's module and collect its tools; any error stops the server (law 13).
    pub fn open(root: &Path, allow: Vec<String>, trusted: bool) -> Result<Package, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", root.display()))?;
        if !trusted {
            return Err(format!(
                "{} is not trusted, and serving it runs its code; run `yan trust` there first",
                root.display()
            ));
        }
        let path = entry_module(&root)?;
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut db = Database::new(false, false).tools();
        let f = db.file(&path, &src);
        let diags = yan_ide::diagnostics(&db, f)?;
        let errors: Vec<String> = finish(diags, DEFAULT_MAX_ERRORS)
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.render_line(None))
            .collect();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        let module = match yan_db::load(&db, f).clone()? {
            Loaded::Ready(m, _, _) => m,
            Loaded::Stopped(d) => {
                return Err(d
                    .iter()
                    .map(|d| d.render_line(None))
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
        };
        let tools = module.items.iter().filter_map(tool_of).collect();
        Ok(Package {
            root,
            module,
            tools,
            allow,
            left: RefCell::new(PER_SESSION),
        })
    }

    pub fn list(&self) -> Value {
        json!({"tools": self.tools.iter().map(|(_, t)| &t.entry).collect::<Vec<_>>()})
    }

    fn call(&self, name: &str, args: &Value) -> Result<Value, String> {
        let (_, tool) = self
            .tools
            .iter()
            .find(|(n, _)| n == name)
            .ok_or_else(|| format!("no tool `{name}`"))?;
        let missing: Vec<&str> = tool
            .uses
            .iter()
            .filter(|c| !self.allow.contains(c))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "`{name}` uses {}, which the server was not granted; restart it with `--allow {}`",
                missing
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                tool.uses.join(",")
            ));
        }
        let args = if args.is_null() {
            json!({})
        } else {
            args.clone()
        };
        let given = PER_CALL.min(*self.left.borrow());
        let (start, mut steps) = (Instant::now(), 0);
        let r = yan_interp::tool(
            &self.module,
            name,
            &args.to_string(),
            &self.root.display().to_string(),
            given.limits(Limits::default()),
            &mut std::io::sink(),
            &mut steps,
        );
        self.left.borrow_mut().spend(steps, start.elapsed());
        match r {
            Ok(Ok(text)) => {
                let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                Ok(json!({
                    "content": [{"type": "text", "text": text}],
                    "structuredContent": {"result": value},
                }))
            }
            Ok(Err(failure)) => Err(failure),
            Err(Stop::Limit(what)) => Err(format!("`{name}` ran out of its {what} budget")),
            Err(Stop::Panic(p)) => Err(format!("`{name}` panicked: {}", p.message)),
            Err(Stop::Unsupported(what)) => Err(what),
        }
    }

    /// The reply to one message; notifications get none.
    pub fn respond(&self, req: &Value) -> Option<Value> {
        let id = req.get("id")?.clone();
        let params = &req["params"];
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "yan-package", "version": env!("CARGO_PKG_VERSION")},
            }),
            "ping" => json!({}),
            "tools/list" => self.list(),
            "tools/call" => match self
                .call(params["name"].as_str().unwrap_or(""), &params["arguments"])
            {
                Ok(r) => r,
                Err(msg) => json!({"content": [{"type": "text", "text": msg}], "isError": true}),
            },
            m => return Some(error(id, -32601, &format!("no method `{m}`"))),
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    pub fn serve(&self) -> Result<(), String> {
        let mut out = std::io::stdout().lock();
        for line in std::io::stdin().lock().lines() {
            let line = line.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(req) => self.respond(&req),
                Err(e) => Some(error(Value::Null, -32700, &e.to_string())),
            };
            if let Some(r) = reply {
                writeln!(out, "{r}")
                    .and_then(|()| out.flush())
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

/// The tool a Core item carries, from the `tool` attribute elaboration attached.
fn tool_of(it: &y::Item) -> Option<(String, Tool)> {
    let y::ItemKind::Fn(d) = &it.kind else {
        return None;
    };
    let a = it.attrs.iter().find(|a| a.name == "tool")?;
    let y::ConstVal::Lit(y::Literal::Str(text)) = &a.args.as_ref()?.first()?.value else {
        return None;
    };
    let t: Value = serde_json::from_str(text).ok()?;
    let name = d.path.join(".");
    let uses: Vec<String> = serde_json::from_value(t["uses"].clone()).ok()?;
    let doc = t["doc"].as_str().unwrap_or("").trim();
    let authority = if uses.is_empty() {
        "uses: none".to_owned()
    } else {
        format!("uses: {}", uses.join(", "))
    };
    let description = if doc.is_empty() {
        authority
    } else {
        format!("{doc}\n\n{authority}")
    };
    let sc = &t["schema"];
    let defs = sc["defs"].as_object()?;
    let mut input = json_schema(&json!({"named": "$args"}), defs, true);
    let reached = reached_defs(defs);
    let mut output = json!({
        "type": "object",
        "properties": {"result": json_schema(&sc["ret"], defs, false)},
        "required": ["result"],
        "additionalProperties": false,
    });
    if !reached.is_empty() {
        input["$defs"] = Value::Object(reached.clone());
        output["$defs"] = Value::Object(reached);
    }
    let entry = json!({
        "name": name,
        "description": description,
        "inputSchema": input,
        "outputSchema": output,
    });
    Some((name, Tool { entry, uses }))
}

/// Every named def but `$args`, as JSON Schema.
fn reached_defs(defs: &Map<String, Value>) -> Map<String, Value> {
    defs.iter()
        .filter(|(n, _)| *n != "$args")
        .map(|(n, d)| (n.clone(), def_schema(d, defs)))
        .collect()
}

/// The JSON Schema of a wire type (the shapes of R875). `inline` expands `named` in place.
fn json_schema(t: &Value, defs: &Map<String, Value>, inline: bool) -> Value {
    match t {
        Value::String(s) => match s.as_str() {
            "bool" => json!({"type": "boolean"}),
            "int" => json!({"type": "integer"}),
            "float" => json!({"type": "number"}),
            "str" => json!({"type": "string"}),
            "char" => json!({"type": "string", "minLength": 1, "maxLength": 1}),
            _ => json!({"type": "null"}),
        },
        Value::Object(o) => {
            let Some((k, v)) = o.iter().next() else {
                return json!({});
            };
            match k.as_str() {
                "named" => {
                    let n = v.as_str().unwrap_or("");
                    if inline {
                        def_schema(&defs[n], defs)
                    } else {
                        json!({"$ref": format!("#/$defs/{n}")})
                    }
                }
                "opt" => json!({"anyOf": [json_schema(v, defs, false), {"type": "null"}]}),
                "list" => json!({"type": "array", "items": json_schema(v, defs, false)}),
                "set" => {
                    json!({"type": "array", "items": json_schema(v, defs, false), "uniqueItems": true})
                }
                "dict" if v[0] == "str" => json!({
                    "type": "object",
                    "additionalProperties": json_schema(&v[1], defs, false),
                }),
                "dict" => json!({
                    "type": "array",
                    "items": {
                        "type": "array",
                        "prefixItems": [json_schema(&v[0], defs, false), json_schema(&v[1], defs, false)],
                        "minItems": 2,
                        "maxItems": 2,
                    },
                }),
                "tuple" => {
                    let ts: Vec<Value> = v
                        .as_array()
                        .map(|ts| ts.iter().map(|t| json_schema(t, defs, false)).collect())
                        .unwrap_or_default();
                    let n = ts.len();
                    json!({"type": "array", "prefixItems": ts, "minItems": n, "maxItems": n})
                }
                _ => json!({}),
            }
        }
        _ => json!({}),
    }
}

/// A record is an object of its fields; a field with a default or an optional type may be absent
/// (R875). A sum is one object per variant, its tag a constant (R874).
fn def_schema(d: &Value, defs: &Map<String, Value>) -> Value {
    let closed = !d["unk"].as_bool().unwrap_or(false);
    let object = |fs: &Value, extra: Option<(&str, &str)>| {
        let mut props = Map::new();
        let mut required = Vec::new();
        if let Some((tag, v)) = extra {
            props.insert(tag.into(), json!({"const": v}));
            required.push(json!(tag));
        }
        for f in fs.as_array().into_iter().flatten() {
            let key = f["w"].as_str().or(f["n"].as_str()).unwrap_or("");
            let mut s = json_schema(&f["t"], defs, false);
            if let Some(dv) = f.get("d") {
                s["default"] = dv.clone();
            } else if f["t"].get("opt").is_none() {
                required.push(json!(key));
            }
            props.insert(key.into(), s);
        }
        json!({
            "type": "object",
            "properties": props,
            "required": required,
            "additionalProperties": !closed,
        })
    };
    if let Some(fs) = d.get("rec") {
        return object(fs, None);
    }
    let tag = d["tag"].as_str().unwrap_or("tag");
    let variants: Vec<Value> = d["sum"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            object(
                &v["fs"],
                Some((tag, v["w"].as_str().or(v["n"].as_str()).unwrap_or(""))),
            )
        })
        .collect();
    json!({"oneOf": variants})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(tag: &str, src: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yan-serve-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.yag"), src).unwrap();
        dir
    }

    fn call(p: &Package, name: &str, args: Value) -> Value {
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}});
        p.respond(&req).unwrap()["result"].clone()
    }

    fn text(r: &Value) -> &str {
        r["content"][0]["text"].as_str().unwrap()
    }

    const SRC: &str = r#"@derive(Wire)
class Point:
    x: int
    y: int

pub def add(a: int, b: int = 1) -> int:
    """Add two numbers."""
    return a + b

pub def mid(p: Point, q: Point) -> Point:
    """The midpoint."""
    return Point(x=(p.x + q.x) // 2, y=(p.y + q.y) // 2)

pub def size(path: str) -> int fail FsError uses fs:
    """The length of a file."""
    text = fs.read(path)?
    return text.char_count()

def hidden() -> int:
    return 1

def test_add():
    assert add(1) == 2
"#;

    #[test]
    fn public_functions_are_tools() {
        let dir = package("list", SRC);
        let p = Package::open(&dir, Vec::new(), true).unwrap();
        let list = p.list();
        let names: Vec<&str> = list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["add", "mid", "size"]);
        let add = &list["tools"][0];
        assert_eq!(add["description"], "Add two numbers.\n\nuses: none");
        assert_eq!(add["inputSchema"]["required"], json!(["a"]));
        assert_eq!(add["inputSchema"]["properties"]["b"]["default"], 1);
        assert_eq!(
            list["tools"][2]["description"],
            "The length of a file.\n\nuses: fs"
        );
        assert_eq!(
            list["tools"][1]["inputSchema"]["properties"]["p"]["$ref"],
            "#/$defs/Point"
        );
    }

    #[test]
    fn calls_decode_arguments_and_encode_results() {
        let dir = package("call", SRC);
        let p = Package::open(&dir, Vec::new(), true).unwrap();
        assert_eq!(
            call(&p, "add", json!({"a": 2}))["structuredContent"]["result"],
            3
        );
        let r = call(
            &p,
            "mid",
            json!({"p": {"x": 0, "y": 0}, "q": {"x": 4, "y": 6}}),
        );
        assert_eq!(r["structuredContent"]["result"], json!({"x": 2, "y": 3}));
        let r = call(&p, "add", json!({"a": 2.5}));
        assert_eq!(r["isError"], true);
        assert!(text(&r).contains("$.a: expected int"), "{r}");
        let r = call(&p, "add", json!({"a": 1, "z": 2}));
        assert!(text(&r).contains("unknown field"), "{r}");
        assert_eq!(call(&p, "hidden", json!({}))["isError"], true);
        assert_eq!(call(&p, "test_add", json!({}))["isError"], true);
    }

    #[test]
    fn authority_is_the_operators_grant() {
        let dir = package("grant", SRC);
        std::fs::write(dir.join("notes.txt"), "buy milk\n").unwrap();
        let p = Package::open(&dir, Vec::new(), true).unwrap();
        let r = call(&p, "size", json!({"path": "notes.txt"}));
        assert!(text(&r).contains("--allow fs"), "{r}");
        let p = Package::open(&dir, vec!["fs".into()], true).unwrap();
        let r = call(&p, "size", json!({"path": "notes.txt"}));
        assert_eq!(r["structuredContent"]["result"], 9, "{r}");
        for out in ["../notes.txt", "/etc/passwd"] {
            let r = call(&p, "size", json!({"path": out}));
            assert!(text(&r).contains("not granted"), "{out}: {r}");
        }
    }

    #[test]
    fn untrusted_packages_are_not_served() {
        let dir = package("untrusted", SRC);
        let e = Package::open(&dir, Vec::new(), false).err().unwrap();
        assert!(e.contains("yan trust"), "{e}");
    }

    #[test]
    fn unservable_public_functions_stop_the_server() {
        for (src, why) in [
            (
                "pub def f(xs: Callable[[int], int]) -> int:\n    \"\"\"F.\"\"\"\n    return xs(1)\n",
                "json form",
            ),
            (
                "pub def f() uses io:\n    \"\"\"F.\"\"\"\n    io.print(\"x\")\n",
                "`io`",
            ),
            (
                "pub def f[T](x: T) -> T:\n    \"\"\"F.\"\"\"\n    return x\n",
                "generic",
            ),
        ] {
            let dir = package("refuse", src);
            let e = Package::open(&dir, Vec::new(), true).err().unwrap();
            assert!(e.contains("E201") && e.contains(why), "{src}: {e}");
        }
    }
}
