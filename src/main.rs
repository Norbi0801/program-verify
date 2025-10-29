use clap::Parser;
use jsonschema::JSONSchema;
use serde_json::Value as JsonValue;
use std::{
    collections::{HashMap, HashSet},
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

    for msg in check_phase_contracts(&instance) {
        had_errors = true;
        eprintln!("❌ Rule: phase contracts: {msg}");
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

fn check_phase_contracts(doc: &JsonValue) -> Vec<String> {
    let mut errors = Vec::new();

    let needs_contracts = doc
        .get("spec_version")
        .and_then(|v| v.as_str())
        .and_then(parse_semver_major)
        .map(|major| major >= 3)
        .unwrap_or(false);

    let algorithm = match doc.get("algorithm") {
        Some(value) => value,
        None => return errors,
    };

    let mut phase_set: HashSet<String> = HashSet::new();
    if let Some(items) = algorithm.get("phases").and_then(|v| v.as_array()) {
        for item in items {
            if let Some(name) = item.as_str() {
                phase_set.insert(name.to_string());
            }
        }
    }

    if let Some(graph) = algorithm.get("graph").and_then(|g| g.as_object()) {
        if let Some(nodes) = graph.get("nodes").and_then(|n| n.as_object()) {
            for (node_id, node_value) in nodes {
                if let Some(node_obj) = node_value.as_object() {
                    if node_obj
                        .get("type")
                        .and_then(|t| t.as_str())
                        .map(|t| t == "phase")
                        .unwrap_or(false)
                    {
                        if let Some(phase_name) = node_obj.get("phase").and_then(|p| p.as_str()) {
                            phase_set.insert(phase_name.to_string());
                        } else {
                            phase_set.insert(node_id.clone());
                        }
                    }
                }
            }
        }
    }

    if phase_set.is_empty() {
        return errors;
    }

    let phases: Vec<String> = phase_set.iter().cloned().collect();

    let implementation = match doc.get("implementation") {
        Some(value) => value,
        None => return errors,
    };

    let contracts_value = match implementation.get("phase_contracts") {
        Some(value) => value,
        None => {
            if needs_contracts {
                errors.push(
                    "implementation.phase_contracts must be present for v3+ specs".to_string(),
                );
            }
            return errors;
        }
    };

    let phase_contracts = match contracts_value.as_object() {
        Some(map) => map,
        None => return errors,
    };

    if needs_contracts {
        for phase in &phases {
            if !phase_contracts.contains_key(phase.as_str()) {
                errors.push(format!(
                    "Missing phase_contracts entry for algorithm phase '{phase}'",
                ));
            }
        }
    }

    for phase_name in phase_contracts.keys() {
        if !phase_set.contains(phase_name.as_str()) {
            errors.push(format!(
                "phase_contracts contains unknown phase '{phase_name}' (not listed in algorithm.phases)"
            ));
        }
    }

    let mut outputs_map: HashMap<String, HashSet<String>> = HashMap::new();
    let mut phase_error_codes: HashMap<String, HashSet<String>> = HashMap::new();

    for (phase_name, contract_value) in phase_contracts.iter() {
        if let Some(contract_obj) = contract_value.as_object() {
            let mut seen_outputs = HashSet::new();
            if let Some(outputs) = contract_obj.get("outputs").and_then(|v| v.as_array()) {
                for output in outputs {
                    if let Some(name) = output.get("name").and_then(|n| n.as_str()) {
                        if !seen_outputs.insert(name.to_string()) {
                            errors.push(format!(
                                "Phase '{phase_name}' defines duplicate output '{name}'",
                            ));
                        }
                    }
                }
            }
            if let Some(errors_array) = contract_obj.get("errors").and_then(|v| v.as_array()) {
                let mut seen_codes = HashSet::new();
                for error_value in errors_array {
                    if let Some(code) = error_value.get("code").and_then(|c| c.as_str()) {
                        if !seen_codes.insert(code.to_string()) {
                            errors.push(format!(
                                "Phase '{phase_name}' declares duplicate error code '{code}'",
                            ));
                        }
                    }
                }
                if !seen_codes.is_empty() {
                    phase_error_codes.insert(phase_name.clone(), seen_codes);
                }
            }
            outputs_map.insert(phase_name.clone(), seen_outputs);
        }
    }

    for (phase_name, contract_value) in phase_contracts.iter() {
        let Some(contract_obj) = contract_value.as_object() else {
            continue;
        };

        let inputs = match contract_obj.get("inputs").and_then(|v| v.as_array()) {
            Some(items) => items,
            None => continue,
        };

        let mut seen_inputs = HashSet::new();
        for input in inputs {
            let Some(input_name) = input.get("name").and_then(|n| n.as_str()) else {
                continue;
            };

            if !seen_inputs.insert(input_name.to_string()) {
                errors.push(format!(
                    "Phase '{phase_name}' declares duplicate input '{input_name}'",
                ));
            }

            if let Some(source_value) = input.get("source") {
                validate_io_source(
                    source_value,
                    Some((phase_name.as_str(), input_name)),
                    None,
                    &phase_set,
                    phase_contracts,
                    &outputs_map,
                    |msg| errors.push(msg),
                );
            }
        }

        if let Some(retry_policy) = contract_obj.get("retry_policy").and_then(|v| v.as_object()) {
            if let Some(retryable_errors) = retry_policy
                .get("retryable_errors")
                .and_then(|v| v.as_array())
            {
                let declared_codes = phase_error_codes.get(phase_name);
                for code_value in retryable_errors {
                    if let Some(code) = code_value.as_str() {
                        if let Some(codes) = declared_codes {
                            if !codes.contains(code) {
                                errors.push(format!(
                                    "Phase '{phase_name}' retry_policy references unknown error code '{code}'",
                                ));
                            }
                        } else {
                            errors.push(format!(
                                "Phase '{phase_name}' retry_policy declares retryable error '{code}' but no errors block is defined",
                            ));
                        }
                    }
                }
            }
        }

        if let Some(fallback) = contract_obj.get("fallback").and_then(|v| v.as_object()) {
            if let Some(fallback_phase) = fallback.get("phase").and_then(|p| p.as_str()) {
                if !phase_set.contains(fallback_phase) {
                    errors.push(format!(
                        "Phase '{phase_name}' fallback references unknown phase '{fallback_phase}'",
                    ));
                } else if !phase_contracts.contains_key(fallback_phase) {
                    errors.push(format!(
                        "Phase '{phase_name}' fallback references phase '{fallback_phase}' but it has no phase_contracts entry",
                    ));
                }
            }
        }
    }

    if let Some(outputs) = algorithm.get("outputs").and_then(|v| v.as_array()) {
        for output in outputs {
            if let Some(build) = output.get("build") {
                let mut sources = Vec::new();
                collect_io_sources(build, &mut sources);
                let output_name = output
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("<composition>");
                for source in sources {
                    validate_io_source(
                        source,
                        None,
                        Some(output_name),
                        &phase_set,
                        phase_contracts,
                        &outputs_map,
                        |msg| errors.push(msg),
                    );
                }
            }
        }
    }

    if let Some(return_contract) = implementation
        .get("return_contract")
        .and_then(|v| v.as_object())
    {
        if let Some(produced_by) = return_contract
            .get("produced_by")
            .and_then(|v| v.as_object())
        {
            let phase = produced_by
                .get("phase")
                .and_then(|p| p.as_str())
                .unwrap_or_default();

            if !phase.is_empty() {
                if !phase_set.contains(phase) {
                    errors.push(format!(
                        "return_contract.produced_by references unknown phase '{phase}'",
                    ));
                } else if !phase_contracts.contains_key(phase) {
                    errors.push(format!(
                        "return_contract.produced_by references phase '{phase}' but it has no phase_contracts entry",
                    ));
                } else if let Some(port) = produced_by.get("port").and_then(|p| p.as_str()) {
                    match outputs_map.get(phase) {
                        Some(outputs) if outputs.contains(port) => {}
                        _ => errors.push(format!(
                            "return_contract.produced_by references output '{port}' from phase '{phase}' which is not declared",
                        )),
                    }
                }
            }
        }
    }

    errors
}

fn validate_io_source<F>(
    source: &JsonValue,
    phase_context: Option<(&str, &str)>,
    composition_name: Option<&str>,
    phase_set: &HashSet<String>,
    phase_contracts: &serde_json::Map<String, JsonValue>,
    outputs_map: &HashMap<String, HashSet<String>>,
    mut push_error: F,
) where
    F: FnMut(String),
{
    let Some(source_obj) = source.as_object() else {
        return;
    };

    let Some(kind) = source_obj.get("kind").and_then(|k| k.as_str()) else {
        return;
    };

    let composition_label = composition_name.unwrap_or("<composition>");

    match kind {
        "phase_output" => {
            let Some(target_phase) = source_obj.get("phase").and_then(|p| p.as_str()) else {
                return;
            };

            if !phase_set.contains(target_phase) {
                push_error(match phase_context {
                    Some((phase_name, input_name)) => format!(
                        "Phase '{phase_name}' references unknown producing phase '{target_phase}' in input '{input_name}'",
                    ),
                    None => format!(
                        "Composition '{composition_label}' references unknown producing phase '{target_phase}'",
                    ),
                });
                return;
            }

            if !phase_contracts.contains_key(target_phase) {
                push_error(match phase_context {
                    Some((phase_name, input_name)) => format!(
                        "Phase '{phase_name}' references phase '{target_phase}' in input '{input_name}' but that phase lacks a phase_contracts entry",
                    ),
                    None => format!(
                        "Composition '{composition_label}' references phase '{target_phase}' but it has no phase_contracts entry",
                    ),
                });
                return;
            }

            let Some(port) = source_obj.get("port").and_then(|p| p.as_str()) else {
                return;
            };

            match outputs_map.get(target_phase) {
                Some(outputs) if outputs.contains(port) => {}
                _ => push_error(match phase_context {
                    Some((phase_name, input_name)) => format!(
                        "Phase '{phase_name}' expects output '{port}' from phase '{target_phase}' in input '{input_name}', but it is not declared",
                    ),
                    None => format!(
                        "Composition '{composition_label}' expects output '{port}' from phase '{target_phase}' but it is not declared",
                    ),
                }),
            }
        }
        "instance" | "global" => {
            match source_obj.get("path").and_then(|p| p.as_str()) {
                Some(path) if !path.trim().is_empty() => {}
                _ => push_error(match phase_context {
                    Some((phase_name, input_name)) => format!(
                        "Phase '{phase_name}' input '{input_name}' must declare a non-empty source.path for kind '{kind}'",
                    ),
                    None => format!(
                        "Composition '{composition_label}' source must declare a non-empty path for kind '{kind}'",
                    ),
                }),
            }
        }
        _ => {}
    }
}

fn collect_io_sources<'a>(value: &'a JsonValue, acc: &mut Vec<&'a JsonValue>) {
    match value {
        JsonValue::Object(map) => {
            if map.contains_key("kind") {
                acc.push(value);
            } else {
                for inner in map.values() {
                    collect_io_sources(inner, acc);
                }
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                collect_io_sources(item, acc);
            }
        }
        _ => {}
    }
}

fn parse_semver_major(ver: &str) -> Option<u64> {
    let trimmed = ver.strip_prefix('v')?;
    let major_part = trimmed.split(|c| c == '.' || c == '-' || c == '+').next()?;
    major_part.parse().ok()
}

/// Extracts the base name from the title: everything before the first opening parenthesis.
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
