//! Local MCP (Model Context Protocol) server over stdio.
//!
//! `maludb mcp` runs a JSON-RPC 2.0 loop on stdin/stdout so MCP clients such as
//! Claude Code and Codex can call a curated, safe subset of maludb commands as
//! tools. Each tool maps to a normal CLI invocation: on `tools/call` we re-exec
//! this same binary as a child process and return its captured output. That
//! keeps the existing command handlers untouched, reuses clap's parsing and
//! validation exactly, and isolates a failing command from the server loop.
//!
//! The exposed surface deliberately excludes credential and secret mutation
//! (`set-token`, `token mint`, `llm set-key`, ...): those should not be driven
//! by an LLM. Run those once by hand before pointing a client at the server.

use std::io::{self, BufRead, Write};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::{Value, json};

/// Protocol version we advertise when a client does not request one.
const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the stdio MCP server until stdin closes.
pub(crate) fn serve() -> Result<()> {
    let registry = tools();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.context("failed to read from stdin")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let message: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(err) => {
                let response = error(Value::Null, -32700, &format!("parse error: {err}"));
                write_message(&mut out, &response)?;
                continue;
            }
        };

        if let Some(response) = handle_message(&message, &registry) {
            write_message(&mut out, &response)?;
        }
    }

    Ok(())
}

/// Dispatch a single JSON-RPC message. Returns `None` for notifications (no id)
/// and for unknown notifications, which must not produce a response.
fn handle_message(message: &Value, registry: &[Tool]) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let is_request = id.is_some();
    let id = id.unwrap_or(Value::Null);

    match method {
        "initialize" => Some(success(id, initialize_result(&params))),
        "ping" => Some(success(id, json!({}))),
        "tools/list" => Some(success(id, json!({ "tools": tool_list(registry) }))),
        "tools/call" => Some(handle_tools_call(id, &params, registry)),
        "resources/list" => Some(success(id, json!({ "resources": [] }))),
        "prompts/list" => Some(success(id, json!({ "prompts": [] }))),
        // Notifications (initialized, cancelled, ...) carry no id and get no reply.
        _ if !is_request => None,
        _ => Some(error(id, -32601, &format!("method not found: {method}"))),
    }
}

fn initialize_result(params: &Value) -> Value {
    let protocol = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);
    json!({
        "protocolVersion": protocol,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "maludb",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn handle_tools_call(id: Value, params: &Value, registry: &[Tool]) -> Value {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(name) => name,
        None => return error(id, -32602, "tools/call requires a string \"name\""),
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let tool = match registry.iter().find(|tool| tool.name == name) {
        Some(tool) => tool,
        None => return error(id, -32602, &format!("unknown tool: {name}")),
    };

    // Argument and execution failures are reported as tool results with
    // isError=true (per the MCP spec), not as JSON-RPC protocol errors.
    match (tool.build_argv)(&arguments).and_then(run_cli) {
        Ok((text, is_error)) => success(id, tool_content(&text, is_error)),
        Err(err) => success(id, tool_content(&format!("error: {err:#}"), true)),
    }
}

fn tool_content(text: &str, is_error: bool) -> Value {
    json!({
        "content": [ { "type": "text", "text": text } ],
        "isError": is_error,
    })
}

/// Re-exec this binary with `argv` and capture its output as a tool result.
fn run_cli(argv: Vec<String>) -> Result<(String, bool)> {
    let exe = std::env::current_exe().context("failed to locate the maludb executable")?;
    let output = Command::new(exe)
        .args(&argv)
        .output()
        .with_context(|| format!("failed to run: maludb {}", argv.join(" ")))?;

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim_end();
    if !stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(stderr);
        text.push('\n');
    }
    if text.trim().is_empty() {
        text = format!("(no output; exit status {})\n", output.status);
    }

    Ok((text, !output.status.success()))
}

fn write_message(out: &mut impl Write, message: &Value) -> Result<()> {
    // MCP stdio frames one compact JSON message per line.
    let line = serde_json::to_string(message)?;
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// --- Tool registry -------------------------------------------------------

/// A single MCP tool, defined once and exposed both in `tools/list` (via its
/// JSON Schema) and in `tools/call` (via its argv builder).
struct Tool {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
    build_argv: fn(&Value) -> Result<Vec<String>>,
}

fn tool_list(registry: &[Tool]) -> Vec<Value> {
    registry
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
            })
        })
        .collect()
}

fn tools() -> Vec<Tool> {
    vec![
        // --- writes / actions ---
        Tool {
            name: "note",
            description: "Enrich a short note with the active profile's context and ingest it into MaluDB memory.",
            input_schema: schema(
                json!({
                    "text": p_str("The note text to ingest."),
                    "debug": p_bool("Print the API's full extraction response."),
                }),
                &["text"],
            ),
            build_argv: |args| {
                let mut argv = vec!["note".to_string()];
                push_flag(&mut argv, args, "debug", "--debug");
                argv.push("--".to_string());
                argv.push(req_str(args, "text")?);
                Ok(argv)
            },
        },
        Tool {
            name: "doc_push",
            description: "Upload a document file to MaluDB memory through the ingest pipeline.",
            input_schema: schema(
                json!({ "path": p_str("Path to the document file to upload.") }),
                &["path"],
            ),
            build_argv: |args| {
                Ok(vec![
                    "doc".to_string(),
                    "push".to_string(),
                    "--".to_string(),
                    req_str(args, "path")?,
                ])
            },
        },
        Tool {
            name: "chat_push",
            description: "Upload a Codex or Claude Code chat log as a normalized transcript document.",
            input_schema: schema(
                json!({
                    "source": json!({
                        "type": "string",
                        "enum": ["codex", "claude-code"],
                        "description": "Chat log source format.",
                    }),
                    "path": p_str("Path to the session .jsonl file."),
                }),
                &["source", "path"],
            ),
            build_argv: |args| {
                Ok(vec![
                    "chat".to_string(),
                    "push".to_string(),
                    "--source".to_string(),
                    req_str(args, "source")?,
                    "--".to_string(),
                    req_str(args, "path")?,
                ])
            },
        },
        Tool {
            name: "subjects_add",
            description: "Add a subject to the active profile's context.",
            input_schema: schema(json!({ "value": p_str("Subject to add.") }), &["value"]),
            build_argv: |args| {
                Ok(vec![
                    "subjects".to_string(),
                    "add".to_string(),
                    "--".to_string(),
                    req_str(args, "value")?,
                ])
            },
        },
        Tool {
            name: "hints_add",
            description: "Add an interpretation hint to the active profile's context.",
            input_schema: schema(json!({ "value": p_str("Hint to add.") }), &["value"]),
            build_argv: |args| {
                Ok(vec![
                    "hints".to_string(),
                    "add".to_string(),
                    "--".to_string(),
                    req_str(args, "value")?,
                ])
            },
        },
        Tool {
            name: "skill_add",
            description: "Resolve a Claude Agent Skill by name or path and upload it as a new version.",
            input_schema: schema(
                json!({
                    "target": p_str("Skill name (e.g. php-htmx-auth) or a path to the skill dir / SKILL.md."),
                    "preview": p_bool("Show what would be sent without writing anything."),
                }),
                &["target"],
            ),
            build_argv: |args| {
                let mut argv = vec!["skill".to_string(), "add".to_string(), "--json".to_string()];
                push_flag(&mut argv, args, "preview", "--preview");
                argv.push("--".to_string());
                argv.push(req_str(args, "target")?);
                Ok(argv)
            },
        },
        Tool {
            name: "skill_pull",
            description: "Reconstruct a stored skill bundle into a local directory.",
            input_schema: schema(
                json!({
                    "skill": p_str("Skill id, or a skill name (resolves to its newest enabled version)."),
                    "dest": p_str("Destination directory (default: ./<skill-name>)."),
                    "force": p_bool("Overwrite an existing destination directory."),
                }),
                &["skill"],
            ),
            build_argv: |args| {
                let mut argv = vec![
                    "skill".to_string(),
                    "pull".to_string(),
                    "--json".to_string(),
                ];
                push_opt(&mut argv, args, "dest", "--dest");
                push_flag(&mut argv, args, "force", "--force");
                argv.push("--".to_string());
                argv.push(req_str(args, "skill")?);
                Ok(argv)
            },
        },
        Tool {
            name: "get_skill",
            description: "Download a stored skill and install it into the skills folder (~/.claude/skills/<name> by default) so it is ready to use.",
            input_schema: schema(
                json!({
                    "skill": p_str("Skill id, or a skill name (resolves to its newest enabled version)."),
                    "dest": p_str("Destination directory (default: ~/.claude/skills/<name>)."),
                    "force": p_bool("Overwrite an existing destination directory."),
                }),
                &["skill"],
            ),
            build_argv: |args| {
                let mut argv = vec!["get".to_string(), "skill".to_string(), "--json".to_string()];
                push_opt(&mut argv, args, "dest", "--dest");
                push_flag(&mut argv, args, "force", "--force");
                argv.push("--".to_string());
                argv.push(req_str(args, "skill")?);
                Ok(argv)
            },
        },
        Tool {
            name: "sync_push",
            description: "Push portable CLI settings (profiles, subjects, hints) to MaluDB.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["sync".to_string(), "push".to_string()]),
        },
        Tool {
            name: "sync_pull",
            description: "Pull portable CLI settings from MaluDB into the local config.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["sync".to_string(), "pull".to_string()]),
        },
        // --- reads / diagnostics ---
        Tool {
            name: "get_config",
            description: "Show the server memory configuration for the active profile.",
            input_schema: no_args(),
            build_argv: |_| {
                Ok(vec![
                    "get".to_string(),
                    "config".to_string(),
                    "--json".to_string(),
                ])
            },
        },
        Tool {
            name: "get_subjects",
            description: "Search subjects available on the server.",
            input_schema: schema(
                json!({
                    "query": p_str("Optional search query."),
                    "limit": p_int("Maximum number of results."),
                    "with": p_str("Comma-separated extra fields to include (e.g. attributes)."),
                }),
                &[],
            ),
            build_argv: |args| {
                let mut argv = vec![
                    "get".to_string(),
                    "subjects".to_string(),
                    "--json".to_string(),
                ];
                push_opt(&mut argv, args, "query", "--query");
                push_num(&mut argv, args, "limit", "--limit");
                push_opt(&mut argv, args, "with", "--with");
                Ok(argv)
            },
        },
        Tool {
            name: "get_projects",
            description: "Search projects available on the server.",
            input_schema: schema(
                json!({
                    "query": p_str("Optional search query."),
                    "limit": p_int("Maximum number of results."),
                }),
                &[],
            ),
            build_argv: |args| {
                let mut argv = vec![
                    "get".to_string(),
                    "projects".to_string(),
                    "--json".to_string(),
                ];
                push_opt(&mut argv, args, "query", "--query");
                push_num(&mut argv, args, "limit", "--limit");
                Ok(argv)
            },
        },
        Tool {
            name: "get_documents",
            description: "List or search ingested documents on the server.",
            input_schema: schema(
                json!({
                    "query": p_str("Optional search query."),
                    "limit": p_int("Maximum number of results."),
                    "with": p_str("Comma-separated extra fields to include (e.g. attributes)."),
                }),
                &[],
            ),
            build_argv: |args| {
                let mut argv = vec![
                    "get".to_string(),
                    "documents".to_string(),
                    "--json".to_string(),
                ];
                push_opt(&mut argv, args, "query", "--query");
                push_num(&mut argv, args, "limit", "--limit");
                push_opt(&mut argv, args, "with", "--with");
                Ok(argv)
            },
        },
        Tool {
            name: "get_note",
            description: "Retrieve notes by the subjects/verbs of their extracted edges, or by free text. Provide `query` or at least one of subject_like/verb_like/action.",
            input_schema: schema(
                json!({
                    "query": p_str("Free text, e.g. \"Install Ubuntu\" (parsed server-side)."),
                    "subject_like": p_str_array("Patterns matched anywhere in a subject name or alias."),
                    "verb_like": p_str("Fuzzy verb match (\"installation\" finds the verb \"install\")."),
                    "action": p_str("Exact verb (canonical name or alias, case-insensitive)."),
                    "limit": p_int("Maximum number of results."),
                    "offset": p_int("Result offset for paging."),
                    "all_sources": p_bool("Search every stored document, not just notes."),
                }),
                &[],
            ),
            build_argv: |args| {
                let mut argv = vec!["get".to_string(), "note".to_string(), "--json".to_string()];
                if let Some(values) = args.get("subject_like").and_then(Value::as_array) {
                    for value in values.iter().filter_map(Value::as_str) {
                        argv.push("--subject-like".to_string());
                        argv.push(value.to_string());
                    }
                }
                push_opt(&mut argv, args, "verb_like", "--verb-like");
                push_opt(&mut argv, args, "action", "--action");
                push_num(&mut argv, args, "limit", "--limit");
                push_num(&mut argv, args, "offset", "--offset");
                push_flag(&mut argv, args, "all_sources", "--all-sources");
                if let Some(query) = args.get("query").and_then(Value::as_str) {
                    argv.push("--".to_string());
                    argv.push(query.to_string());
                }
                Ok(argv)
            },
        },
        Tool {
            name: "subjects_list",
            description: "List the active profile's subjects.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["subjects".to_string(), "list".to_string()]),
        },
        Tool {
            name: "hints_list",
            description: "List the active profile's hints.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["hints".to_string(), "list".to_string()]),
        },
        Tool {
            name: "profile_list",
            description: "List configured profiles.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["profile".to_string(), "list".to_string()]),
        },
        Tool {
            name: "profile_show",
            description: "Show the active profile's settings.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["profile".to_string(), "show".to_string()]),
        },
        Tool {
            name: "llm_catalog",
            description: "List the server's model catalog and which provider keys are set.",
            input_schema: schema(
                json!({
                    "task": json!({
                        "type": "string",
                        "enum": ["extract", "skill-extract", "embed"],
                        "description": "Only show models for one task.",
                    }),
                }),
                &[],
            ),
            build_argv: |args| {
                let mut argv = vec![
                    "llm".to_string(),
                    "catalog".to_string(),
                    "--json".to_string(),
                ];
                push_opt(&mut argv, args, "task", "--task");
                Ok(argv)
            },
        },
        Tool {
            name: "llm_models",
            description: "Show the current task -> model choices.",
            input_schema: no_args(),
            build_argv: |_| {
                Ok(vec![
                    "llm".to_string(),
                    "models".to_string(),
                    "--json".to_string(),
                ])
            },
        },
        Tool {
            name: "skill_list",
            description: "List or search stored skills (subject/verb hit the discovery tags).",
            input_schema: schema(
                json!({
                    "query": p_str("Optional search query."),
                    "subject": p_str("Filter by subject tag."),
                    "verb": p_str("Filter by verb tag."),
                    "limit": p_int("Maximum number of results."),
                }),
                &[],
            ),
            build_argv: |args| {
                let mut argv = vec![
                    "skill".to_string(),
                    "list".to_string(),
                    "--json".to_string(),
                ];
                push_opt(&mut argv, args, "query", "--query");
                push_opt(&mut argv, args, "subject", "--subject");
                push_opt(&mut argv, args, "verb", "--verb");
                push_num(&mut argv, args, "limit", "--limit");
                Ok(argv)
            },
        },
        Tool {
            name: "sync_status",
            description: "Show local vs. remote CLI settings sync status.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["sync".to_string(), "status".to_string()]),
        },
        Tool {
            name: "sync_diff",
            description: "Show the diff between local and remote CLI settings.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["sync".to_string(), "diff".to_string()]),
        },
        Tool {
            name: "smoke_health",
            description: "Check the configured API server's /health endpoint.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["smoke".to_string(), "health".to_string()]),
        },
        Tool {
            name: "smoke_config",
            description: "Check authenticated access to the memory configuration endpoint.",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["smoke".to_string(), "config".to_string()]),
        },
        Tool {
            name: "smoke_search",
            description: "Run a memory search against the active profile's context.",
            input_schema: schema(
                json!({
                    "query": p_str("Search query."),
                    "subject": p_str("Optional subject filter."),
                    "verb": p_str("Optional verb filter."),
                    "limit": p_int("Maximum number of results."),
                }),
                &["query"],
            ),
            build_argv: |args| {
                let mut argv = vec![
                    "smoke".to_string(),
                    "search".to_string(),
                    "--query".to_string(),
                    req_str(args, "query")?,
                ];
                push_opt(&mut argv, args, "subject", "--subject");
                push_opt(&mut argv, args, "verb", "--verb");
                push_num(&mut argv, args, "limit", "--limit");
                Ok(argv)
            },
        },
        Tool {
            name: "smoke_full",
            description: "Run the full smoke-test workflow (health, config, note, document, search).",
            input_schema: no_args(),
            build_argv: |_| Ok(vec!["smoke".to_string(), "full".to_string()]),
        },
    ]
}

// --- schema + argument helpers -------------------------------------------

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn no_args() -> Value {
    schema(json!({}), &[])
}

fn p_str(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn p_int(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn p_bool(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

fn p_str_array(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": description })
}

fn req_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("missing required string argument: {key}"))
}

fn push_opt(argv: &mut Vec<String>, args: &Value, key: &str, flag: &str) {
    if let Some(value) = args.get(key).and_then(Value::as_str) {
        argv.push(flag.to_string());
        argv.push(value.to_string());
    }
}

fn push_num(argv: &mut Vec<String>, args: &Value, key: &str, flag: &str) {
    if let Some(value) = args.get(key).and_then(Value::as_i64) {
        argv.push(flag.to_string());
        argv.push(value.to_string());
    }
}

fn push_flag(argv: &mut Vec<String>, args: &Value, key: &str, flag: &str) {
    if args.get(key).and_then(Value::as_bool).unwrap_or(false) {
        argv.push(flag.to_string());
    }
}
