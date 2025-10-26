use clap::Parser;
use jsonschema::JSONSchema;
use serde_json::Value as JsonValue;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// Simple YAML program validator that checks JSON Schema plus extra domain rules.
#[derive(Parser, Debug)]
#[command(name = "program-verify", author, version, about)]
struct Args {
    /// Path to the YAML program specification.
    input: PathBuf,

    /// Optional custom JSON Schema file instead of the embedded one.
    #[arg(long)]
    schema: Option<PathBuf>,

    /// Print the YAML converted to JSON (debug).
    #[arg(long)]
    show_json: bool,

    /// Specification version key, e.g. "v1" or "v2.1" — used to pick a schema from version_map.yaml.
    /// (Do not confuse with clap's --version flag.)
    #[arg(long = "spec-version", short = 'v', value_name = "NAME")]
    spec_version: Option<String>,

    /// Path to the YAML file that maps specification versions to schema files.
    /// Relative paths within that file are resolved relative to the map file location.
    #[arg(
        long = "versions-map",
        value_name = "FILE",
        default_value = "version_map.yaml"
    )]
    versions_map: PathBuf,
}

fn main() -> ExitCode {
    let args = Args::parse();

    // 1) Read YAML and parse into serde_json::Value
    let yaml_text = match fs::read_to_string(&args.input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read file {}: {e}", args.input.display());
            return ExitCode::from(1);
        }
    };

    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&yaml_text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: invalid YAML: {e}");
            return ExitCode::from(1);
        }
    };

    let instance: JsonValue = match serde_json::to_value(yaml_value) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: YAML→JSON conversion failed: {e}");
            return ExitCode::from(1);
        }
    };

    if args.show_json {
        println!("{}", serde_json::to_string_pretty(&instance).unwrap());
    }

    let combined_spec_version = match extract_spec_version(&instance) {
        Ok(from_doc) => {
            if let Some(from_arg) = &args.spec_version {
                Some(from_arg.clone())
            } else {
                from_doc
            }
        }
        Err(msg) => {
            eprintln!("Error: {msg}");
            return ExitCode::from(1);
        }
    };

    // 2) Load the schema (priority: --schema > spec_version → version_map.yaml > embedded)
    let schema_json: JsonValue = if let Some(path) = &args.schema {
        match read_schema_file(path) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!("{msg}");
                return ExitCode::from(1);
            }
        }
    } else if let Some(ver) = combined_spec_version {
        let versions_map_path = match resolve_versions_map_path(&args.versions_map, &args.input) {
            Ok(p) => p,
            Err(msg) => {
                eprintln!("{msg}");
                return ExitCode::from(1);
            }
        };
        match load_schema_from_version_map(&versions_map_path, &ver) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!("{msg}");
                return ExitCode::from(1);
            }
        }
    } else {
        // Embedded fallback
        match serde_json::from_str(EMBEDDED_SCHEMA) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Embedded schema is invalid: {e}");
                return ExitCode::from(1);
            }
        }
    };

    // 3) JSON Schema validation
    // Note: we do not force a specific draft — the library infers it via `$schema`.
    let compiled = match JSONSchema::compile(&schema_json) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: schema document is invalid: {e}");
            return ExitCode::from(1);
        }
    };

    let mut had_errors = false;
    if let Err(errors) = compiled.validate(&instance) {
        eprintln!("❌ JSON Schema validation failed:");
        for err in errors {
            had_errors = true;
            let instance_path = err.instance_path.to_string();
            let schema_path = err.schema_path.to_string();
            eprintln!(
                "  • {} (instance: {}, schema: {})",
                err, instance_path, schema_path
            );
        }
    }

    // 4) Additional domain-specific rules (beyond JSON Schema)
    if let Err(msg) = check_title_vs_algorithm(&instance) {
        had_errors = true;
        eprintln!("❌ Rule: meta.title vs algorithm.name: {msg}");
    }

    if had_errors {
        ExitCode::from(1)
    } else {
        println!("✅ OK — the document matches the specification.");
        ExitCode::from(0)
    }
}

/// Checks consistency: algorithm.name == base(meta.title)
fn check_title_vs_algorithm(doc: &JsonValue) -> Result<(), String> {
    let meta_title = doc
        .get("meta")
        .and_then(|m| m.get("title"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| "Missing meta.title".to_string())?;

    let algorithm_name = doc
        .get("algorithm")
        .and_then(|a| a.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| "Missing algorithm.name".to_string())?;

    let base = base_name_from_title(meta_title);
    if base != algorithm_name {
        return Err(format!(
            "algorithm.name='{}' does not match the base of meta.title='{}' (detected '{}')",
            algorithm_name, meta_title, base
        ));
    }
    Ok(())
}

/// Extracts the base name from the title: everything before the first '('.
fn base_name_from_title(title: &str) -> String {
    if let Some((left, _)) = title.split_once('(') {
        left.trim().to_string()
    } else {
        title.trim().to_string()
    }
}

/// Reads a JSON schema from disk. Tries JSON first; if that fails, attempts YAML and converts it to JSON.
fn read_schema_file(path: &Path) -> Result<JsonValue, String> {
    let s = fs::read_to_string(path)
        .map_err(|e| format!("Error: failed to read schema {}: {e}", path.display()))?;

    // Try JSON first…
    if let Ok(v) = serde_json::from_str::<JsonValue>(&s) {
        return Ok(v);
    }
    // …and fall back to YAML -> JSON
    let y: serde_yaml::Value = serde_yaml::from_str(&s).map_err(|e| {
        format!(
            "Error: schema file {} is neither valid JSON nor YAML: {e}",
            path.display()
        )
    })?;
    serde_json::to_value(y).map_err(|e| {
        format!(
            "Error: converting schema {} from YAML to JSON failed: {e}",
            path.display()
        )
    })
}

/// Loads `version_map.yaml` and returns the schema corresponding to the provided version.
/// Relative paths in the map are resolved relative to the directory containing the map file.
fn load_schema_from_version_map(map_path: &Path, version: &str) -> Result<JsonValue, String> {
    let map_text = fs::read_to_string(map_path).map_err(|e| {
        format!(
            "Error: failed to read version map {}: {e}",
            map_path.display()
        )
    })?;

    let map: HashMap<String, String> = serde_yaml::from_str(&map_text).map_err(|e| {
        format!(
            "Error: {} is not valid YAML mapping 'version: path': {e}",
            map_path.display()
        )
    })?;

    let Some(target) = map.get(version) else {
        let mut keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        return Err(format!(
            "Error: version '{}' was not found in {}.\nAvailable versions: {}",
            version,
            map_path.display(),
            if keys.is_empty() {
                "(no entries)".into()
            } else {
                keys.join(", ")
            }
        ));
    };

    let resolved = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        map_path.parent().unwrap_or(Path::new(".")).join(target)
    };

    read_schema_file(&resolved)
}

/// Attempts to extract spec_version from the document. Returns None when the field is absent.
fn extract_spec_version(doc: &JsonValue) -> Result<Option<String>, String> {
    match doc.get("spec_version") {
        Some(JsonValue::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err("Field 'spec_version' exists but is not a string.".into()),
        None => Ok(None),
    }
}

/// Searches for the `version_map` file in several locations so the program works regardless of the working directory.
fn resolve_versions_map_path(original: &Path, input: &Path) -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1) User-provided path (absolute or relative to the current working directory)
    if original.is_absolute() {
        candidates.push(original.to_path_buf());
    } else {
        if let Ok(cwd) = env::current_dir() {
            candidates.push(cwd.join(original));
        }
        candidates.push(PathBuf::from(original));
    }

    // 2) Directory of the input document
    if let Some(input_dir) = input.parent() {
        candidates.push(input_dir.join(original));
    }

    // 3) Binary directory and its ancestors (target/release -> target -> project root)
    if let Ok(mut exe_path) = env::current_exe() {
        if exe_path.pop() {
            let mut dir_opt = Some(exe_path);
            while let Some(dir) = dir_opt {
                candidates.push(dir.join(original));
                dir_opt = dir.parent().map(Path::to_path_buf);
            }
        }
    }

    // Remove duplicates while keeping order
    let mut unique = Vec::new();
    for candidate in candidates {
        if !unique.iter().any(|p: &PathBuf| p == &candidate) {
            unique.push(candidate);
        }
    }

    let mut tried = Vec::new();
    for candidate in unique {
        tried.push(candidate.display().to_string());
        if candidate.exists() {
            return candidate.canonicalize().map_err(|e| {
                format!(
                    "Error: failed to canonicalize path {}: {e}",
                    candidate.display()
                )
            });
        }
    }

    Err(format!(
        "Error: could not find the version map '{}' in any location. Checked:\n  - {}",
        original.display(),
        tried.join("\n  - ")
    ))
}

// ▼ Embedded fallback schema lives in src/specyfication.json (used when neither version nor --schema is provided)
const EMBEDDED_SCHEMA: &str = include_str!("specyfication.json");
