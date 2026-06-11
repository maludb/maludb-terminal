//! LLM provider/model configuration — catalog, providers, set-key,
//! remove-key, models, use.
//!
//! The API server seeds a catalog of default model configurations
//! (provider × model × task, with ready-made system prompts). A user stores
//! their own provider API keys server-side (`PUT /v1/llm/providers/{p}`) and
//! picks a model per task (`PUT /v1/llm/models/{t}`); `malu note` then works
//! without any model plumbing in the CLI. Provider keys are read from a
//! hidden prompt (or stdin when piped) and sent straight to the server —
//! they are never written to config.toml, credentials.toml, or the keyring.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

use crate::{ApiClient, Config, Paths, compact_json, string_field};

// ---------------------------------------------------------------------------
// Server endpoint paths — kept in one block so a server-side change is a
// one-place edit.
// ---------------------------------------------------------------------------

const CATALOG_PATH: &str = "/v1/llm/catalog";
const PROVIDERS_PATH: &str = "/v1/llm/providers";
const MODELS_PATH: &str = "/v1/llm/models";

fn provider_path(provider: &str) -> String {
    format!("{PROVIDERS_PATH}/{provider}")
}

fn model_task_path(task: LlmTask) -> String {
    format!("{MODELS_PATH}/{}", task.as_str())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Tasks the server resolves a model for. CLI spelling is kebab-case
/// (`--task skill-extract`); the wire spelling is snake_case.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum LlmTask {
    Extract,
    SkillExtract,
    Embed,
}

impl LlmTask {
    fn as_str(self) -> &'static str {
        match self {
            LlmTask::Extract => "extract",
            LlmTask::SkillExtract => "skill_extract",
            LlmTask::Embed => "embed",
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum LlmCommand {
    /// List the server's model catalog and whether your provider keys are set
    Catalog {
        /// Only show models for one task
        #[arg(long, value_enum)]
        task: Option<LlmTask>,
        #[arg(long)]
        json: bool,
    },
    /// List providers you have stored an API key for
    Providers {
        #[arg(long)]
        json: bool,
    },
    /// Store your API key for a provider (read from a hidden prompt or stdin, never argv)
    SetKey {
        /// Provider name, e.g. openai, anthropic, google, xai, deepseek, ollama
        provider: String,
        /// Override the provider base URL (mainly for ollama / self-hosted gateways)
        #[arg(long)]
        base_url: Option<String>,
    },
    /// Delete your stored API key for a provider
    RemoveKey { provider: String },
    /// Show your current task -> model choices
    Models {
        #[arg(long)]
        json: bool,
    },
    /// Choose the model used for a task (default task: extract)
    Use {
        /// Model name as shown by `malu llm catalog`
        model_name: String,
        #[arg(long, value_enum, default_value_t = LlmTask::Extract)]
        task: LlmTask,
        /// Read a custom system prompt for this task from a file
        #[arg(long)]
        system_prompt_file: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn handle_llm(paths: &Paths, command: LlmCommand) -> Result<()> {
    let config = Config::load(paths)?;
    let (_, profile) = config.active_profile()?;
    let token = config.required_token(paths, profile)?;
    let api = ApiClient::new(&profile.api_url, Some(token));

    match command {
        LlmCommand::Catalog { task, json } => catalog(&api, task, json),
        LlmCommand::Providers { json } => providers(&api, json),
        LlmCommand::SetKey { provider, base_url } => set_key(&api, &provider, base_url),
        LlmCommand::RemoveKey { provider } => remove_key(&api, &provider),
        LlmCommand::Models { json } => models(&api, json),
        LlmCommand::Use {
            model_name,
            task,
            system_prompt_file,
            json,
        } => use_model(&api, &model_name, task, system_prompt_file, json),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn catalog(api: &ApiClient, task: Option<LlmTask>, json: bool) -> Result<()> {
    let body = api.get_json(CATALOG_PATH)?;
    if json {
        println!("{}", compact_json(&body));
        return Ok(());
    }

    let models = body
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut shown = 0usize;
    for model in &models {
        let model_task = string_field(model, "task", "?");
        if let Some(filter) = task
            && model_task != filter.as_str()
        {
            continue;
        }
        let key = if model.get("key_set").and_then(Value::as_bool) == Some(true) {
            "key:set"
        } else {
            "key:missing"
        };
        let chosen = if model.get("is_choice").and_then(Value::as_bool) == Some(true) {
            "  (current choice)"
        } else {
            ""
        };
        println!(
            "{:<14} {:<10} {:<24} {:<10} {}{}",
            model_task,
            string_field(model, "provider", "?"),
            string_field(model, "model_name", "?"),
            string_field(model, "api_format", "?"),
            key,
            chosen,
        );
        shown += 1;
    }
    if shown == 0 {
        println!("No models in catalog");
    }
    Ok(())
}

fn providers(api: &ApiClient, json: bool) -> Result<()> {
    let body = api.get_json(PROVIDERS_PATH)?;
    if json {
        println!("{}", compact_json(&body));
        return Ok(());
    }

    let providers = body
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if providers.is_empty() {
        println!("No provider keys stored. Add one with `malu llm set-key <provider>`.");
        return Ok(());
    }
    for provider in &providers {
        let key = if provider.get("key_set").and_then(Value::as_bool) == Some(true) {
            "key:set"
        } else {
            "key:missing"
        };
        let base_url = provider
            .get("base_url")
            .and_then(Value::as_str)
            .unwrap_or("(default)");
        println!(
            "{:<12} {:<12} base_url:{}",
            string_field(provider, "provider", "?"),
            key,
            base_url,
        );
    }
    Ok(())
}

fn set_key(api: &ApiClient, provider: &str, base_url: Option<String>) -> Result<()> {
    let provider = provider.trim().to_lowercase();
    let key = read_secret(&format!("API key for {provider}: "))?;

    let mut body = json!({ "api_key": key });
    if let Some(url) = base_url {
        body["base_url"] = Value::String(url);
    }
    api.put_value(&provider_path(&provider), &body)?;
    println!("Stored {provider} API key on the server");
    Ok(())
}

fn remove_key(api: &ApiClient, provider: &str) -> Result<()> {
    let provider = provider.trim().to_lowercase();
    api.delete_value(&provider_path(&provider))?;
    println!("Removed {provider} API key from the server");
    Ok(())
}

fn models(api: &ApiClient, json: bool) -> Result<()> {
    let body = api.get_json(MODELS_PATH)?;
    if json {
        println!("{}", compact_json(&body));
        return Ok(());
    }

    let models = body
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if models.is_empty() {
        println!("No task choices configured");
        return Ok(());
    }
    for model in &models {
        let chosen = model.get("chosen").and_then(Value::as_bool) == Some(true);
        let model_name = model
            .get("model_name")
            .and_then(Value::as_str)
            .unwrap_or("(none)");
        let provider = model.get("provider").and_then(Value::as_str);
        let name = match provider {
            Some(provider) => format!("{provider}/{model_name}"),
            None => model_name.to_string(),
        };
        println!(
            "{:<14} {:<32} {}",
            string_field(model, "task", "?"),
            name,
            if chosen {
                "(chosen)"
            } else {
                "(server default)"
            },
        );
    }
    Ok(())
}

fn use_model(
    api: &ApiClient,
    model_name: &str,
    task: LlmTask,
    system_prompt_file: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let mut body = json!({ "model_name": model_name });
    if let Some(path) = system_prompt_file {
        let prompt = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        body["system_prompt"] = Value::String(prompt);
    }
    let response = api.put_value(&model_task_path(task), &body)?;
    if json {
        println!("{}", compact_json(&response));
        return Ok(());
    }
    println!("Using {model_name} for the {} task", task.as_str());
    if let Some(warning) = response
        .get("choice")
        .and_then(|choice| choice.get("warning"))
        .and_then(Value::as_str)
    {
        println!("Warning: {warning}");
    }
    Ok(())
}

/// Read a secret from a hidden prompt when stdin is a terminal, else from a
/// single (piped) stdin line. Keeps keys out of argv and shell history.
fn read_secret(prompt: &str) -> Result<String> {
    use std::io::IsTerminal;

    let secret = if std::io::stdin().is_terminal() {
        rpassword::prompt_password(prompt).context("failed to read key from prompt")?
    } else {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .context("failed to read key from stdin")?;
        line.trim().to_string()
    };
    if secret.is_empty() {
        bail!("no API key provided");
    }
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::LlmTask;

    #[test]
    fn task_wire_names_are_snake_case() {
        assert_eq!(LlmTask::Extract.as_str(), "extract");
        assert_eq!(LlmTask::SkillExtract.as_str(), "skill_extract");
        assert_eq!(LlmTask::Embed.as_str(), "embed");
    }
}
