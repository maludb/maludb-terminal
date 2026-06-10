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

fn malu_keyring_disabled(config_dir: &TempDir) -> Command {
    let mut cmd = malu(config_dir);
    cmd.env("MALU_KEYRING_DISABLED", "1");
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
fn completions_prints_requested_shell_script() {
    let config_dir = tempfile::tempdir().expect("temp config dir");

    malu(&config_dir)
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_malu"))
        .stdout(predicate::str::contains("profile"));
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
fn set_token_defaults_to_keyring_and_falls_back_to_file_when_disabled() {
    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, "http://localhost:8000");

    malu_keyring_disabled(&config_dir)
        .args(["set-token", "malu_fallbacktoken"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Stored token for profile maludb-api in file credential store",
        ));

    let config = std::fs::read_to_string(config_dir.path().join("config.toml"))
        .expect("config file should be written");
    assert!(config.contains("token_store = \"file\""));
    assert!(!config.contains("malu_fallbacktoken"));

    let credentials = std::fs::read_to_string(config_dir.path().join("credentials.toml"))
        .expect("credential file should be written");
    assert!(credentials.contains("malu_fallbacktoken"));
}

#[test]
fn token_mint_posts_postgres_credentials_and_stores_returned_token() {
    let mut server = mockito::Server::new();
    let mint = server
        .mock("POST", "/v1/tokens")
        .match_body(Matcher::PartialJson(json!({
            "pg_dbname": "maludb",
            "pg_user": "craig",
            "pg_password": "secret",
            "role": "executor",
            "device_name": "macbook",
        })))
        .with_status(201)
        .with_body(r#"{"token":"malu_mintedtoken","id":9,"user_id":1,"role":"executor"}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());

    malu(&config_dir)
        .args([
            "token",
            "mint",
            "--pg-dbname",
            "maludb",
            "--pg-user",
            "craig",
            "--pg-password",
            "secret",
            "--role",
            "executor",
            "--device-name",
            "macbook",
            "--store",
            "file",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Minted and stored token for profile maludb-api",
        ));

    let config = std::fs::read_to_string(config_dir.path().join("config.toml"))
        .expect("config file should be written");
    assert!(!config.contains("malu_mintedtoken"));

    let credentials = std::fs::read_to_string(config_dir.path().join("credentials.toml"))
        .expect("credential file should be written");
    assert!(credentials.contains("malu_mintedtoken"));

    mint.assert();
}

#[test]
fn note_posts_contextualized_text_to_memory_ingest() {
    let mut server = mockito::Server::new();
    let ingest = server
        .mock("POST", "/v1/memory/ingest")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""model":"chatgpt-4o""#.to_string()),
            Matcher::Regex(r#""namespace":"default""#.to_string()),
            Matcher::Regex(r#""subject-type":"project""#.to_string()),
            Matcher::Regex(r#""subject-name":"maludb api""#.to_string()),
            Matcher::Regex(r#""subject-type":"other""#.to_string()),
            Matcher::Regex(r#""subject-name":"FastAPI""#.to_string()),
            Matcher::Regex(r#"This is about API smoke testing"#.to_string()),
            Matcher::Regex(r#"Context:\\n- User: Craig"#.to_string()),
            Matcher::Regex(r#"Note:\\nStarting to debug the maludb api"#.to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":42,"result":{"created":{},"skipped":[]}}"#)
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
fn chat_push_codex_log_posts_normalized_transcript_to_memory_documents() {
    let mut server = mockito::Server::new();
    let ingest = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""title":"codex chat log codex-session.jsonl""#.to_string()),
            Matcher::Regex(r#""source_type":"chat_log""#.to_string()),
            Matcher::Regex(r#""media_type":"application/x-ndjson""#.to_string()),
            Matcher::Regex(r#""chat_source":"codex""#.to_string()),
            Matcher::Regex(r#""subjects":\["FastAPI"\]"#.to_string()),
            Matcher::Regex(r#"Chat Log:\\n"#.to_string()),
            Matcher::Regex("Please inspect the API".to_string()),
            Matcher::Regex("The API is healthy".to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":301,"edges":[]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();
    let chat_path = config_dir.path().join("codex-session.jsonl");
    std::fs::write(
        &chat_path,
        concat!(
            r#"{"type":"session_meta","timestamp":"2026-06-10T00:00:00Z","payload":{"id":"codex-1","cwd":"/repo"}}"#,
            "\n",
            r#"{"type":"response_item","timestamp":"2026-06-10T00:00:01Z","payload":{"role":"user","content":[{"type":"input_text","text":"Please inspect the API"}]}}"#,
            "\n",
            r#"{"type":"response_item","timestamp":"2026-06-10T00:00:02Z","payload":{"role":"assistant","content":[{"type":"output_text","text":"The API is healthy"}]}}"#,
            "\n",
        ),
    )
    .unwrap();

    malu(&config_dir)
        .args([
            "chat",
            "push",
            "--source",
            "codex",
            chat_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Uploaded codex chat log codex-session.jsonl as document 301",
        ));

    ingest.assert();
}

#[test]
fn chat_push_claude_code_log_posts_normalized_transcript_to_memory_documents() {
    let mut server = mockito::Server::new();
    let ingest = server
        .mock("POST", "/v1/memory/documents")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(r#""title":"claude-code chat log claude-session.jsonl""#.to_string()),
            Matcher::Regex(r#""source_type":"chat_log""#.to_string()),
            Matcher::Regex(r#""media_type":"application/x-ndjson""#.to_string()),
            Matcher::Regex(r#""chat_source":"claude-code""#.to_string()),
            Matcher::Regex(r#""subjects":\["FastAPI"\]"#.to_string()),
            Matcher::Regex(r#"Chat Log:\\n"#.to_string()),
            Matcher::Regex("Summarize this bug".to_string()),
            Matcher::Regex("The bug is in auth".to_string()),
        ]))
        .with_status(201)
        .with_body(r#"{"document_id":302,"edges":[]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();
    let chat_path = config_dir.path().join("claude-session.jsonl");
    std::fs::write(
        &chat_path,
        concat!(
            r#"{"type":"user","timestamp":"2026-06-10T00:00:00Z","sessionId":"claude-1","message":{"role":"user","content":"Summarize this bug"}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-06-10T00:00:01Z","sessionId":"claude-1","message":{"role":"assistant","content":[{"type":"text","text":"The bug is in auth"}]}}"#,
            "\n",
        ),
    )
    .unwrap();

    malu(&config_dir)
        .args([
            "chat",
            "push",
            "--source",
            "claude-code",
            chat_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Uploaded claude-code chat log claude-session.jsonl as document 302",
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
fn get_commands_support_query_limit_and_json_output() {
    let mut server = mockito::Server::new();
    let subjects = server
        .mock("GET", "/v1/subjects")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("q".into(), "FastAPI".into()),
            Matcher::UrlEncoded("limit".into(), "5".into()),
        ]))
        .with_status(200)
        .with_body(r#"{"subjects":[{"id":1,"label":"FastAPI","type":"technology"}]}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args([
            "get", "subjects", "--query", "FastAPI", "--limit", "5", "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""subjects""#))
        .stdout(predicate::str::contains(r#""FastAPI""#));

    subjects.assert();
}

#[test]
fn sync_push_creates_remote_settings_note_without_raw_token() {
    let mut server = mockito::Server::new();
    let lookup = server
        .mock("GET", "/v1/notes")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("type".into(), "malu_cli_settings".into()),
            Matcher::UrlEncoded("q".into(), "malu-cli-settings".into()),
        ]))
        .with_status(200)
        .with_body(r#"{"notes":[]}"#)
        .create();
    let create = server
        .mock("POST", "/v1/notes")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::PartialJson(json!({
                "title": "malu-cli-settings",
                "type": "malu_cli_settings",
            })),
            Matcher::Regex(r#""body":"\{.*\\"schema_version\\":1"#.to_string()),
            Matcher::Regex(r#"\\"profiles\\":\{.*\\"maludb-api\\""#.to_string()),
        ]))
        .match_request(|request| {
            !request
                .utf8_lossy_body()
                .expect("request body")
                .contains("malu_testtoken")
        })
        .with_status(201)
        .with_body(r#"{"note":{"id":55,"title":"malu-cli-settings","type":"malu_cli_settings"}}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    malu(&config_dir)
        .args(["subjects", "add", "FastAPI"])
        .assert()
        .success();

    malu(&config_dir)
        .args(["sync", "push"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pushed settings to note 55"));

    lookup.assert();
    create.assert();
}

#[test]
fn sync_push_updates_existing_remote_settings_note() {
    let mut server = mockito::Server::new();
    let lookup = server
        .mock("GET", "/v1/notes")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("type".into(), "malu_cli_settings".into()),
            Matcher::UrlEncoded("q".into(), "malu-cli-settings".into()),
        ]))
        .with_status(200)
        .with_body(r#"{"notes":[{"id":55,"title":"malu-cli-settings","body":"{}","type":"malu_cli_settings"}]}"#)
        .create();
    let update = server
        .mock("PATCH", "/v1/notes/55")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::PartialJson(json!({
            "title": "malu-cli-settings",
            "type": "malu_cli_settings",
        })))
        .with_status(200)
        .with_body(r#"{"note":{"id":55,"title":"malu-cli-settings","type":"malu_cli_settings"}}"#)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args(["sync", "push"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pushed settings to note 55"));

    lookup.assert();
    update.assert();
}

#[test]
fn sync_pull_imports_remote_profiles_without_requiring_remote_token() {
    let remote_body = json!({
        "schema_version": 1,
        "updated_at": "2099-01-01T00:00:00Z",
        "device_id": "other-device",
        "active_profile": "remote",
        "profiles": {
            "remote": {
                "api_url": "https://api.maludb.org",
                "token_key": "remote",
                "token_store": "file",
                "user_name": "Craig",
                "project": "remote project",
                "namespace": "default",
                "subjects": ["FastAPI"],
                "hints": ["remote hint"],
                "updated_at": "2099-01-01T00:00:00Z"
            }
        }
    })
    .to_string();
    let mut server = mockito::Server::new();
    let lookup = server
        .mock("GET", "/v1/notes")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("type".into(), "malu_cli_settings".into()),
            Matcher::UrlEncoded("q".into(), "malu-cli-settings".into()),
        ]))
        .with_status(200)
        .with_body(json!({"notes":[{"id":55,"title":"malu-cli-settings","body":remote_body,"type":"malu_cli_settings"}]}).to_string())
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args(["sync", "pull"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pulled settings from note 55"));

    malu(&config_dir)
        .args(["profile", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Profile: remote"))
        .stdout(predicate::str::contains("Project: remote project"))
        .stdout(predicate::str::contains("Subjects: FastAPI"));

    lookup.assert();
}

#[test]
fn sync_status_and_diff_report_remote_state() {
    let remote_body = json!({
        "schema_version": 1,
        "updated_at": "2099-01-01T00:00:00Z",
        "device_id": "other-device",
        "active_profile": "remote",
        "profiles": {
            "remote": {
                "api_url": "https://api.maludb.org",
                "namespace": "default",
                "subjects": [],
                "hints": [],
                "updated_at": "2099-01-01T00:00:00Z"
            }
        }
    })
    .to_string();
    let mut server = mockito::Server::new();
    let status_lookup = server
        .mock("GET", "/v1/notes")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("type".into(), "malu_cli_settings".into()),
            Matcher::UrlEncoded("q".into(), "malu-cli-settings".into()),
        ]))
        .with_status(200)
        .with_body(json!({"notes":[{"id":55,"title":"malu-cli-settings","body":remote_body,"type":"malu_cli_settings"}]}).to_string())
        .expect(2)
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args(["sync", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Remote settings note 55"))
        .stdout(predicate::str::contains("Remote profiles: 1"));

    malu(&config_dir)
        .args(["sync", "diff"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Only local: maludb-api"))
        .stdout(predicate::str::contains("Only remote: remote"));

    status_lookup.assert();
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

// ---------------------------------------------------------------------------
// Skill commands — push / push-all / list / pull
// ---------------------------------------------------------------------------

fn write_skill_fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let skill_dir = dir.join("pdf-processing");
    std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: pdf-processing\ndescription: Extract text from PDF files. Use when working with PDFs.\nmetadata:\n  version: \"1.0\"\n---\n\n# PDF processing\n\nExtract text from PDF files.\n",
    )
    .unwrap();
    std::fs::write(
        skill_dir.join("scripts").join("extract.py"),
        "#!/usr/bin/env python3\nprint(\"extract\")\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            skill_dir.join("scripts").join("extract.py"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    skill_dir
}

#[test]
fn skill_push_uploads_bundle_with_frontmatter_and_executable_bit() {
    let mut server = mockito::Server::new();
    let ingest = server
        .mock("POST", "/v1/skills/ingest")
        .match_header("authorization", "Bearer malu_testtoken")
        .match_body(Matcher::AllOf(vec![
            Matcher::PartialJson(json!({
                "name": "pdf-processing",
                "frontmatter": {
                    "name": "pdf-processing",
                    "metadata": {"version": "1.0"},
                },
            })),
            Matcher::Regex(r#""relative_path":"SKILL.md""#.to_string()),
            Matcher::Regex(r#""relative_path":"scripts/extract.py""#.to_string()),
            Matcher::Regex(r#""is_executable":true"#.to_string()),
        ]))
        .with_status(201)
        .with_body(
            r#"{"skill_id":6,"version":"1.0","bundle_hash":"abc","reused":false,
               "parent":{"owner_schema":null,"skill_id":null,"note":null},
               "materiality":{"verdict":"material","reasons":["no_parent"]},
               "register":{"skill_id":6,"files_linked":2},
               "ingest":{"created":{"subjects":1}}}"#,
        )
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    let skill_dir = write_skill_fixture(config_dir.path());

    malu(&config_dir)
        .args(["skill", "push", skill_dir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Pushed skill pdf-processing as skill 6 version 1.0 (2 files)",
        ));

    ingest.assert();
}

#[test]
fn skill_push_reports_supersession_and_reuse() {
    let mut server = mockito::Server::new();
    server
        .mock("POST", "/v1/skills/ingest")
        .match_body(Matcher::PartialJson(json!({"materially_different": false})))
        .with_status(201)
        .with_body(
            r#"{"skill_id":8,"version":"1.0+2ac553c0","reused":false,
               "parent":{"owner_schema":"app","skill_id":7,"note":"auto_detected_same_name"},
               "materiality":{"verdict":"non_material"},
               "register":{"skill_id":8,"files_linked":2,"superseded_skill_id":7}}"#,
        )
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    let skill_dir = write_skill_fixture(config_dir.path());

    malu(&config_dir)
        .args(["skill", "push", skill_dir.to_str().unwrap(), "--supersede"])
        .assert()
        .success()
        .stdout(predicate::str::contains("supersedes skill 7"));

    let mut server2 = mockito::Server::new();
    server2
        .mock("POST", "/v1/skills/ingest")
        .with_status(200)
        .with_body(r#"{"skill_id":8,"version":"1.0","reused":true}"#)
        .create();
    malu(&config_dir)
        .args(["set-api", &server2.url()])
        .assert()
        .success();

    malu(&config_dir)
        .args(["skill", "push", skill_dir.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("unchanged"));
}

#[test]
fn skill_list_uses_tag_search_params() {
    let mut server = mockito::Server::new();
    let search = server
        .mock("GET", "/v1/skills")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("verb".into(), "extract".into()),
            Matcher::UrlEncoded("limit".into(), "50".into()),
        ]))
        .with_body(
            r#"{"skills":[{"owner_schema":"app","id":6,"name":"pdf-processing",
                "version":"1.0","description":"Extract text.","score":80.0,
                "match_reasons":["verb"],"source_skill_id":null}]}"#,
        )
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);

    malu(&config_dir)
        .args(["skill", "list", "--verb", "extract"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "6  pdf-processing  1.0  score=80 [verb]",
        ));

    search.assert();
}

#[test]
fn skill_pull_reconstructs_bundle_with_executable_bit() {
    let mut server = mockito::Server::new();
    let script = "#!/usr/bin/env python3\nprint(\"extract\")\n";
    let skill_md = "# PDF processing\n";
    let body = json!({
        "skill": {"id": 6, "name": "pdf-processing", "version": "1.0"},
        "files": [
            {
                "relative_path": "SKILL.md",
                "file_hash": sha256_hex_for_test(skill_md.as_bytes()),
                "file_size": skill_md.len(),
                "is_executable": false,
                "media_type": "text/markdown",
                "content_base64": base64_for_test(skill_md.as_bytes()),
            },
            {
                "relative_path": "scripts/extract.py",
                "file_hash": sha256_hex_for_test(script.as_bytes()),
                "file_size": script.len(),
                "is_executable": true,
                "media_type": "text/x-python",
                "content_base64": base64_for_test(script.as_bytes()),
            }
        ]
    });
    server
        .mock("GET", "/v1/skills/6/bundle")
        .with_body(body.to_string())
        .create();

    let config_dir = tempfile::tempdir().expect("temp config dir");
    create_profile(&config_dir, &server.url());
    set_file_token(&config_dir);
    let dest = config_dir.path().join("pulled");

    malu(&config_dir)
        .args(["skill", "pull", "6", "--dest", dest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Pulled skill pdf-processing version 1.0 (2 files)",
        ));

    assert_eq!(
        std::fs::read_to_string(dest.join("SKILL.md")).unwrap(),
        skill_md
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("scripts/extract.py")).unwrap(),
        script
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dest.join("scripts/extract.py"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "executable bit restored");
    }

    // refuses to overwrite without --force
    malu(&config_dir)
        .args(["skill", "pull", "6", "--dest", dest.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
}

fn sha256_hex_for_test(content: &[u8]) -> String {
    // tests avoid new deps: shell out to sha256sum via std
    use std::io::Write;
    use std::process::{Command as StdCommand, Stdio};
    let mut child = StdCommand::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("sha256sum available");
    child.stdin.as_mut().unwrap().write_all(content).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn base64_for_test(content: &[u8]) -> String {
    use std::io::Write;
    use std::process::{Command as StdCommand, Stdio};
    let mut child = StdCommand::new("base64")
        .arg("-w0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("base64 available");
    child.stdin.as_mut().unwrap().write_all(content).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}
