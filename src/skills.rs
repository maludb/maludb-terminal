//! Agent-skill commands — push, push-all, list, pull.
//!
//! A Claude Agent Skill is a directory bundle: `SKILL.md` (YAML frontmatter +
//! markdown instructions) plus optional `scripts/`, `references/`, `assets/`.
//! `malu skill push` walks the directory, parses the frontmatter locally
//! (no LLM in the CLI — extraction is server-side, like documents), and
//! uploads the complete bundle to `POST /v1/skills/ingest`. The server
//! recomputes the bundle hash, so re-pushing an unchanged skill is a no-op,
//! and a changed skill becomes a NEW immutable version with fork lineage.
//! `malu skill pull` reconstructs a bundle from the database into a local
//! directory, restoring relative paths and executable bits.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{ApiClient, Config, Paths, compact_json};

/// Server-side caps mirrored client-side so a too-big bundle fails fast.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 30 * 1024 * 1024;

#[derive(Debug, Subcommand)]
pub(crate) enum SkillCommand {
    /// Upload one skill directory (or its SKILL.md path) as a new skill version
    Push {
        path: PathBuf,
        /// Server-side extraction model (omit for the deterministic fallback)
        #[arg(long)]
        model: Option<String>,
        /// Mark this revision as NOT materially different: supersede its parent
        #[arg(long)]
        supersede: bool,
        /// Show what would be sent/judged without writing anything
        #[arg(long)]
        preview: bool,
        #[arg(long)]
        json: bool,
    },
    /// Scan skill roots (~/.claude/skills, ./.claude/skills) and push every skill found
    PushAll {
        /// Extra root directories to scan (repeatable)
        #[arg(long)]
        root: Vec<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List or search skills (subject/verb hit the discovery tags)
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long)]
        verb: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: u16,
        #[arg(long)]
        json: bool,
    },
    /// Reconstruct a skill bundle into a local directory
    Pull {
        /// Skill id, or a skill name (resolves to its newest enabled version)
        skill: String,
        /// Destination directory (default: ./<skill-name>)
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Overwrite an existing destination directory
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn handle_skill(paths: &Paths, command: SkillCommand) -> Result<()> {
    let config = Config::load(paths)?;
    let (_, profile) = config.active_profile()?;
    let token = config.required_token(paths, profile)?;
    let api = ApiClient::new(&profile.api_url, Some(token));

    match command {
        SkillCommand::Push {
            path,
            model,
            supersede,
            preview,
            json,
        } => push_one(&api, &path, model.as_deref(), supersede, preview, json),
        SkillCommand::PushAll { root, model, json } => {
            push_all(&api, &root, model.as_deref(), json)
        }
        SkillCommand::List {
            query,
            subject,
            verb,
            limit,
            json,
        } => list(&api, query, subject, verb, limit, json),
        SkillCommand::Pull {
            skill,
            dest,
            force,
            json,
        } => pull(&api, &skill, dest, force, json),
    }
}

// ---------------------------------------------------------------------------
// Bundle collection
// ---------------------------------------------------------------------------

struct BundleFile {
    relative_path: String,
    content: Vec<u8>,
    is_executable: bool,
    media_type: Option<String>,
}

/// Accept either the skill directory or a path to its SKILL.md.
fn skill_dir(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", path.display()))?;
    if path.is_file() {
        if path.file_name().and_then(|n| n.to_str()) != Some("SKILL.md") {
            bail!(
                "{} is a file but not a SKILL.md; pass the skill directory",
                path.display()
            );
        }
        return Ok(path
            .parent()
            .ok_or_else(|| anyhow!("SKILL.md has no parent directory"))?
            .to_path_buf());
    }
    if !path.join("SKILL.md").is_file() {
        bail!("{} does not contain a SKILL.md", path.display());
    }
    Ok(path)
}

fn is_executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn collect_bundle(dir: &Path) -> Result<Vec<BundleFile>> {
    let mut files = Vec::new();
    let mut total: u64 = 0;
    collect_into(dir, dir, &mut files, &mut total)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    if files.is_empty() {
        bail!("{} contains no files", dir.display());
    }
    Ok(files)
}

fn collect_into(
    root: &Path,
    dir: &Path,
    files: &mut Vec<BundleFile>,
    total: &mut u64,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == ".DS_Store" {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to stat {}", path.display()))?;
        if metadata.is_dir() {
            collect_into(root, &path, files, total)?;
            continue;
        }
        if metadata.len() > MAX_FILE_BYTES {
            bail!(
                "{} is {} bytes; the per-file limit is {MAX_FILE_BYTES}",
                path.display(),
                metadata.len()
            );
        }
        *total += metadata.len();
        if *total > MAX_BUNDLE_BYTES {
            bail!("bundle exceeds the {MAX_BUNDLE_BYTES}-byte limit");
        }
        let relative = path
            .strip_prefix(root)
            .expect("entry is under root")
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let content =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let media_type = mime_guess::from_path(&path)
            .first()
            .map(|m| m.essence_str().to_string());
        files.push(BundleFile {
            relative_path: relative,
            content,
            is_executable: is_executable(&metadata),
            media_type,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Frontmatter + hashing
// ---------------------------------------------------------------------------

/// Parse the leading `---` YAML block of SKILL.md into JSON. A missing or
/// malformed block degrades to an empty object — the server extracts what it
/// can; the upload itself preserves the file verbatim either way.
fn parse_frontmatter(markdown: &str) -> Value {
    let rest = match markdown.strip_prefix("---") {
        Some(rest) => rest,
        None => return json!({}),
    };
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'));
    let rest = match rest {
        Some(r) => r,
        None => return json!({}),
    };
    for terminator in ["\n---\n", "\n---\r\n", "\r\n---\r\n", "\r\n---\n"] {
        if let Some(end) = rest.find(terminator) {
            return serde_yaml::from_str::<Value>(&rest[..end])
                .ok()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
        }
    }
    if let Some(stripped) = rest
        .strip_suffix("\n---")
        .or_else(|| rest.strip_suffix("\r\n---"))
    {
        return serde_yaml::from_str::<Value>(stripped)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
    }
    json!({})
}

fn sha256_hex(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Canonical bundle hash: sha256 over the sorted "<file sha256>  <path>\n"
/// lines. Must match the server's computation (which is authoritative).
fn bundle_hash(files: &[BundleFile]) -> String {
    let mut lines: Vec<String> = files
        .iter()
        .map(|f| format!("{}  {}\n", sha256_hex(&f.content), f.relative_path))
        .collect();
    lines.sort();
    sha256_hex(lines.concat().as_bytes())
}

// ---------------------------------------------------------------------------
// push
// ---------------------------------------------------------------------------

fn push_one(
    api: &ApiClient,
    path: &Path,
    model: Option<&str>,
    supersede: bool,
    preview: bool,
    json_output: bool,
) -> Result<()> {
    let dir = skill_dir(path)?;
    let files = collect_bundle(&dir)?;
    let markdown = files
        .iter()
        .find(|f| f.relative_path == "SKILL.md")
        .map(|f| String::from_utf8_lossy(&f.content).to_string())
        .expect("skill_dir guarantees SKILL.md");
    let frontmatter = parse_frontmatter(&markdown);
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // The directory name is the skill's identity in the open standard; the
    // frontmatter name is a display label that must match it anyway.
    let name = if dir_name.is_empty() {
        frontmatter
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        dir_name
    };
    if name.is_empty() {
        bail!("could not determine the skill name from the directory or frontmatter");
    }

    let local_hash = bundle_hash(&files);
    let files_json: Vec<Value> = files
        .iter()
        .map(|f| {
            json!({
                "relative_path": f.relative_path,
                "content_base64": BASE64.encode(&f.content),
                "is_executable": f.is_executable,
                "media_type": f.media_type,
            })
        })
        .collect();

    let mut body = Map::new();
    body.insert("name".into(), Value::String(name.clone()));
    body.insert("markdown".into(), Value::String(markdown));
    body.insert("frontmatter".into(), frontmatter);
    body.insert("files".into(), Value::Array(files_json));
    if let Some(model) = model {
        body.insert("model".into(), Value::String(model.to_string()));
    }
    if supersede {
        body.insert("materially_different".into(), Value::Bool(false));
    }
    if preview {
        body.insert("preview".into(), Value::Bool(true));
    }

    let response = api.post_value("/v1/skills/ingest", &Value::Object(body))?;

    if json_output || preview {
        println!("{}", compact_json(&response));
        return Ok(());
    }

    let reused = response
        .get("reused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let skill_id = response
        .get("skill_id")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let version = response
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("?");
    if reused {
        println!(
            "Skill {name} unchanged (bundle {}); already registered as skill {skill_id} version {version}",
            &local_hash[..12.min(local_hash.len())]
        );
        return Ok(());
    }
    let superseded = response
        .pointer("/register/superseded_skill_id")
        .and_then(Value::as_i64);
    let files_linked = response
        .pointer("/register/files_linked")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    println!("Pushed skill {name} as skill {skill_id} version {version} ({files_linked} files)");
    if let Some(parent) = response.pointer("/parent/skill_id").and_then(Value::as_i64) {
        match superseded {
            Some(s) => println!("  supersedes skill {s} (not materially different)"),
            None => println!("  forked from skill {parent}; both versions stay visible"),
        }
    }
    Ok(())
}

fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        roots.push(base.home_dir().join(".claude").join("skills"));
    }
    roots.push(PathBuf::from(".claude/skills"));
    roots
}

fn push_all(
    api: &ApiClient,
    extra_roots: &[PathBuf],
    model: Option<&str>,
    json: bool,
) -> Result<()> {
    let mut roots = default_roots();
    roots.extend(extra_roots.iter().cloned());

    let mut seen = BTreeSet::new();
    let mut skill_dirs = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() && path.join("SKILL.md").is_file() {
                let canonical = path.canonicalize().unwrap_or(path);
                if seen.insert(canonical.clone()) {
                    skill_dirs.push(canonical);
                }
            }
        }
    }

    if skill_dirs.is_empty() {
        println!("No skills found (looked in ~/.claude/skills, ./.claude/skills, and any --root)");
        return Ok(());
    }

    let mut pushed = 0usize;
    let mut failed = 0usize;
    for dir in &skill_dirs {
        match push_one(api, dir, model, false, false, json) {
            Ok(()) => pushed += 1,
            Err(err) => {
                failed += 1;
                eprintln!("Failed to push {}: {err:#}", dir.display());
            }
        }
    }
    println!(
        "Pushed {pushed}/{} skills{}",
        skill_dirs.len(),
        if failed > 0 {
            format!(" ({failed} failed)")
        } else {
            String::new()
        }
    );
    if failed > 0 {
        bail!("{failed} skill(s) failed to push");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// list / pull
// ---------------------------------------------------------------------------

fn list(
    api: &ApiClient,
    query: Option<String>,
    subject: Option<String>,
    verb: Option<String>,
    limit: u16,
    json: bool,
) -> Result<()> {
    let mut params: Vec<(&str, String)> = vec![("limit", limit.to_string())];
    if let Some(q) = query {
        params.push(("q", q));
    }
    if let Some(s) = subject {
        params.push(("subject", s));
    }
    if let Some(v) = verb {
        params.push(("verb", v));
    }
    let body = api.get_json_query("/v1/skills", &params)?;
    if json {
        println!("{}", compact_json(&body));
        return Ok(());
    }
    let skills = body
        .get("skills")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if skills.is_empty() {
        println!("No skills found");
        return Ok(());
    }
    for s in skills {
        let id = s.get("id").and_then(Value::as_i64).unwrap_or_default();
        let name = s.get("name").and_then(Value::as_str).unwrap_or("?");
        let version = s.get("version").and_then(Value::as_str).unwrap_or("?");
        let mut line = format!("{id}  {name}  {version}");
        if let Some(score) = s.get("score").and_then(Value::as_f64) {
            let reasons = s
                .get("match_reasons")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            line.push_str(&format!("  score={score} [{reasons}]"));
        } else if s.get("enabled").and_then(Value::as_bool) == Some(false) {
            line.push_str("  (superseded)");
        }
        if let Some(desc) = s.get("description").and_then(Value::as_str) {
            let short: String = desc.chars().take(80).collect();
            line.push_str(&format!("  — {short}"));
        }
        println!("{line}");
    }
    Ok(())
}

fn resolve_skill_id(api: &ApiClient, skill: &str) -> Result<i64> {
    if let Ok(id) = skill.parse::<i64>() {
        return Ok(id);
    }
    let body = api.get_json_query(
        "/v1/skills",
        &[("q", skill.to_string()), ("limit", "200".into())],
    )?;
    let skills = body
        .get("skills")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // Newest enabled version with this exact name.
    let best = skills
        .iter()
        .filter(|s| s.get("name").and_then(Value::as_str) == Some(skill))
        .filter(|s| s.get("enabled").and_then(Value::as_bool) != Some(false))
        .filter_map(|s| s.get("id").and_then(Value::as_i64))
        .max();
    best.ok_or_else(|| anyhow!("no enabled skill named {skill:?} found"))
}

fn pull(
    api: &ApiClient,
    skill: &str,
    dest: Option<PathBuf>,
    force: bool,
    json: bool,
) -> Result<()> {
    let skill_id = resolve_skill_id(api, skill)?;
    let body = api.get_json(&format!("/v1/skills/{skill_id}/bundle"))?;
    let name = body
        .pointer("/skill/name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("bundle response is missing the skill name"))?
        .to_string();
    let version = body
        .pointer("/skill/version")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    let files = body
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if files.is_empty() {
        bail!("skill {skill_id} has no files to pull");
    }

    let dest = dest.unwrap_or_else(|| PathBuf::from(&name));
    if dest.exists()
        && fs::read_dir(&dest)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
        && !force
    {
        bail!(
            "{} already exists and is not empty (use --force to overwrite)",
            dest.display()
        );
    }

    let mut written = 0usize;
    for f in &files {
        let rel = f
            .get("relative_path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("bundle file is missing relative_path"))?;
        if rel.starts_with('/') || rel.split('/').any(|c| c == "..") {
            bail!("refusing to write unsafe path {rel:?}");
        }
        let encoded = f
            .get("content_base64")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("bundle file {rel} is missing content"))?;
        let content = BASE64
            .decode(encoded)
            .with_context(|| format!("invalid base64 content for {rel}"))?;
        if let Some(expected) = f.get("file_hash").and_then(Value::as_str) {
            let actual = sha256_hex(&content);
            if actual != expected {
                bail!("hash mismatch for {rel}: expected {expected}, got {actual}");
            }
        }
        let target = dest.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&target, &content)
            .with_context(|| format!("failed to write {}", target.display()))?;
        #[cfg(unix)]
        if f.get("is_executable").and_then(Value::as_bool) == Some(true) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                .with_context(|| format!("failed to set permissions on {}", target.display()))?;
        }
        written += 1;
    }

    if json {
        println!(
            "{}",
            compact_json(&json!({
                "skill_id": skill_id,
                "name": name,
                "version": version,
                "dest": dest.display().to_string(),
                "files": written,
            }))
        );
    } else {
        println!(
            "Pulled skill {name} version {version} ({written} files) into {}",
            dest.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_parses_standard_block() {
        let md = "---\nname: pdf-processing\ndescription: Extract text. Use when PDFs.\nmetadata:\n  version: \"1.2\"\n---\n\n# Body\n";
        let fm = parse_frontmatter(md);
        assert_eq!(fm["name"], "pdf-processing");
        assert_eq!(fm["metadata"]["version"], "1.2");
    }

    #[test]
    fn frontmatter_missing_returns_empty_object() {
        assert_eq!(parse_frontmatter("# Just markdown\n"), json!({}));
    }

    #[test]
    fn frontmatter_malformed_yaml_degrades_to_empty() {
        let md = "---\n: [unbalanced\n---\nbody";
        assert_eq!(parse_frontmatter(md), json!({}));
    }

    #[test]
    fn bundle_hash_is_order_independent_and_content_sensitive() {
        let f = |path: &str, content: &[u8]| BundleFile {
            relative_path: path.to_string(),
            content: content.to_vec(),
            is_executable: false,
            media_type: None,
        };
        let a = vec![f("SKILL.md", b"x"), f("scripts/run.py", b"y")];
        let b = vec![f("scripts/run.py", b"y"), f("SKILL.md", b"x")];
        assert_eq!(bundle_hash(&a), bundle_hash(&b));
        let c = vec![f("SKILL.md", b"x"), f("scripts/run.py", b"CHANGED")];
        assert_ne!(bundle_hash(&a), bundle_hash(&c));
    }

    #[test]
    fn bundle_hash_matches_server_canonical_form() {
        // Mirror of the server-side test: sha256("<file sha256>  SKILL.md\n").
        let content = b"hello";
        let file = BundleFile {
            relative_path: "SKILL.md".to_string(),
            content: content.to_vec(),
            is_executable: false,
            media_type: None,
        };
        let line = format!("{}  SKILL.md\n", sha256_hex(content));
        assert_eq!(bundle_hash(&[file]), sha256_hex(line.as_bytes()));
    }
}
