use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

/// Run `maludb mcp` with `input` on stdin and return the parsed response lines.
fn run_mcp(config_dir: &TempDir, input: &str) -> Vec<Value> {
    let output = Command::cargo_bin("maludb")
        .expect("binary exists")
        .env("MALU_CONFIG_DIR", config_dir.path())
        .arg("mcp")
        .write_stdin(input.to_string())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    String::from_utf8(output)
        .expect("utf8 stdout")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect()
}

fn create_profile(config_dir: &TempDir) {
    Command::cargo_bin("maludb")
        .expect("binary exists")
        .env("MALU_CONFIG_DIR", config_dir.path())
        .args([
            "profile",
            "create",
            "maludb-api",
            "--api-url",
            "http://localhost:9",
            "--namespace",
            "default",
        ])
        .assert()
        .success();
}

#[test]
fn initialize_and_tools_list() {
    let config_dir = tempfile::tempdir().expect("temp config dir");
    let input = format!(
        "{}\n{}\n{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}
            }
        }),
        // A notification must not produce a response.
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );

    let responses = run_mcp(&config_dir, &input);
    assert_eq!(responses.len(), 2, "notification should not be answered");

    let init = &responses[0];
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "maludb");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert!(init["result"]["capabilities"].get("tools").is_some());

    let tools = responses[1]["result"]["tools"]
        .as_array()
        .expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for expected in [
        "note",
        "doc_push",
        "get_config",
        "get_note",
        "get_skill",
        "subjects_list",
        "smoke_full",
    ] {
        assert!(names.contains(&expected), "missing tool: {expected}");
    }
    // Every tool advertises an object input schema.
    for tool in tools {
        assert_eq!(
            tool["inputSchema"]["type"], "object",
            "tool {}",
            tool["name"]
        );
    }
}

#[test]
fn unknown_method_and_tool_report_errors() {
    let config_dir = tempfile::tempdir().expect("temp config dir");
    let input = format!(
        "{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 7, "method": "no/such/method"}),
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {"name": "does_not_exist", "arguments": {}}
        }),
    );

    let responses = run_mcp(&config_dir, &input);
    assert_eq!(responses[0]["error"]["code"], -32601);
    assert_eq!(responses[1]["error"]["code"], -32602);
}

#[test]
fn invalid_json_line_gets_parse_error() {
    let config_dir = tempfile::tempdir().expect("temp config dir");
    let responses = run_mcp(&config_dir, "this is not json\n");
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[0]["id"], Value::Null);
}

#[test]
fn tool_call_executes_command_end_to_end() {
    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir);

    let input = format!(
        "{}\n{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "subjects_add", "arguments": {"value": "FastAPI"}}
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "subjects_list", "arguments": {}}
        }),
    );

    let responses = run_mcp(&config_dir, &input);

    let add = &responses[0]["result"];
    assert_eq!(add["isError"], false);

    let list = &responses[1]["result"];
    assert_eq!(list["isError"], false);
    let text = list["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("FastAPI"), "subjects_list output: {text}");
}
