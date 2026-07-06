use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use directories::ProjectDirs;
use keyring_core::Entry;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

mod llm;
mod mcp;
mod skills;
use llm::LlmCommand;
use skills::SkillCommand;

#[derive(Debug, Parser)]
#[command(
    name = "maludb",
    about = "Send notes and smoke-test workflows to MaluDB"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    SetApi {
        api_url: String,
    },
    SetToken {
        token: String,
        #[arg(long, value_enum, default_value_t = TokenStore::Keyring)]
        store: TokenStore,
    },
    /// Pin the model sent with `maludb note` (legacy servers only; new servers
    /// use your `maludb llm use` choice when this is unset)
    SetModel {
        model: Option<String>,
        /// Clear the override so the server resolves the model
        #[arg(long)]
        clear: bool,
    },
    Token {
        #[command(subcommand)]
        command: TokenCommand,
    },
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Subjects {
        #[command(subcommand)]
        command: ListCommand,
    },
    Hints {
        #[command(subcommand)]
        command: ListCommand,
    },
    Get {
        #[command(subcommand)]
        command: GetCommand,
    },
    /// Query and manage the tenant knowledge graph
    Graph {
        #[command(subcommand)]
        command: GraphCommand,
    },
    Note {
        #[arg(long)]
        debug: bool,
        text: String,
    },
    Doc {
        #[command(subcommand)]
        command: DocCommand,
    },
    #[command(visible_alias = "skills")]
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    Llm {
        #[command(subcommand)]
        command: LlmCommand,
    },
    Chat {
        #[command(subcommand)]
        command: ChatCommand,
    },
    Smoke {
        #[command(subcommand)]
        command: SmokeCommand,
    },
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
    },
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Run as a local MCP server over stdio (for Claude Code, Codex, etc.)
    Mcp,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    Create {
        name: String,
        #[arg(long, default_value = "https://api.maludb.org")]
        api_url: String,
        #[arg(long)]
        user_name: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value = "default")]
        namespace: String,
    },
    Use {
        name: String,
    },
    List,
    Show,
    Delete {
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum ListCommand {
    Add { value: String },
    List,
    Clear,
}

#[derive(Debug, Subcommand)]
enum GetCommand {
    Config {
        #[arg(long)]
        json: bool,
    },
    Subjects {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long)]
        json: bool,
        #[arg(long = "with")]
        with_: Option<String>,
    },
    Projects {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long)]
        json: bool,
    },
    Documents {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long)]
        json: bool,
        #[arg(long = "with")]
        with_: Option<String>,
    },
    /// Retrieve notes by the subjects/verbs of their extracted edges
    Note {
        /// Free text, e.g. `maludb get note "Install Ubuntu"` (parsed server-side)
        query: Option<String>,
        /// Pattern matched anywhere in a subject name or alias (repeatable)
        #[arg(long = "subject-like")]
        subject_like: Vec<String>,
        /// Fuzzy verb match: "installation" finds the verb "install"
        #[arg(long = "verb-like")]
        verb_like: Option<String>,
        /// Exact verb (canonical name or alias, case-insensitive)
        #[arg(long)]
        action: Option<String>,
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long)]
        offset: Option<u32>,
        /// Search every stored document, not just notes
        #[arg(long = "all-sources")]
        all_sources: bool,
        #[arg(long)]
        json: bool,
    },
    /// Download a stored skill into your skills folder (~/.claude/skills/<name>)
    Skill {
        /// Skill id, or a skill name (resolves to its newest enabled version)
        skill: String,
        /// Destination directory (default: ~/.claude/skills/<name>)
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Overwrite an existing destination directory
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    /// Ask a question of the graph: lexical seeds + bounded walk
    Query {
        text: String,
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long, default_value_t = 2)]
        depth: u32,
        #[arg(long, default_value_t = 50)]
        max_nodes: u32,
        #[arg(long)]
        json: bool,
    },
    /// One-hop neighbors of a node
    Neighbors {
        id: i64,
        #[arg(long, default_value = "subject")]
        kind: String,
        #[arg(long, default_value = "both")]
        direction: String,
        /// Comma-separated relationship filter
        #[arg(long)]
        rel: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Multi-hop walk from a node
    Walk {
        id: i64,
        #[arg(long, default_value = "subject")]
        kind: String,
        #[arg(long, default_value_t = 4)]
        max_depth: u32,
        #[arg(long, default_value = "both")]
        direction: String,
        /// Comma-separated relationship filter
        #[arg(long)]
        rel: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Paths between two nodes, shortest first
    Path {
        source_id: i64,
        target_id: i64,
        #[arg(long, default_value = "subject")]
        source_kind: String,
        #[arg(long, default_value = "subject")]
        target_kind: String,
        #[arg(long, default_value_t = 6)]
        max_depth: u32,
        #[arg(long, default_value = "both")]
        direction: String,
        /// Comma-separated relationship filter
        #[arg(long)]
        rel: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Node/edge totals for the tenant graph
    Stats {
        #[arg(long)]
        json: bool,
    },
    /// Highest-degree nodes (tenant-wide)
    GodNodes {
        #[arg(long, default_value_t = 10)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Cross-community edges, rarest community pair first
    Surprises {
        namespace: String,
        #[arg(long, default_value_t = 25)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Community sets (optionally scoped to one namespace)
    Communities {
        #[arg(long)]
        namespace: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Members of one community
    Members {
        community_id: i64,
        #[arg(long, default_value_t = 200)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Import a graphify graph.json into the tenant graph
    Import {
        file: PathBuf,
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DocCommand {
    Push { path: PathBuf },
}

#[derive(Debug, Subcommand)]
enum ChatCommand {
    Push {
        #[arg(long, value_enum)]
        source: ChatSource,
        path: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ChatSource {
    Codex,
    ClaudeCode,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum TokenStore {
    Keyring,
    File,
}

#[derive(Debug, Subcommand)]
enum TokenCommand {
    Mint {
        #[arg(long)]
        pg_dbname: String,
        #[arg(long)]
        pg_user: String,
        #[arg(long)]
        pg_password: String,
        #[arg(long, default_value = "executor")]
        role: String,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(long)]
        expires_in_days: Option<u32>,
        #[arg(long, value_enum, default_value_t = TokenStore::Keyring)]
        store: TokenStore,
    },
}

#[derive(Debug, Subcommand)]
enum SmokeCommand {
    Health,
    Config,
    Note,
    Document {
        path: PathBuf,
    },
    Search {
        #[arg(long)]
        query: String,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long)]
        verb: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u16,
    },
    Full,
}

#[derive(Debug, Subcommand)]
enum SyncCommand {
    Push,
    Pull,
    Status,
    Diff,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Config {
    active_profile: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Profile {
    api_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_store: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    /// Legacy model override for `maludb note`; unset = the server resolves
    /// the user's `maludb llm use` choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    namespace: String,
    #[serde(default)]
    subjects: Vec<String>,
    #[serde(default)]
    hints: Vec<String>,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: ServerError,
}

#[derive(Debug, Deserialize)]
struct ServerError {
    code: String,
    message: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Credentials {
    #[serde(default)]
    tokens: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct TokenMintRequest {
    pg_dbname: String,
    pg_user: String,
    pg_password: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TokenMintResponse {
    token: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct SyncBlob {
    schema_version: u16,
    updated_at: String,
    device_id: String,
    active_profile: Option<String>,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Debug)]
struct SettingsNote {
    id: i64,
    body: String,
}

#[derive(Debug, Serialize)]
struct MemoryDocumentRequest {
    title: String,
    text: String,
    namespace: String,
    source_type: String,
    media_type: Option<String>,
    metadata: Value,
    projects: Vec<String>,
    subjects: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edges: Option<Vec<MemoryEdge>>,
}

#[derive(Debug, Serialize)]
struct MemoryIngestRequest {
    /// Omitted by default — the server resolves the user's extract-model
    /// choice. Set per-profile via `maludb set-model` for legacy servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    text: String,
    namespace: String,
    hints: Vec<Value>,
    /// Notes are stamped source_type "note" so `maludb get note` finds them
    /// by default. Servers older than maludb_core 0.98.0 ignore the field.
    source_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

#[derive(Debug, Serialize)]
struct MemoryEdge {
    subject_text: String,
    verb_text: String,
    predicate: Vec<String>,
    subject_type: String,
    source_span: String,
    confidence: f32,
    provenance: String,
}

#[derive(Debug, Deserialize)]
struct MemoryDocumentResponse {
    document_id: i64,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::discover()?;
    handle(cli.command, &paths)
}

fn handle(command: Commands, paths: &Paths) -> Result<()> {
    match command {
        Commands::SetApi { api_url } => set_api(paths, api_url),
        Commands::SetToken { token, store } => set_token(paths, token, store),
        Commands::SetModel { model, clear } => set_model(paths, model, clear),
        Commands::Token { command } => handle_token(paths, command),
        Commands::Profile { command } => handle_profile(paths, command),
        Commands::Subjects { command } => handle_collection(paths, command, Collection::Subjects),
        Commands::Hints { command } => handle_collection(paths, command, Collection::Hints),
        Commands::Get { command } => handle_get(paths, command),
        Commands::Graph { command } => handle_graph(paths, command),
        Commands::Note { text, debug } => handle_note(paths, text, debug),
        Commands::Doc { command } => handle_doc(paths, command),
        Commands::Skill { command } => skills::handle_skill(paths, command),
        Commands::Llm { command } => llm::handle_llm(paths, command),
        Commands::Chat { command } => handle_chat(paths, command),
        Commands::Smoke { command } => handle_smoke(paths, command),
        Commands::Sync { command } => handle_sync(paths, command),
        Commands::Completions { shell } => {
            let mut command = Cli::command();
            generate(shell, &mut command, "maludb", &mut io::stdout());
            Ok(())
        }
        Commands::Mcp => mcp::serve(),
    }
}

fn set_api(paths: &Paths, api_url: String) -> Result<()> {
    let mut config = Config::load(paths)?;
    let active = match config.active_profile.clone() {
        Some(name) => name,
        None => {
            let name = "default".to_string();
            config.active_profile = Some(name.clone());
            config
                .profiles
                .insert(name.clone(), Profile::new(api_url.clone()));
            name
        }
    };

    let profile = config
        .profiles
        .get_mut(&active)
        .context("active profile is missing from config")?;
    profile.api_url = api_url;
    profile.touch();
    config.save(paths)?;
    println!("Updated API URL for profile {active}");
    Ok(())
}

fn set_token(paths: &Paths, token: String, store: TokenStore) -> Result<()> {
    let mut config = Config::load(paths)?;
    let (profile_name, actual_store) =
        store_token_for_active_profile(paths, &mut config, token, store)?;
    config.save(paths)?;
    println!(
        "Stored token for profile {profile_name} in {} credential store",
        actual_store.as_str()
    );
    Ok(())
}

fn set_model(paths: &Paths, model: Option<String>, clear: bool) -> Result<()> {
    let mut config = Config::load(paths)?;
    let (profile_name, profile) = config.active_profile_mut()?;
    match (model, clear) {
        (Some(model), false) => {
            profile.model = Some(model.clone());
            profile.touch();
            config.save(paths)?;
            println!("Set note model override to {model} for profile {profile_name}");
        }
        (None, true) => {
            profile.model = None;
            profile.touch();
            config.save(paths)?;
            println!("Cleared note model override for profile {profile_name}");
        }
        _ => bail!("pass a model name or --clear"),
    }
    Ok(())
}

fn handle_token(paths: &Paths, command: TokenCommand) -> Result<()> {
    match command {
        TokenCommand::Mint {
            pg_dbname,
            pg_user,
            pg_password,
            role,
            device_name,
            expires_in_days,
            store,
        } => {
            let mut config = Config::load(paths)?;
            let (_, profile) = config.active_profile()?;
            let api = ApiClient::new(&profile.api_url, None);
            let request = TokenMintRequest {
                pg_dbname,
                pg_user,
                pg_password,
                role,
                device_name,
                expires_in_days,
            };
            let response: TokenMintResponse = api.post_json("/v1/tokens", &request)?;
            let (profile_name, _) =
                store_token_for_active_profile(paths, &mut config, response.token, store)?;
            config.save(paths)?;
            println!("Minted and stored token for profile {profile_name}");
            Ok(())
        }
    }
}

fn handle_profile(paths: &Paths, command: ProfileCommand) -> Result<()> {
    match command {
        ProfileCommand::Create {
            name,
            api_url,
            user_name,
            project,
            namespace,
        } => {
            let mut config = Config::load(paths)?;
            if config.profiles.contains_key(&name) {
                bail!("Profile {name} already exists");
            }

            let mut profile = Profile::new(api_url);
            profile.user_name = user_name;
            profile.project = project;
            profile.namespace = namespace;
            config.profiles.insert(name.clone(), profile);
            if config.active_profile.is_none() {
                config.active_profile = Some(name.clone());
            }
            config.save(paths)?;
            println!("Created profile {name}");
            Ok(())
        }
        ProfileCommand::Use { name } => {
            let mut config = Config::load(paths)?;
            if !config.profiles.contains_key(&name) {
                bail!("Profile {name} does not exist");
            }
            config.active_profile = Some(name.clone());
            config.save(paths)?;
            println!("Using profile {name}");
            Ok(())
        }
        ProfileCommand::List => {
            let config = Config::load(paths)?;
            if config.profiles.is_empty() {
                println!("No profiles configured");
                return Ok(());
            }

            for name in config.profiles.keys() {
                let marker = if config.active_profile.as_deref() == Some(name.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{marker} {name}");
            }
            Ok(())
        }
        ProfileCommand::Show => {
            let config = Config::load(paths)?;
            let (name, profile) = config.active_profile()?;
            print_profile(name, profile);
            Ok(())
        }
        ProfileCommand::Delete { name } => {
            let mut config = Config::load(paths)?;
            if config.profiles.remove(&name).is_none() {
                bail!("Profile {name} does not exist");
            }
            if config.active_profile.as_deref() == Some(name.as_str()) {
                config.active_profile = config.profiles.keys().next().cloned();
            }
            config.save(paths)?;
            println!("Deleted profile {name}");
            Ok(())
        }
    }
}

fn handle_note(paths: &Paths, text: String, debug: bool) -> Result<()> {
    let config = Config::load(paths)?;
    let (_, profile) = config.active_profile()?;
    let token = config.required_token(paths, profile)?;
    let api = ApiClient::new(&profile.api_url, Some(token));
    let request = memory_ingest_request(profile, &text);
    let request = serde_json::to_value(&request).context("failed to serialize note request")?;
    let response = api.post_value("/v1/memory/ingest", &request)?;
    let document_id = response
        .get("document_id")
        .and_then(Value::as_i64)
        .context("API response missing numeric document_id")?;
    println!("Ingested note into memory {document_id}");
    if debug {
        let body = serde_json::to_string_pretty(&response)
            .context("failed to format debug API response")?;
        println!("{body}");
    }
    Ok(())
}

fn handle_get(paths: &Paths, command: GetCommand) -> Result<()> {
    let config = Config::load(paths)?;
    let (_, profile) = config.active_profile()?;
    let token = config.required_token(paths, profile)?;
    let api = ApiClient::new(&profile.api_url, Some(token));

    match command {
        GetCommand::Config { json } => {
            let body = api.get_json("/v1/memory/config")?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            let namespace = string_field(&body, "namespace", "default");
            let config = body.get("config").unwrap_or(&Value::Null);
            println!("Memory config {namespace} {}", compact_json(config));
            Ok(())
        }
        GetCommand::Subjects {
            query,
            limit,
            json,
            with_,
        } => {
            let params = list_query(query, limit, with_);
            let body = api.get_json_query("/v1/subjects", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_subjects(&body);
            Ok(())
        }
        GetCommand::Projects { query, limit, json } => {
            let params = list_query(query, limit, None);
            let body = api.get_json_query("/v1/projects", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_named_items(&body, "projects", "name");
            Ok(())
        }
        GetCommand::Documents {
            query,
            limit,
            json,
            with_,
        } => {
            let params = list_query(query, limit, with_);
            let body = api.get_json_query("/v1/documents", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_documents(&body);
            Ok(())
        }
        GetCommand::Note {
            query,
            subject_like,
            verb_like,
            action,
            limit,
            offset,
            all_sources,
            json,
        } => {
            if query.is_none() && subject_like.is_empty() && verb_like.is_none() && action.is_none()
            {
                bail!(
                    "Provide free text (maludb get note \"Install Ubuntu\") or at least one of \
                     --subject-like, --verb-like, --action"
                );
            }
            let mut params: Vec<(&str, String)> = Vec::new();
            if let Some(query) = query {
                params.push(("q", query));
            }
            for pattern in subject_like {
                params.push(("subject_like", pattern));
            }
            if let Some(verb_like) = verb_like {
                params.push(("verb_like", verb_like));
            }
            if let Some(action) = action {
                params.push(("action", action));
            }
            if let Some(limit) = limit {
                params.push(("limit", limit.to_string()));
            }
            if let Some(offset) = offset {
                params.push(("offset", offset.to_string()));
            }
            if all_sources {
                params.push(("all_sources", "true".to_string()));
            }
            let body = api.get_json_query("/v1/memory/notes", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_notes(&body);
            Ok(())
        }
        GetCommand::Skill {
            skill,
            dest,
            force,
            json,
        } => skills::get_skill(&api, &skill, dest, force, json),
    }
}

fn handle_doc(paths: &Paths, command: DocCommand) -> Result<()> {
    match command {
        DocCommand::Push { path } => {
            let config = Config::load(paths)?;
            let (_, profile) = config.active_profile()?;
            let token = config.required_token(paths, profile)?;
            let api = ApiClient::new(&profile.api_url, Some(token));
            let title = file_name(&path)?;
            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let media_type = mime_guess::from_path(&path)
                .first_or_text_plain()
                .essence_str()
                .to_string();
            let request = memory_document_request(
                profile,
                &title,
                "document",
                &media_type,
                &text,
                Some(&path),
            );
            let response: MemoryDocumentResponse =
                api.post_json("/v1/memory/documents", &request)?;
            println!(
                "Ingested document {title} as document {}",
                response.document_id
            );
            Ok(())
        }
    }
}

fn handle_chat(paths: &Paths, command: ChatCommand) -> Result<()> {
    match command {
        ChatCommand::Push { source, path } => {
            let config = Config::load(paths)?;
            let (_, profile) = config.active_profile()?;
            let token = config.required_token(paths, profile)?;
            let api = ApiClient::new(&profile.api_url, Some(token));
            let filename = file_name(&path)?;
            let title = format!("{} chat log {filename}", source.as_str());
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let transcript = normalize_chat_log(source, &raw);
            let mut request = memory_document_request(
                profile,
                &title,
                "chat_log",
                "application/x-ndjson",
                &transcript,
                Some(&path),
            );
            if let Value::Object(metadata) = &mut request.metadata {
                metadata.insert(
                    "chat_source".to_string(),
                    Value::String(source.as_str().to_string()),
                );
            }
            let response: MemoryDocumentResponse =
                api.post_json("/v1/memory/documents", &request)?;
            println!(
                "Uploaded {} chat log {filename} as document {}",
                source.as_str(),
                response.document_id
            );
            Ok(())
        }
    }
}

fn handle_sync(paths: &Paths, command: SyncCommand) -> Result<()> {
    let config = Config::load(paths)?;
    let (_, profile) = config.active_profile()?;
    let token = config.required_token(paths, profile)?;
    let api = ApiClient::new(&profile.api_url, Some(token));

    match command {
        SyncCommand::Push => {
            let blob = sync_blob_from_config(paths, &config)?;
            let body = serde_json::to_string(&blob).context("failed to serialize sync blob")?;
            let note_body = serde_json::json!({
                "title": SETTINGS_TITLE,
                "type": SETTINGS_TYPE,
                "body": body,
            });
            let response = match fetch_settings_note(&api)? {
                Some(note) => api.patch_value(&format!("/v1/notes/{}", note.id), &note_body)?,
                None => api.post_value("/v1/notes", &note_body)?,
            };
            let id = note_id(&response).unwrap_or(0);
            println!("Pushed settings to note {id}");
            Ok(())
        }
        SyncCommand::Pull => {
            let note =
                fetch_settings_note(&api)?.context("No remote malu-cli-settings note found")?;
            let blob: SyncBlob =
                serde_json::from_str(&note.body).context("failed to parse remote settings blob")?;
            let mut next = Config {
                active_profile: blob.active_profile,
                profiles: blob.profiles,
            };
            preserve_local_token_settings(&config, &mut next);
            next.save(paths)?;
            println!("Pulled settings from note {}", note.id);
            Ok(())
        }
        SyncCommand::Status => {
            match fetch_settings_note(&api)? {
                Some(note) => {
                    let blob: SyncBlob = serde_json::from_str(&note.body)
                        .context("failed to parse remote settings blob")?;
                    println!("Remote settings note {}", note.id);
                    println!("Remote updated: {}", blob.updated_at);
                    println!("Remote profiles: {}", blob.profiles.len());
                    println!("Local profiles: {}", config.profiles.len());
                }
                None => {
                    println!("No remote settings note found");
                    println!("Local profiles: {}", config.profiles.len());
                }
            }
            Ok(())
        }
        SyncCommand::Diff => {
            let Some(note) = fetch_settings_note(&api)? else {
                println!("No remote settings note found");
                return Ok(());
            };
            let blob: SyncBlob =
                serde_json::from_str(&note.body).context("failed to parse remote settings blob")?;
            let mut local_only = Vec::new();
            let mut remote_only = Vec::new();
            let mut both = Vec::new();

            for name in config.profiles.keys() {
                if blob.profiles.contains_key(name) {
                    both.push(name.clone());
                } else {
                    local_only.push(name.clone());
                }
            }
            for name in blob.profiles.keys() {
                if !config.profiles.contains_key(name) {
                    remote_only.push(name.clone());
                }
            }

            print_name_list("Only local", &local_only);
            print_name_list("Only remote", &remote_only);
            print_name_list("Both", &both);
            Ok(())
        }
    }
}

fn handle_collection(paths: &Paths, command: ListCommand, collection: Collection) -> Result<()> {
    let mut config = Config::load(paths)?;
    let (_, profile) = config.active_profile_mut()?;
    let values = collection.values_mut(profile);

    match command {
        ListCommand::Add { value } => {
            if !values.contains(&value) {
                values.push(value.clone());
                profile.touch();
                config.save(paths)?;
            }
            println!("Added {} {value}", collection.singular());
        }
        ListCommand::List => {
            if values.is_empty() {
                println!("No {} configured", collection.plural());
            } else {
                println!("{}", values.join("\n"));
            }
        }
        ListCommand::Clear => {
            values.clear();
            profile.touch();
            config.save(paths)?;
            println!("Cleared {}", collection.plural());
        }
    }
    Ok(())
}

fn handle_smoke(paths: &Paths, command: SmokeCommand) -> Result<()> {
    let config = Config::load(paths)?;
    let (_, profile) = config.active_profile()?;

    match command {
        SmokeCommand::Health => {
            let api = ApiClient::new(&profile.api_url, None);
            let body = api.get_json("/health")?;
            let status = body
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            println!("PASS health {status}");
            Ok(())
        }
        SmokeCommand::Config => {
            let token = config.required_token(paths, profile)?;
            let api = ApiClient::new(&profile.api_url, Some(token));
            let body = api.get_json("/v1/memory/config")?;
            println!("PASS config {}", compact_json(&body));
            Ok(())
        }
        SmokeCommand::Note => {
            let token = config.required_token(paths, profile)?;
            let api = ApiClient::new(&profile.api_url, Some(token));
            let smoke_subject = smoke_subject_from_profile(profile);
            let note = ingest_smoke_note(&api, profile, &smoke_subject, &now())?;
            println!("PASS note document {}", note.document_id);
            Ok(())
        }
        SmokeCommand::Document { path } => {
            let token = config.required_token(paths, profile)?;
            let api = ApiClient::new(&profile.api_url, Some(token));
            let smoke_subject = smoke_subject_from_profile(profile);
            let document =
                ingest_smoke_document(&api, profile, &smoke_subject, &now(), Some(&path))?;
            println!("PASS document {}", document.document_id);
            Ok(())
        }
        SmokeCommand::Search {
            query,
            subject,
            verb,
            limit,
        } => {
            let token = config.required_token(paths, profile)?;
            let api = ApiClient::new(&profile.api_url, Some(token));
            let subject = subject.or_else(|| profile.subjects.first().cloned());
            if subject.is_none() && verb.is_none() {
                bail!("smoke search requires --subject, --verb, or an active profile subject");
            }

            let body = serde_json::json!({
                "query": query,
                "namespace": profile.namespace,
                "subject": subject,
                "verb": verb,
                "limit": limit,
            });
            let response = api.post_value("/v1/memory/search", &body)?;
            let count = response
                .get("results")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            let label = if count == 1 { "result" } else { "results" };
            println!("PASS search {count} {label}");
            Ok(())
        }
        SmokeCommand::Full => smoke_full(paths, &config, profile),
    }
}

fn smoke_full(paths: &Paths, config: &Config, profile: &Profile) -> Result<()> {
    let public_api = ApiClient::new(&profile.api_url, None);
    let health = public_api.get_json("/health")?;
    let health_status = health
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    println!("PASS health {health_status}");

    let token = config.required_token(paths, profile)?;
    let api = ApiClient::new(&profile.api_url, Some(token));

    let subjects = api.get_json("/v1/subjects")?;
    let subject_count = array_len(&subjects, "subjects");
    println!("PASS auth subjects {subject_count}");

    let memory_config = api.get_json("/v1/memory/config")?;
    let namespace = string_field(&memory_config, "namespace", &profile.namespace);
    println!("PASS config {namespace}");

    let smoke_subject = smoke_subject(profile, &subjects);
    let stamp = now();
    let note = ingest_smoke_note(&api, profile, &smoke_subject, &stamp)?;
    println!("PASS note document {}", note.document_id);

    let document = ingest_smoke_document(&api, profile, &smoke_subject, &stamp, None)?;
    println!("PASS document {}", document.document_id);

    let search_body = serde_json::json!({
        "query": format!("MaluDB CLI smoke {stamp}"),
        "namespace": profile.namespace,
        "subject": smoke_subject,
        "limit": 20,
    });
    let search = api.post_value("/v1/memory/search", &search_body)?;
    let count = array_len(&search, "results");
    let label = if count == 1 { "result" } else { "results" };
    println!("PASS search {count} {label}");
    Ok(())
}

fn ingest_smoke_note(
    api: &ApiClient,
    profile: &Profile,
    smoke_subject: &str,
    stamp: &str,
) -> Result<MemoryDocumentResponse> {
    let note_text = format!("MaluDB CLI smoke note generated at {stamp}");
    let mut request = memory_document_request(
        profile,
        "maludb smoke note",
        "note",
        "text/plain",
        &note_text,
        None,
    );
    ensure_subject(&mut request, smoke_subject);
    request.edges = Some(vec![smoke_edge(smoke_subject, "recorded", &note_text)]);
    api.post_json("/v1/memory/documents", &request)
}

fn ingest_smoke_document(
    api: &ApiClient,
    profile: &Profile,
    smoke_subject: &str,
    stamp: &str,
    path: Option<&Path>,
) -> Result<MemoryDocumentResponse> {
    let (title, media_type, text, source_path) = match path {
        Some(path) => {
            let title = file_name(path)?;
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let media_type = mime_guess::from_path(path)
                .first_or_text_plain()
                .essence_str()
                .to_string();
            (title, media_type, text, Some(path))
        }
        None => (
            "maludb-smoke-document.md".to_string(),
            "text/markdown".to_string(),
            format!("# MaluDB CLI smoke document\n\nGenerated at {stamp}.\n"),
            None,
        ),
    };

    let mut request =
        memory_document_request(profile, &title, "document", &media_type, &text, source_path);
    ensure_subject(&mut request, smoke_subject);
    request.edges = Some(vec![smoke_edge(smoke_subject, "documented", &text)]);
    api.post_json("/v1/memory/documents", &request)
}

fn memory_document_request(
    profile: &Profile,
    title: &str,
    source_type: &str,
    media_type: &str,
    content: &str,
    source_path: Option<&Path>,
) -> MemoryDocumentRequest {
    let body_label = match source_type {
        "note" => "Note",
        "chat_log" => "Chat Log",
        _ => "Document",
    };
    let text = format!("{}\n{body_label}:\n{content}", context_preamble(profile));
    let metadata = serde_json::json!({
        "source": "maludb-cli",
        "source_type": source_type,
        "hints": profile.hints,
        "user_name": profile.user_name,
        "project": profile.project,
        "source_path": source_path.map(|path| path.display().to_string()),
        "created_at": now(),
    });

    MemoryDocumentRequest {
        title: title.to_string(),
        text,
        namespace: profile.namespace.clone(),
        source_type: source_type.to_string(),
        media_type: Some(media_type.to_string()),
        metadata,
        projects: profile.project.iter().cloned().collect(),
        subjects: profile.subjects.clone(),
        edges: None,
    }
}

fn memory_ingest_request(profile: &Profile, content: &str) -> MemoryIngestRequest {
    let text = format!("{}\nNote:\n{content}", context_preamble(profile));
    let mut hints = Vec::new();

    if let Some(project) = profile
        .project
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        hints.push(serde_json::json!({
            "subject-type": "project",
            "subject-name": project,
        }));
    }

    for subject in &profile.subjects {
        hints.push(serde_json::json!({
            "subject-type": "other",
            "subject-name": subject,
        }));
    }

    for hint in &profile.hints {
        hints.push(serde_json::json!({
            "hint": hint,
        }));
    }

    let title = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(80).collect::<String>());

    MemoryIngestRequest {
        model: profile.model.clone(),
        text,
        namespace: profile.namespace.clone(),
        hints,
        source_type: "note",
        title,
    }
}

fn smoke_edge(subject: &str, verb: &str, source_span: &str) -> MemoryEdge {
    MemoryEdge {
        subject_text: subject.to_string(),
        verb_text: verb.to_string(),
        predicate: Vec::new(),
        subject_type: "other".to_string(),
        source_span: source_span.to_string(),
        confidence: 1.0,
        provenance: "provided".to_string(),
    }
}

fn smoke_subject(profile: &Profile, subjects: &Value) -> String {
    profile
        .subjects
        .first()
        .cloned()
        .or_else(|| {
            subjects
                .get("subjects")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("label"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| profile.project.clone())
        .unwrap_or_else(|| "maludb smoke".to_string())
}

fn smoke_subject_from_profile(profile: &Profile) -> String {
    profile
        .subjects
        .first()
        .cloned()
        .or_else(|| profile.project.clone())
        .unwrap_or_else(|| "maludb smoke".to_string())
}

fn ensure_subject(request: &mut MemoryDocumentRequest, subject: &str) {
    if !request.subjects.iter().any(|value| value == subject) {
        request.subjects.push(subject.to_string());
    }
}

fn context_preamble(profile: &Profile) -> String {
    format!(
        "Context:\n- User: {}\n- Time: {}\n- Project: {}\n- Subjects: {}\n- Hints: {}\n",
        profile.user_name.as_deref().unwrap_or("(unset)"),
        now(),
        profile.project.as_deref().unwrap_or("(unset)"),
        display_list(&profile.subjects),
        display_list(&profile.hints)
    )
}

fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .with_context(|| format!("{} does not have a valid file name", path.display()))
}

fn print_profile(name: &str, profile: &Profile) {
    println!("Profile: {name}");
    println!("API URL: {}", profile.api_url);
    println!(
        "User: {}",
        profile.user_name.as_deref().unwrap_or("(unset)")
    );
    println!(
        "Project: {}",
        profile.project.as_deref().unwrap_or("(unset)")
    );
    println!("Namespace: {}", profile.namespace);
    println!(
        "Note model: {}",
        profile.model.as_deref().unwrap_or("(server default)")
    );
    println!("Subjects: {}", display_list(&profile.subjects));
    println!("Hints: {}", display_list(&profile.hints));
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_string()
    } else {
        values.join(", ")
    }
}

fn list_query(
    query: Option<String>,
    limit: Option<u16>,
    with_: Option<String>,
) -> Vec<(&'static str, String)> {
    let mut params = Vec::new();
    if let Some(query) = query {
        params.push(("q", query));
    }
    if let Some(limit) = limit {
        params.push(("limit", limit.to_string()));
    }
    if let Some(with_) = with_ {
        params.push(("with", with_));
    }
    params
}

fn handle_graph(paths: &Paths, command: GraphCommand) -> Result<()> {
    let config = Config::load(paths)?;
    let (_, profile) = config.active_profile()?;
    let token = config.required_token(paths, profile)?;
    let api = ApiClient::new(&profile.api_url, Some(token));

    match command {
        GraphCommand::Query {
            text,
            namespace,
            depth,
            max_nodes,
            json,
        } => {
            let mut params = vec![
                ("q", text),
                ("depth", depth.to_string()),
                ("max_nodes", max_nodes.to_string()),
            ];
            if let Some(ns) = namespace {
                params.push(("namespace", ns));
            }
            let params: Vec<(&str, String)> = params;
            let body = api.get_json_query("/v1/graph/query", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_query(&body);
            Ok(())
        }
        GraphCommand::Neighbors {
            id,
            kind,
            direction,
            rel,
            json,
        } => {
            let mut params = vec![
                ("kind", kind),
                ("id", id.to_string()),
                ("direction", direction),
            ];
            if let Some(rel) = rel {
                params.push(("rel", rel));
            }
            let body = api.get_json_query("/v1/graph/neighbors", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_rows(&body, "neighbors", |row| {
                let kind = string_field(row, "neighbor_kind", "subject");
                let id = row.get("neighbor_id").and_then(Value::as_i64).unwrap_or(0);
                let rel = string_field(row, "rel", "related_to");
                let label = string_field(row, "label", "(unnamed)");
                format!("{kind}:{id} {rel} {label}")
            });
            Ok(())
        }
        GraphCommand::Walk {
            id,
            kind,
            max_depth,
            direction,
            rel,
            json,
        } => {
            let mut params = vec![
                ("kind", kind),
                ("id", id.to_string()),
                ("max_depth", max_depth.to_string()),
                ("direction", direction),
            ];
            if let Some(rel) = rel {
                params.push(("rel", rel));
            }
            let body = api.get_json_query("/v1/graph/walk", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_rows(&body, "walk", |row| {
                let kind = string_field(row, "object_kind", "subject");
                let id = row.get("object_id").and_then(Value::as_i64).unwrap_or(0);
                let depth = row.get("depth").and_then(Value::as_i64).unwrap_or(0);
                let rel = string_field(row, "rel", "related_to");
                let label = string_field(row, "label", "(unnamed)");
                format!("{kind}:{id} d{depth} {rel} {label}")
            });
            Ok(())
        }
        GraphCommand::Path {
            source_id,
            target_id,
            source_kind,
            target_kind,
            max_depth,
            direction,
            rel,
            json,
        } => {
            let mut params = vec![
                ("source_kind", source_kind),
                ("source_id", source_id.to_string()),
                ("target_kind", target_kind),
                ("target_id", target_id.to_string()),
                ("max_depth", max_depth.to_string()),
                ("direction", direction),
            ];
            if let Some(rel) = rel {
                params.push(("rel", rel));
            }
            let body = api.get_json_query("/v1/graph/path", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_rows(&body, "paths", |row| {
                let depth = row.get("depth").and_then(Value::as_i64).unwrap_or(0);
                let hops = row
                    .get("path")
                    .and_then(Value::as_array)
                    .map(|path| {
                        path.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    })
                    .unwrap_or_default();
                format!("depth {depth}: {hops}")
            });
            Ok(())
        }
        GraphCommand::Stats { json } => {
            let body = api.get_json("/v1/graph/stats")?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            let stats = body.get("stats").unwrap_or(&Value::Null);
            let edges = stats.get("edges").and_then(Value::as_i64).unwrap_or(0);
            let nodes = stats.get("nodes").and_then(Value::as_i64).unwrap_or(0);
            println!("nodes {nodes}");
            println!("edges {edges}");
            if let Some(stores) = stats.get("by_store").and_then(Value::as_object) {
                for (store, count) in stores {
                    println!("store {store} {}", count.as_i64().unwrap_or(0));
                }
            }
            if let Some(rels) = stats.get("top_rels").and_then(Value::as_array) {
                for rel in rels {
                    let name = string_field(rel, "rel", "(none)");
                    let count = rel.get("edges").and_then(Value::as_i64).unwrap_or(0);
                    println!("rel {name} {count}");
                }
            }
            Ok(())
        }
        GraphCommand::GodNodes { limit, json } => {
            let params = vec![("limit", limit.to_string())];
            let body = api.get_json_query("/v1/graph/god-nodes", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_rows(&body, "god_nodes", |row| {
                let label = string_field(row, "label", "(unnamed)");
                let total = row.get("degree_total").and_then(Value::as_i64).unwrap_or(0);
                let out = row.get("degree_out").and_then(Value::as_i64).unwrap_or(0);
                let inbound = row.get("degree_in").and_then(Value::as_i64).unwrap_or(0);
                format!("{label} total {total} out {out} in {inbound}")
            });
            Ok(())
        }
        GraphCommand::Surprises {
            namespace,
            limit,
            json,
        } => {
            let params = vec![("namespace", namespace), ("limit", limit.to_string())];
            let body = api.get_json_query("/v1/graph/surprises", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_rows(&body, "surprises", |row| {
                let src = string_field(row, "source_label", "(unnamed)");
                let tgt = string_field(row, "target_label", "(unnamed)");
                let rel = string_field(row, "rel", "related_to");
                let src_comm = row
                    .get("source_community")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let tgt_comm = row
                    .get("target_community")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let pair = row
                    .get("community_pair_edges")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                format!("[{src_comm}->{tgt_comm}] {src} {rel} {tgt} pair_edges {pair}")
            });
            Ok(())
        }
        GraphCommand::Communities { namespace, json } => {
            let mut params: Vec<(&str, String)> = Vec::new();
            if let Some(ns) = namespace {
                params.push(("namespace", ns));
            }
            let body = api.get_json_query("/v1/communities", &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_rows(&body, "communities", |row| {
                let id = row.get("community_id").and_then(Value::as_i64).unwrap_or(0);
                let key = row
                    .get("community_key")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let label = string_field(row, "label", "-");
                let members = row.get("member_count").and_then(Value::as_i64).unwrap_or(0);
                let ns = string_field(row, "namespace", "default");
                format!("{id} key {key} {label} members {members} {ns}")
            });
            Ok(())
        }
        GraphCommand::Members {
            community_id,
            limit,
            json,
        } => {
            let params = vec![("limit", limit.to_string())];
            let body =
                api.get_json_query(&format!("/v1/communities/{community_id}/members"), &params)?;
            if json {
                println!("{}", compact_json(&body));
                return Ok(());
            }
            print_graph_rows(&body, "members", |row| {
                let kind = string_field(row, "object_kind", "subject");
                let id = row.get("object_id").and_then(Value::as_i64).unwrap_or(0);
                let label = string_field(row, "label", "(unnamed)");
                format!("{kind}:{id} {label}")
            });
            Ok(())
        }
        GraphCommand::Import {
            file,
            namespace,
            json,
        } => {
            let raw = fs::read_to_string(&file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let graph: Value = serde_json::from_str(&raw)
                .with_context(|| format!("{} is not valid JSON", file.display()))?;
            if graph.get("nodes").and_then(Value::as_array).is_none() {
                bail!(
                    "{} does not look like a graphify graph.json (no top-level \"nodes\" array)",
                    file.display()
                );
            }
            let body = serde_json::json!({ "namespace": namespace, "graph": graph });
            let response = api.post_value("/v1/graph/import", &body)?;
            if json {
                println!("{}", compact_json(&response));
                return Ok(());
            }
            let nodes = response.get("nodes").unwrap_or(&Value::Null);
            let edges = response.get("edges").unwrap_or(&Value::Null);
            let imported = nodes.get("imported").and_then(Value::as_i64).unwrap_or(0);
            let created = nodes.get("created").and_then(Value::as_i64).unwrap_or(0);
            let resolved = nodes.get("resolved").and_then(Value::as_i64).unwrap_or(0);
            let edges_imported = edges.get("imported").and_then(Value::as_i64).unwrap_or(0);
            let skipped = response
                .get("skipped")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            println!(
                "Imported into '{namespace}': {imported} nodes ({created} new, {resolved} updated), \
                 {edges_imported} edges, {skipped} skipped"
            );
            if let Some(communities) = response.get("communities").filter(|c| !c.is_null()) {
                let stored = communities
                    .get("stored")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let members = communities
                    .get("members")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                println!("Communities stored: {stored} ({members} members)");
            }
            Ok(())
        }
    }
}

fn print_graph_query(body: &Value) {
    if let Some(seeds) = body.get("seeds").and_then(Value::as_array) {
        if seeds.is_empty() {
            println!("No matching nodes found");
            return;
        }
        for seed in seeds {
            let name = string_field(seed, "canonical_name", "(unnamed)");
            let score = seed.get("score").and_then(Value::as_i64).unwrap_or(0);
            println!("seed {name} score {score}");
        }
    }
    if let Some(nodes) = body.get("nodes").and_then(Value::as_array) {
        for node in nodes {
            let kind = string_field(node, "object_kind", "subject");
            let id = node.get("object_id").and_then(Value::as_i64).unwrap_or(0);
            let depth = node.get("depth").and_then(Value::as_i64).unwrap_or(0);
            let label = string_field(node, "label", "(unnamed)");
            println!("node {kind}:{id} d{depth} {label}");
        }
    }
    if let Some(edges) = body.get("edges").and_then(Value::as_array) {
        for edge in edges {
            let src = edge.get("source_id").and_then(Value::as_i64).unwrap_or(0);
            let tgt = edge.get("target_id").and_then(Value::as_i64).unwrap_or(0);
            let rel = string_field(edge, "rel", "related_to");
            println!("edge {src} {rel} {tgt}");
        }
    }
}

fn print_graph_rows(body: &Value, envelope: &str, format_row: impl Fn(&Value) -> String) {
    let Some(rows) = body.get(envelope).and_then(Value::as_array) else {
        println!("No {envelope} returned");
        return;
    };
    if rows.is_empty() {
        println!("No {envelope} returned");
        return;
    }
    for row in rows {
        println!("{}", format_row(row));
    }
}

fn print_subjects(body: &Value) {
    let Some(subjects) = body.get("subjects").and_then(Value::as_array) else {
        println!("No subjects returned");
        return;
    };

    if subjects.is_empty() {
        println!("No subjects returned");
        return;
    }

    for subject in subjects {
        let id = item_id(subject);
        let label = string_field(subject, "label", "(unnamed)");
        let kind = string_field(subject, "type", "unknown");
        println!("{id} {label} subject {kind}");
    }
}

fn print_named_items(body: &Value, envelope: &str, name_field: &str) {
    let Some(items) = body.get(envelope).and_then(Value::as_array) else {
        println!("No {envelope} returned");
        return;
    };

    if items.is_empty() {
        println!("No {envelope} returned");
        return;
    }

    for item in items {
        let id = item_id(item);
        let name = string_field(item, name_field, "(unnamed)");
        println!("{id} {name}");
    }
}

fn print_documents(body: &Value) {
    let Some(documents) = body.get("documents").and_then(Value::as_array) else {
        println!("No documents returned");
        return;
    };

    if documents.is_empty() {
        println!("No documents returned");
        return;
    }

    for document in documents {
        let id = item_id(document);
        let title = string_field(document, "title", "(untitled)");
        let source_type = string_field(document, "source_type", "document");
        println!("{id} {title} {source_type}");
    }
}

fn print_notes(body: &Value) {
    let Some(notes) = body.get("notes").and_then(Value::as_array) else {
        println!("No notes returned");
        return;
    };

    if notes.is_empty() {
        println!("No notes returned");
        return;
    }

    for note in notes {
        let id = item_id(note);
        let title = string_field(note, "title", "(untitled)");
        let source_type = string_field(note, "source_type", "note");
        let match_count = note.get("match_count").and_then(Value::as_i64).unwrap_or(0);
        let edge_word = if match_count == 1 { "edge" } else { "edges" };
        println!("{id} {title} ({source_type}, {match_count} {edge_word})");
        if let Some(edges) = note.get("matched_edges").and_then(Value::as_array) {
            for edge in edges {
                let subject = string_field(edge, "subject_name", "?");
                let verb = string_field(edge, "verb_name", "?");
                let object = string_field(edge, "object_name", "?");
                let via = string_field(edge, "match_via", "?");
                println!("  {subject} --{verb}--> {object} [{via}]");
            }
        }
        if let Some(snippet) = note
            .get("snippet")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|snippet| !snippet.is_empty())
        {
            println!("  {}", snippet.replace('\n', " "));
        }
    }
}

fn normalize_chat_log(source: ChatSource, raw: &str) -> String {
    let mut entries = Vec::new();

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(value) => {
                if let Some(entry) = chat_entry_text(source, &value) {
                    entries.push(entry);
                }
            }
            Err(_) => entries.push(format!("[{} raw]\n{line}", source.as_str())),
        }
    }

    if entries.is_empty() {
        raw.trim().to_string()
    } else {
        entries.join("\n\n")
    }
}

fn chat_entry_text(source: ChatSource, value: &Value) -> Option<String> {
    match source {
        ChatSource::Codex => codex_entry_text(value),
        ChatSource::ClaudeCode => claude_code_entry_text(value),
    }
}

fn codex_entry_text(value: &Value) -> Option<String> {
    let entry_type = value.get("type").and_then(Value::as_str).unwrap_or("entry");
    let payload = value.get("payload").unwrap_or(value);

    if entry_type == "session_meta" {
        let mut fields = Vec::new();
        if let Some(id) = payload.get("id").and_then(Value::as_str) {
            fields.push(format!("id: {id}"));
        }
        if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
            fields.push(format!("cwd: {cwd}"));
        }
        return (!fields.is_empty()).then(|| format!("[codex session]\n{}", fields.join("\n")));
    }

    let role = payload
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("message")
                .and_then(|message| message.get("role"))
                .and_then(Value::as_str)
        })
        .unwrap_or(entry_type);

    let content = payload
        .get("content")
        .or_else(|| payload.get("text"))
        .or_else(|| {
            payload
                .get("message")
                .and_then(|message| message.get("content").or_else(|| message.get("text")))
        })
        .or_else(|| payload.get("message"))?;
    let text = extract_chat_text(content)?;
    Some(format!("[codex {role}]\n{text}"))
}

fn claude_code_entry_text(value: &Value) -> Option<String> {
    let entry_type = value.get("type").and_then(Value::as_str).unwrap_or("entry");

    if entry_type == "attachment" {
        return claude_attachment_text(value);
    }

    let message = value.get("message").unwrap_or(value);
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or(entry_type);
    let content = message
        .get("content")
        .or_else(|| message.get("text"))
        .or_else(|| value.get("content"))
        .or_else(|| value.get("text"))?;
    let text = extract_chat_text(content)?;
    Some(format!("[claude-code {role}]\n{text}"))
}

fn claude_attachment_text(value: &Value) -> Option<String> {
    let mut fields = Vec::new();
    for field in ["command", "content", "stdout", "stderr"] {
        if let Some(text) = value.get(field).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                fields.push(format!("{field}: {text}"));
            }
        }
    }
    (!fields.is_empty()).then(|| format!("[claude-code attachment]\n{}", fields.join("\n")))
}

fn extract_chat_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => non_empty_text(text),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(extract_chat_text).collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        Value::Object(object) => object
            .get("text")
            .and_then(extract_chat_text)
            .or_else(|| object.get("content").and_then(extract_chat_text))
            .or_else(|| {
                let kind = object.get("type").and_then(Value::as_str)?;
                let name = object.get("name").and_then(Value::as_str)?;
                Some(format!("[{kind} {name}]"))
            }),
        _ => None,
    }
}

fn non_empty_text(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn item_id(item: &Value) -> String {
    item.get("id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn string_field(value: &Value, field: &str, default: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

fn array_len(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

const SETTINGS_TITLE: &str = "malu-cli-settings";
const SETTINGS_TYPE: &str = "malu_cli_settings";
const KEYRING_SERVICE: &str = "org.maludb.malu";

fn fetch_settings_note(api: &ApiClient) -> Result<Option<SettingsNote>> {
    let body = api.get_json_query(
        "/v1/notes",
        &[
            ("type", SETTINGS_TYPE.to_string()),
            ("q", SETTINGS_TITLE.to_string()),
        ],
    )?;
    let note = body
        .get("notes")
        .and_then(Value::as_array)
        .and_then(|notes| notes.first());
    let Some(note) = note else {
        return Ok(None);
    };
    let id = note
        .get("id")
        .and_then(Value::as_i64)
        .context("remote settings note is missing id")?;
    let body = note
        .get("body")
        .and_then(Value::as_str)
        .context("remote settings note is missing body")?
        .to_string();
    Ok(Some(SettingsNote { id, body }))
}

fn sync_blob_from_config(paths: &Paths, config: &Config) -> Result<SyncBlob> {
    Ok(SyncBlob {
        schema_version: 1,
        updated_at: now(),
        device_id: device_id(paths)?,
        active_profile: config.active_profile.clone(),
        profiles: config.profiles.clone(),
    })
}

fn preserve_local_token_settings(local: &Config, remote: &mut Config) {
    for (name, remote_profile) in &mut remote.profiles {
        if let Some(local_profile) = local.profiles.get(name) {
            if local_profile.token_key.is_some() {
                remote_profile.token_key = local_profile.token_key.clone();
            }
            if local_profile.token_store.is_some() {
                remote_profile.token_store = local_profile.token_store.clone();
            }
        }
    }
}

fn note_id(response: &Value) -> Option<i64> {
    response
        .get("note")
        .and_then(|note| note.get("id"))
        .and_then(Value::as_i64)
}

fn print_name_list(label: &str, names: &[String]) {
    if names.is_empty() {
        println!("{label}: (none)");
    } else {
        println!("{label}: {}", names.join(", "));
    }
}

fn store_token_for_active_profile(
    paths: &Paths,
    config: &mut Config,
    token: String,
    requested_store: TokenStore,
) -> Result<(String, TokenStore)> {
    let (profile_name, token_key) = {
        let (profile_name, profile) = config.active_profile_mut()?;
        let token_key = profile_name.clone();
        profile.token_key = Some(token_key.clone());
        profile.touch();
        (profile_name, token_key)
    };

    let actual_store = match requested_store {
        TokenStore::File => {
            store_file_token(paths, token_key.clone(), token)?;
            TokenStore::File
        }
        TokenStore::Keyring => match store_keyring_token(&token_key, &token) {
            Ok(()) => TokenStore::Keyring,
            Err(_) => {
                store_file_token(paths, token_key.clone(), token)?;
                TokenStore::File
            }
        },
    };

    let (_, profile) = config.active_profile_mut()?;
    profile.token_store = Some(actual_store.as_str().to_string());
    Ok((profile_name, actual_store))
}

fn store_file_token(paths: &Paths, token_key: String, token: String) -> Result<()> {
    let mut credentials = Credentials::load(paths)?;
    credentials.tokens.insert(token_key, token);
    credentials.save(paths)
}

fn store_keyring_token(token_key: &str, token: &str) -> Result<()> {
    if let Some(env_var) = keyring_disabled_env() {
        bail!("keyring disabled by {env_var}");
    }
    keyring::use_native_store(false).context("failed to initialize native keyring")?;
    let entry = Entry::new(KEYRING_SERVICE, token_key).context("failed to create keyring entry")?;
    entry
        .set_password(token)
        .context("failed to store token in keyring")?;
    keyring::release_store();
    Ok(())
}

fn load_keyring_token(token_key: &str) -> Result<String> {
    if let Some(env_var) = keyring_disabled_env() {
        bail!("keyring disabled by {env_var}");
    }
    keyring::use_native_store(false).context("failed to initialize native keyring")?;
    let entry = Entry::new(KEYRING_SERVICE, token_key).context("failed to create keyring entry")?;
    let token = entry
        .get_password()
        .context("failed to read token from keyring")?;
    keyring::release_store();
    Ok(token)
}

fn keyring_disabled_env() -> Option<&'static str> {
    if std::env::var_os("MALUDB_KEYRING_DISABLED").is_some() {
        Some("MALUDB_KEYRING_DISABLED")
    } else if std::env::var_os("MALU_KEYRING_DISABLED").is_some() {
        Some("MALU_KEYRING_DISABLED")
    } else {
        None
    }
}

impl TokenStore {
    fn as_str(self) -> &'static str {
        match self {
            TokenStore::Keyring => "keyring",
            TokenStore::File => "file",
        }
    }
}

impl ChatSource {
    fn as_str(self) -> &'static str {
        match self {
            ChatSource::Codex => "codex",
            ChatSource::ClaudeCode => "claude-code",
        }
    }
}

#[derive(Clone, Copy)]
enum Collection {
    Subjects,
    Hints,
}

impl Collection {
    fn values_mut(self, profile: &mut Profile) -> &mut Vec<String> {
        match self {
            Collection::Subjects => &mut profile.subjects,
            Collection::Hints => &mut profile.hints,
        }
    }

    fn singular(self) -> &'static str {
        match self {
            Collection::Subjects => "subject",
            Collection::Hints => "hint",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Collection::Subjects => "subjects",
            Collection::Hints => "hints",
        }
    }
}

struct ApiClient {
    base_url: String,
    token: Option<String>,
    client: Client,
}

impl ApiClient {
    fn new(base_url: &str, token: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client: Client::new(),
        }
    }

    fn get_json(&self, path: &str) -> Result<Value> {
        self.get_json_query(path, &[])
    }

    fn get_json_query(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let response = self
            .request(reqwest::Method::GET, path)
            .query(query)
            .send()
            .context("failed to send API request")?;
        decode_response(response)
    }

    fn post_value(&self, path: &str, body: &Value) -> Result<Value> {
        let response = self
            .request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .context("failed to send API request")?;
        decode_response(response)
    }

    fn patch_value(&self, path: &str, body: &Value) -> Result<Value> {
        let response = self
            .request(reqwest::Method::PATCH, path)
            .json(body)
            .send()
            .context("failed to send API request")?;
        decode_response(response)
    }

    fn put_value(&self, path: &str, body: &Value) -> Result<Value> {
        let response = self
            .request(reqwest::Method::PUT, path)
            .json(body)
            .send()
            .context("failed to send API request")?;
        decode_response(response)
    }

    fn delete_value(&self, path: &str) -> Result<Value> {
        let response = self
            .request(reqwest::Method::DELETE, path)
            .send()
            .context("failed to send API request")?;
        decode_response_allow_empty(response)
    }

    fn post_json<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R> {
        let response = self
            .request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .context("failed to send API request")?;
        let value = decode_response(response)?;
        serde_json::from_value(value).context("failed to parse API response")
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let request = self.client.request(method, url);
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

fn decode_response(response: reqwest::blocking::Response) -> Result<Value> {
    decode_response_inner(response, false)
}

/// Like `decode_response`, but an empty success body (e.g. a 204 DELETE)
/// decodes to `Value::Null` instead of a parse error.
fn decode_response_allow_empty(response: reqwest::blocking::Response) -> Result<Value> {
    decode_response_inner(response, true)
}

fn decode_response_inner(
    response: reqwest::blocking::Response,
    allow_empty: bool,
) -> Result<Value> {
    let status = response.status();
    let body = response.text().context("failed to read API response")?;

    if !status.is_success() {
        if let Ok(envelope) = serde_json::from_str::<ErrorEnvelope>(&body) {
            match error_hint(&envelope.error.code) {
                Some(hint) => bail!(
                    "API error {}: {}\n{hint}",
                    envelope.error.code,
                    envelope.error.message
                ),
                None => bail!(
                    "API error {}: {}",
                    envelope.error.code,
                    envelope.error.message
                ),
            }
        }
        bail!("API error HTTP {status}: {body}");
    }

    if allow_empty && body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body).context("failed to parse API response as JSON")
}

/// Actionable next steps for the model-setup errors an end user can fix
/// from the CLI.
fn error_hint(code: &str) -> Option<&'static str> {
    match code {
        "model_not_configured" => Some(
            "Hint: choose an extraction model with `maludb llm use <model>` (see `maludb llm catalog`).",
        ),
        "model_api_key_missing" => Some(
            "Hint: store your provider API key with `maludb llm set-key <provider>` (see `maludb llm providers`).",
        ),
        _ => None,
    }
}

impl Config {
    fn load(paths: &Paths) -> Result<Self> {
        if !paths.config_file.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&paths.config_file)
            .with_context(|| format!("failed to read {}", paths.config_file.display()))?;
        toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", paths.config_file.display()))
    }

    fn save(&self, paths: &Paths) -> Result<()> {
        fs::create_dir_all(&paths.config_dir)
            .with_context(|| format!("failed to create {}", paths.config_dir.display()))?;
        let contents = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(&paths.config_file, contents)
            .with_context(|| format!("failed to write {}", paths.config_file.display()))
    }

    fn active_profile(&self) -> Result<(&str, &Profile)> {
        let name = self
            .active_profile
            .as_deref()
            .context("No active profile. Run `maludb profile create <name>` first.")?;
        let profile = self
            .profiles
            .get(name)
            .with_context(|| format!("Active profile {name} is missing from config"))?;
        Ok((name, profile))
    }

    fn active_profile_mut(&mut self) -> Result<(String, &mut Profile)> {
        let name = self
            .active_profile
            .clone()
            .context("No active profile. Run `maludb profile create <name>` first.")?;
        let profile = self
            .profiles
            .get_mut(&name)
            .with_context(|| format!("Active profile {name} is missing from config"))?;
        Ok((name, profile))
    }

    fn required_token(&self, paths: &Paths, profile: &Profile) -> Result<String> {
        let token_key = profile
            .token_key
            .as_deref()
            .context("No token configured. Run `maludb set-token <token>` first.")?;
        if profile.token_store.as_deref() == Some(TokenStore::Keyring.as_str())
            && let Ok(token) = load_keyring_token(token_key)
        {
            return Ok(token);
        }
        file_token(paths, token_key)
    }
}

fn file_token(paths: &Paths, token_key: &str) -> Result<String> {
    let credentials = Credentials::load(paths)?;
    credentials
        .tokens
        .get(token_key)
        .cloned()
        .with_context(|| format!("No token found for key {token_key}"))
}

impl Credentials {
    fn load(paths: &Paths) -> Result<Self> {
        if !paths.credentials_file.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&paths.credentials_file)
            .with_context(|| format!("failed to read {}", paths.credentials_file.display()))?;
        toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", paths.credentials_file.display()))
    }

    fn save(&self, paths: &Paths) -> Result<()> {
        fs::create_dir_all(&paths.config_dir)
            .with_context(|| format!("failed to create {}", paths.config_dir.display()))?;
        let contents = toml::to_string_pretty(self).context("failed to serialize credentials")?;
        fs::write(&paths.credentials_file, contents)
            .with_context(|| format!("failed to write {}", paths.credentials_file.display()))?;
        restrict_file_permissions(&paths.credentials_file)?;
        Ok(())
    }
}

impl Profile {
    fn new(api_url: String) -> Self {
        Self {
            api_url,
            token_key: None,
            token_store: None,
            user_name: None,
            project: None,
            model: None,
            namespace: "default".to_string(),
            subjects: Vec::new(),
            hints: Vec::new(),
            updated_at: now(),
        }
    }

    fn touch(&mut self) {
        self.updated_at = now();
    }
}

struct Paths {
    config_dir: PathBuf,
    config_file: PathBuf,
    credentials_file: PathBuf,
    device_file: PathBuf,
}

impl Paths {
    fn discover() -> Result<Self> {
        let config_dir = std::env::var_os("MALUDB_CONFIG_DIR")
            .or_else(|| std::env::var_os("MALU_CONFIG_DIR"))
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| {
                ProjectDirs::from("org", "MaluDB", "malu")
                    .context("could not determine platform config directory")
                    .map(|dirs| dirs.config_dir().to_path_buf())
            })?;
        let config_file = config_dir.join("config.toml");
        let credentials_file = config_dir.join("credentials.toml");
        let device_file = config_dir.join("device_id");
        Ok(Self {
            config_dir,
            config_file,
            credentials_file,
            device_file,
        })
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn device_id(paths: &Paths) -> Result<String> {
    if paths.device_file.exists() {
        return fs::read_to_string(&paths.device_file)
            .map(|value| value.trim().to_string())
            .with_context(|| format!("failed to read {}", paths.device_file.display()));
    }

    fs::create_dir_all(&paths.config_dir)
        .with_context(|| format!("failed to create {}", paths.config_dir.display()))?;
    let generated = format!(
        "malu-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    fs::write(&paths.device_file, &generated)
        .with_context(|| format!("failed to write {}", paths.device_file.display()))?;
    restrict_file_permissions(&paths.device_file)?;
    Ok(generated)
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
