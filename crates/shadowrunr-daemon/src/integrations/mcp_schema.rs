//! MCP tool input schemas, cut from the generated wire contract
//! (`shadowrunr_types::schema::contract_schema`) with `$ref`s inlined and a few fields
//! relaxed so an agent may omit them.

use std::sync::OnceLock;

use serde_json::{json, Map, Value};

use super::mcp_tools::{ToolDef, TOOLS};

fn contract() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(shadowrunr_types::schema::contract_schema)
}

/// Replace every `#/$defs/X` reference with the def itself (siblings such as a
/// `description` are kept). The params types are not recursive; `depth` guards anyway.
fn inline(defs: &Value, v: &Value, depth: usize) -> Value {
    match v {
        Value::Object(obj) if depth < 32 => {
            let mut out = Map::new();
            if let Some(name) = obj
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| r.strip_prefix("#/$defs/"))
            {
                if let Some(Value::Object(def)) = defs.get(name).map(|d| inline(defs, d, depth + 1))
                {
                    out.extend(def);
                }
            }
            for (k, child) in obj {
                if k != "$ref" {
                    out.insert(k.clone(), inline(defs, child, depth + 1));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|i| inline(defs, i, depth + 1)).collect())
        }
        other => other.clone(),
    }
}

/// The params schema of a `/rpc` method, self-contained.
pub fn params_schema(method: &str) -> Value {
    let s = contract();
    let r = &s["properties"][method]["properties"]["params"];
    let mut v = inline(&s["$defs"], r, 0);
    if let Value::Object(o) = &mut v {
        o.entry("type").or_insert(json!("object"));
    }
    v
}

fn unrequire(schema: &mut Value, field: &str) {
    if let Some(Value::Array(req)) = schema.get_mut("required") {
        req.retain(|r| r != field);
    }
}

fn add_prop(schema: &mut Value, name: &str, prop: Value) {
    if let Value::Object(o) = schema {
        let props = o.entry("properties").or_insert_with(|| json!({}));
        if let Value::Object(p) = props {
            p.insert(name.into(), prop);
        }
    }
}

/// A tool's `inputSchema`.
pub fn input_schema(t: &ToolDef) -> Value {
    match t.name {
        // The character is `[moderator] character`: the agent names none.
        "moderator_character_get" => return json!({ "type": "object", "properties": {} }),
        "moderator_character_set" => {
            return json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Display name." },
                    "tagline": { "type": "string", "description": "One line under the name." },
                    "identity": { "type": "object", "description": "The [identity] table of character.toml." }
                }
            })
        }
        _ => {}
    }
    let mut s = params_schema(t.method);
    match t.name {
        "list_runs" => unrequire(&mut s, "limit"),
        "get_transcript" => {
            add_prop(
                &mut s,
                "cursor",
                json!({ "type": "string", "description": "next_cursor of the previous page." }),
            );
            add_prop(
                &mut s,
                "limit",
                json!({ "type": "integer", "minimum": 1, "maximum": 500, "description": "Events per page (default 200)." }),
            );
        }
        "start_sim" => {
            unrequire(&mut s, "spec");
            if let Some(Value::Object(o)) = s.pointer_mut("/properties/spec") {
                o.remove("required");
            }
            add_prop(
                &mut s,
                "live",
                json!({ "type": "boolean", "description": "Use the live Claude Code fleet (spends quota). Default false: scripted." }),
            );
        }
        _ => {}
    }
    s
}

/// `tools/list` entries. The schema is also mirrored as `input_schema`: the Personas MCP
/// client deserializes tool definitions by that snake_case name (`McpTool::input_schema`
/// in its `engine/mcp_tools.rs`), so without it Personas agents see no schema.
pub fn tool_list() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            let schema = input_schema(t);
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": schema,
                "input_schema": schema,
            })
        })
        .collect()
}
