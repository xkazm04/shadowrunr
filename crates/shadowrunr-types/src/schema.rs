//! JSON Schema export of the whole wire contract, the input of `@shadowrunr/contract`'s
//! generator (`packages/contract/scripts/generate.mjs`).
//!
//! One file, `contract.schema.json`: the root describes the RPC method table (`RpcMethods`:
//! method -> `{params, result}`, each a `$ref`), and `$defs` holds every wire type, a
//! `<Pascal>Params` / `<Pascal>Result` def where a method's params or result is not a named
//! type (e.g. `PreflightRunResult = HealthReport[]`), and `ServerFrame` (anything `/events`
//! may push).
//! Schemas describe what the daemon *serializes*; optional fields are omitted, never `null`,
//! so a `null` in an optional field fails validation.

use std::collections::BTreeMap;

use schemars::generate::{SchemaGenerator, SchemaSettings};
use schemars::JsonSchema;
use serde_json::{json, Map, Value};

use crate::catalog::*;
use crate::reconcile;
use crate::rpc::{self, MethodVisitor};
use crate::*;

/// The schema file name inside `packages/contract/schema/`.
pub const CONTRACT_FILE: &str = "contract.schema.json";

fn generator() -> SchemaGenerator {
    SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
}

struct Methods<'g> {
    gen: &'g mut SchemaGenerator,
    defs: Vec<(String, Value)>,
    table: Map<String, Value>,
}

impl Methods<'_> {
    /// A named type is referenced as itself; anything else (`Vec<T>`, a map) becomes the named
    /// def `name`, so every method's params and result have a TS type name.
    fn named(&mut self, schema: Value, name: String) -> Value {
        if schema.get("$ref").is_some() {
            return schema;
        }
        let r = json!({ "$ref": format!("#/$defs/{name}") });
        self.defs.push((name, schema));
        r
    }
}

impl MethodVisitor for Methods<'_> {
    fn method<P: JsonSchema, R: JsonSchema>(&mut self, name: &'static str) {
        let base = rpc::pascal_name(name);
        let p = self.gen.subschema_for::<P>().to_value();
        let r = self.gen.subschema_for::<R>().to_value();
        let params = self.named(p, format!("{base}Params"));
        let result = self.named(r, format!("{base}Result"));
        self.table.insert(
            name.to_string(),
            json!({
                "type": "object",
                "properties": { "params": params, "result": result },
                "required": ["params", "result"],
                "additionalProperties": false
            }),
        );
    }
}

/// Every wire type that is not reached through a method, so it is still in the contract.
fn wire_types(gen: &mut SchemaGenerator) {
    gen.subschema_for::<StreamEvent>();
    gen.subschema_for::<ChatMsg>();
    gen.subschema_for::<Viewer>();
    gen.subschema_for::<Command>();
    gen.subschema_for::<Op>();
    gen.subschema_for::<Case>();
    gen.subschema_for::<Hex>();
    gen.subschema_for::<SegmentContext>();
    gen.subschema_for::<SegmentStatus>();
    gen.subschema_for::<EngineRequest>();
    gen.subschema_for::<EngineReply>();
    gen.subschema_for::<EngineError>();
    gen.subschema_for::<HealthReport>();
    gen.subschema_for::<Episode>();
    gen.subschema_for::<Fact>();
    gen.subschema_for::<SimRunSpec>();
    gen.subschema_for::<JudgeVerdict>();
    gen.subschema_for::<ErrorCode>();
    gen.subschema_for::<ErrorEnvelope>();
    gen.subschema_for::<rpc::RpcRequest>();
    gen.subschema_for::<rpc::Subscribe>();
    gen.subschema_for::<rpc::EventFrame>();
    gen.subschema_for::<crate::sandbox::AckFrame>();
    gen.subschema_for::<crate::review::SessionTags>();
    // The keys of `ClipActs`: the owner-act vocabulary, named on its own.
    gen.subschema_for::<crate::clips::ClipAct>();
    gen.subschema_for::<crate::prep::StreamPrep>();
    gen.subschema_for::<crate::voice::VoiceFrame>();
    gen.subschema_for::<crate::voice::VoiceFollowFrame>();
    gen.subschema_for::<crate::voice::VoiceClientFrame>();
    gen.subschema_for::<reconcile::SegmentMetrics>();
    gen.subschema_for::<CharacterRuleInfo>();
}

/// Two passes over every object schema, so the schema says what a reader may receive:
/// - optional (not `required`) properties are omitted when absent, never `null`: drop the
///   `null` alternative schemars adds for `Option<T>`;
/// - a property with a serde `default` may be absent (stored transcripts written before the
///   field existed are replayed verbatim), so it leaves `required`.
fn normalize(v: &mut Value) {
    match v {
        Value::Object(obj) => {
            let required: Vec<String> = obj
                .get("required")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let mut defaulted = Vec::new();
            if let Some(Value::Object(props)) = obj.get_mut("properties") {
                for (name, prop) in props.iter_mut() {
                    if !required.contains(name) {
                        drop_null(prop);
                    }
                    if prop.get("default").is_some() {
                        defaulted.push(name.clone());
                    }
                }
            }
            if let Some(Value::Array(req)) = obj.get_mut("required") {
                req.retain(|r| r.as_str().is_none_or(|n| !defaulted.iter().any(|d| d == n)));
            }
            for child in obj.values_mut() {
                normalize(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize),
        _ => {}
    }
}

fn drop_null(prop: &mut Value) {
    let Value::Object(obj) = prop else { return };
    if let Some(Value::Array(types)) = obj.get_mut("type") {
        types.retain(|t| t != "null");
        if types.len() == 1 {
            let only = types.remove(0);
            obj.insert("type".into(), only);
        }
    }
    let collapse = if let Some(Value::Array(alts)) = obj.get_mut("anyOf") {
        alts.retain(|a| a != &json!({ "type": "null" }));
        (alts.len() == 1).then(|| alts.remove(0))
    } else {
        None
    };
    if let Some(Value::Object(only)) = collapse {
        obj.remove("anyOf");
        for (k, val) in only {
            obj.entry(k).or_insert(val);
        }
    }
}

/// The contract schema as a JSON value (deterministic: same Rust types, same value).
pub fn contract_schema() -> Value {
    let mut gen = generator();
    let mut methods = Methods {
        gen: &mut gen,
        defs: Vec::new(),
        table: Map::new(),
    };
    rpc::visit_methods(&mut methods);
    let Methods {
        defs: method_defs,
        table,
        ..
    } = methods;
    wire_types(&mut gen);
    let mut defs: BTreeMap<String, Value> = gen.take_definitions(true).into_iter().collect();
    for (name, schema) in method_defs {
        let clash = defs.insert(name.clone(), schema);
        assert!(clash.is_none(), "schema name {name} is used twice");
    }
    defs.insert(
        "ServerFrame".into(),
        json!({
            "description": "Anything `/events` may push: an enveloped event (after a `since` subscribe), an error frame, or a bare StreamEvent (legacy mode).",
            "anyOf": [
                { "$ref": "#/$defs/EventFrame" },
                { "$ref": "#/$defs/ErrorEnvelope" },
                { "$ref": "#/$defs/StreamEvent" }
            ]
        }),
    );
    let names: Vec<Value> = table.keys().map(|k| Value::String(k.clone())).collect();
    let mut root = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "urn:shadowrunr:contract",
        "title": "RpcMethods",
        "description": "Shadowrunr OS wire contract, generated from crates/shadowrunr-types. Do not edit; run `pnpm -C packages/contract generate`.",
        "type": "object",
        "properties": Value::Object(table),
        "required": names,
        "additionalProperties": false,
        "$defs": defs,
    });
    normalize(&mut root);
    root
}

/// `file name -> file text` for `packages/contract/schema/`: pretty JSON, LF, trailing newline.
pub fn export_files() -> BTreeMap<&'static str, String> {
    let text = serde_json::to_string_pretty(&contract_schema()).expect("schema serializes") + "\n";
    BTreeMap::from([(CONTRACT_FILE, text)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_is_deterministic() {
        assert_eq!(export_files(), export_files());
    }

    #[test]
    fn every_method_has_params_and_result_defs() {
        let s = contract_schema();
        for m in rpc::method_names() {
            for part in ["params", "result"] {
                let r = s["properties"][m]["properties"][part]["$ref"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{m}.{part} is not a $ref"));
                let name = r.trim_start_matches("#/$defs/");
                assert!(s["$defs"][name].is_object(), "{m}.{part} -> {name}");
            }
        }
    }

    #[test]
    fn optional_fields_are_not_nullable() {
        let s = contract_schema();
        let hex = &s["$defs"]["Hex"]["properties"]["grade"];
        assert!(!hex.to_string().contains("null"), "{hex}");
        // A required Option without skip_serializing_if stays nullable (it is written as null).
        let rl = s["$defs"]["EngineError"].to_string();
        assert!(rl.contains("null"), "{rl}");
    }

    #[test]
    fn check_ids_cross_the_contract_as_enums() {
        let s = contract_schema();
        for (owner, vocab) in [
            ("SandboxCheck", "SandboxCheckId"),
            ("GateCheck", "GateCheckId"),
        ] {
            let id = &s["$defs"][owner]["properties"]["id"];
            assert_eq!(
                id["$ref"].as_str(),
                Some(format!("#/$defs/{vocab}").as_str()),
                "{owner}.id = {id}"
            );
            let members = s["$defs"][vocab]["enum"]
                .as_array()
                .unwrap_or_else(|| panic!("{vocab} is not a plain enum: {}", s["$defs"][vocab]));
            assert_eq!(members.len(), 10, "{vocab}");
        }
    }

    #[test]
    fn defaulted_fields_are_not_required() {
        let s = contract_schema();
        let req = s["$defs"]["Viewer"]["required"].to_string();
        assert!(!req.contains("months") && req.contains("rank"), "{req}");
    }
}
