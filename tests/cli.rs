use assert_cmd::Command;
use mockito::Matcher;
use predicates::prelude::*;
use serde_json::json;
use tempfile::TempDir;

fn malu(config_dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("malu").expect("binary exists");
    cmd.env("MALU_CONFIG_DIR", config_dir.path());
    cmd
}

fn create_profile(config_dir: &TempDir, api_url: &str) {
    malu(config_dir)
        .args([
            "profile",
            "create",
            "maludb-api",
            "--api-url",
            api_url,
            "--user-name",
            "Craig",
            "--project",
            "maludb api",
            "--namespace",
            "default",
        ])
        .assert()
        .success();
}

fn set_file_token(config_dir: &TempDir) {
    malu(config_dir)
        .args(["set-token", "malu_testtoken", "--store", "file"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Stored token for profile maludb-api",
        ));
}

#[test]
fn set_api_bootstraps_default_profile_when_none_exists() {
    let config_dir = tempfile::tempdir().expect("temp config dir");

    malu(&config_dir)
        .args(["set-api", "https://api.maludb.org"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Updated API URL for profile default",
        ));

    malu(&config_dir)
        .args(["profile", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Profile: default"))
        .stdout(predicate::str::contains("API URL: https://api.maludb.org"));
}

#[test]
fn profile_create_sets_active_profile_and_show_displays_context() {
    let config_dir = tempfile::tempdir().expect("temp config dir");

    malu(&config_dir)
        .args([
            "profile",
            "create",
            "maludb-api",
            "--api-url",
            "http://localhost:8000",
            "--user-name",
            "Craig",
            "--project",
            "maludb api",
            "--namespace",
            "default",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created profile maludb-api"));

    malu(&config_dir)
        .args(["profile", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Profile: maludb-api"))
        .stdout(predicate::str::contains("API URL: http://localhost:8000"))
        .stdout(predicate::str::contains("User: Craig"))
        .stdout(predicate::str::contains("Project: maludb api"))
        .stdout(predicate::str::contains("Namespace: default"));

    let config = std::fs::read_to_string(config_dir.path().join("config.toml"))
        .expect("config file should be written");
    assert!(config.contains("active_profile = \"maludb-api\""));
    assert!(config.contains("[profiles.maludb-api]"));
}

#[test]
fn subjects_and_hints_apply_to_active_profile() {
    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, "http://localhost:8000");

    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added subject FastAPI"));

    malu(&config_dir)
        .args(["hints", "add", "This is about API smoke testing"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added hint"));

    malu(&config_dir)
        .args(["profile", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Subjects: FastAPI"))
        .stdout(predicate::str::contains(
            "Hints: This is about API smoke testing",
        ));
}

#[test]
fn smoke_health_reports_pass_for_healthy_api() {
    let mut server = mockito::Server::new();
    let health = server
        .mock("GET", "/health")
        .with_status(200)
        .with_body(r#"{"status":"ok"}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());

    malu(&config_dir)
        .args(["smoke", "health"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS health"))
        .stdout(predicate::str::contains("ok"));

    health.assert();
}

#[test]
fn api_errors_preserve_server_error_code_and_message() {
    let mut server = mockito::Server::new();
    let config = server
        .mock("GET", "/v1/memory/config")
        .match_header("authorization", "Bearer malu_testtoken")
        .with_status(503)
        .with_body(r#"{"error":{"code":"memory_unavailable","message":"model offline"}}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args(["smoke", "config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "API error memory_unavailable: model offline",
        ));

    config.assert();
}

#[test]
fn set_token_file_store_keeps_raw_token_out_of_main_config() {
    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, "http://localhost:8000");

    set_file_token(&config_dir);

    let config = std::fs::read_to_string(config_dir.path().join("config.toml"))
        .expect("config file should be written");
    assert!(config.contains("token_key = \"maludb-api\""));
    assert!(!config.contains("malu_testtoken"));

    let credentials = std::fs::read_to_string(config_dir.path().join("credentials.toml"))
        .expect("credential file should be written");
    assert!(credentials.contains("malu_testtoken"));
}

#[test]
fn note_posts_contextualized_text_to_memory_documents() {
    let mut server = mockito::Server::new();
    let ingest = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""source_type":"note""#.to_string()),
            Matcher::Regex(r#""subjects":\["FastAPI"\]"#.to_string()),
            Matcher::Regex(r#""hints":\["This is about API smoke testing"\]"#.to_string()),
            Matcher::Regex(r#"Context:\\n- User: Craig"#.to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":42,"edges":[]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();
    malu(&config_dir)
        .args(["hints", "add", "This is about API smoke testing"])
        .assert()
        .success();

    malu(&config_dir)
        .args(["note", "Starting to debug the maludb api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Ingested note as document 42"));

    ingest.assert();
}

#[test]
fn doc_push_reads_file_and_posts_document_to_memory_pipeline() {
    let mut server = mockito::Server::new();
    let ingest = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::PartialJson(json!({
            "title": "sample.md",
            "source_type": "document",
            "media_type": "text/markdown",
            "namespace": "default",
            "subjects": ["FastAPI"],
        })))
        .with_status(201)
        .with_body(r#"{"document_id":77,"edges":[]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();
    let doc_path = config_dir.path().join("sample.md");
    std::fs::write(&doc_path, "# Debug log\n\nThe API health check passed.\n").unwrap();

    malu(&config_dir)
        .args(["doc", "push", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Ingested document sample.md as document 77",
        ));

    ingest.assert();
}

#[test]
fn smoke_search_posts_query_with_compartment_filter() {
    let mut server = mockito::Server::new();
    let search = server
        .mock("POST", "/v1/memory/search")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::PartialJson(json!({
            "query": "debug API",
            "namespace": "default",
            "subject": "FastAPI",
            "limit": 20,
        })))
        .with_status(200)
        .with_body(r#"{"namespace":"default","results":[{"document_id":77}]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args([
            "smoke",
            "search",
            "--query",
            "debug API",
            "--subject",
            "FastAPI",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS search 1 result"));

    search.assert();
}

#[test]
fn smoke_note_ingests_generated_note_with_provided_edge() {
    let mut server = mockito::Server::new();
    let note = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""title":"malu smoke note""#.to_string()),
            Matcher::Regex(r#""source_type":"note""#.to_string()),
            Matcher::Regex(r#""edges":\["#.to_string()),
            Matcher::Regex(r#""subject_text":"FastAPI""#.to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":201,"edges":[{"statement_id":21}]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();

    malu(&config_dir)
        .args(["smoke", "note"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS note document 201"));

    note.assert();
}

#[test]
fn smoke_document_ingests_file_with_provided_edge() {
    let mut server = mockito::Server::new();
    let document = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""title":"smoke.md""#.to_string()),
            Matcher::Regex(r#""source_type":"document""#.to_string()),
            Matcher::Regex(r#""media_type":"text/markdown""#.to_string()),
            Matcher::Regex(r#""edges":\["#.to_string()),
            Matcher::Regex(r#""subject_text":"FastAPI""#.to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":202,"edges":[{"statement_id":22}]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();
    let doc_path = config_dir.path().join("smoke.md");
    std::fs::write(&doc_path, "# Smoke\n\nGenerated smoke document.\n").unwrap();

    malu(&config_dir)
        .args(["smoke", "document", doc_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS document 202"));

    document.assert();
}

#[test]
fn get_commands_print_api_resources() {
    let mut server = mockito::Server::new();
    let config = server
        .mock("GET", "/v1/memory/config")
        .match_header("authorization", "Bearer malu_testtoken")
        .with_status(200)
        .with_body(r#"{"namespace":"default","config":{"embedding_model":"maludb-local-dev"}}"#)
        .create();
    let subjects = server
        .mock("GET", "/v1/subjects")
        .match_header("authorization", "Bearer malu_testtoken")
        .with_status(200)
        .with_body(r#"{"subjects":[{"id":1,"label":"FastAPI","type":"technology"}]}"#)
        .create();
    let projects = server
        .mock("GET", "/v1/projects")
        .match_header("authorization", "Bearer malu_testtoken")
        .with_status(200)
        .with_body(r#"{"projects":[{"id":2,"name":"maludb api"}]}"#)
        .create();
    let documents = server
        .mock("GET", "/v1/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .with_status(200)
        .with_body(r#"{"documents":[{"id":77,"title":"sample.md","source_type":"document"}]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args(["get", "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Memory config default"))
        .stdout(predicate::str::contains("maludb-local-dev"));
    malu(&config_dir)
        .args(["get", "subjects"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 FastAPI subject"));
    malu(&config_dir)
        .args(["get", "projects"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 maludb api"));
    malu(&config_dir)
        .args(["get", "documents"])
        .assert()
        .success()
        .stdout(predicate::str::contains("77 sample.md document"));

    config.assert();
    subjects.assert();
    projects.assert();
    documents.assert();
}

#[test]
fn smoke_full_runs_memory_pipeline_workflow() {
    let mut server = mockito::Server::new();
    let health = server
        .mock("GET", "/health")
        .with_status(200)
        .with_body(r#"{"status":"ok"}"#)
        .create();
    let subjects = server
        .mock("GET", "/v1/subjects")
        .match_header("authorization", "Bearer malu_testtoken")
        .with_status(200)
        .with_body(r#"{"subjects":[{"id":1,"label":"FastAPI","type":"technology"}]}"#)
        .create();
    let config = server
        .mock("GET", "/v1/memory/config")
        .match_header("authorization", "Bearer malu_testtoken")
        .with_status(200)
        .with_body(r#"{"namespace":"default","config":{"embedding_model":"maludb-local-dev"}}"#)
        .create();
    let note = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""source_type":"note""#.to_string()),
            Matcher::Regex(r#""edges":\["#.to_string()),
            Matcher::Regex(r#""subject_text":"FastAPI""#.to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":101,"edges":[{"statement_id":11}]}"#)
        .create();
    let document = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""source_type":"document""#.to_string()),
            Matcher::Regex(r#""media_type":"text/markdown""#.to_string()),
            Matcher::Regex(r#""edges":\["#.to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":102,"edges":[{"statement_id":12}]}"#)
        .create();
    let search = server
        .mock("POST", "/v1/memory/search")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::PartialJson(json!({
            "namespace": "default",
            "subject": "FastAPI",
            "limit": 20,
        })))
        .with_status(200)
        .with_body(r#"{"namespace":"default","results":[{"document_id":101},{"document_id":102}]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();

    malu(&config_dir)
        .args(["smoke", "full"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS health ok"))
        .stdout(predicate::str::contains("PASS auth subjects"))
        .stdout(predicate::str::contains("PASS config default"))
        .stdout(predicate::str::contains("PASS note document 101"))
        .stdout(predicate::str::contains("PASS document 102"))
        .stdout(predicate::str::contains("PASS search 2 results"));

    health.assert();
    subjects.assert();
    config.assert();
    note.assert();
    document.assert();
    search.assert();
}
