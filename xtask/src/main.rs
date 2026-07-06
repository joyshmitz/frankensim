//! FrankenSim repository policy checks (`cargo run -p xtask -- <command>`).
//!
//! Commands:
//! - `check-layers`   — enforce the L0..L6 layer dependency direction (plan §4, AGENTS.md).
//! - `check-deps`     — enforce the Franken-only runtime dependency policy (Decalogue P1).
//! - `check-contracts`— every workspace `fs-*` crate ships a CONTRACT.md with required sections.
//! - `check-all`      — all of the above; non-zero exit on any violation.
//!
//! Output is JSON-lines (one verdict object per check per crate) so agents parse
//! outcomes without scraping; a human-readable summary goes to stderr.
//!
//! Parsing note: this tool intentionally hand-parses the *subset* of TOML that this
//! repository's own generated manifests use (see docs/CONVENTIONS.md "Manifest
//! conventions": one dependency per line, `[section]` headers on their own line).
//! It fails loudly on shapes it does not understand rather than guessing — these are
//! our files, and the conventions are enforced, not inferred.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Layers, in the plan's order. `Util` crates (fs-qty, fs-obs) are usable by every
/// layer; `Tool` crates (xtask) are outside the shipped dependency graph entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Util,
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
    L6,
    Tool,
}

impl Layer {
    fn parse(s: &str) -> Option<Layer> {
        Some(match s {
            "UTIL" => Layer::Util,
            "L0" => Layer::L0,
            "L1" => Layer::L1,
            "L2" => Layer::L2,
            "L3" => Layer::L3,
            "L4" => Layer::L4,
            "L5" => Layer::L5,
            "L6" => Layer::L6,
            "TOOL" => Layer::Tool,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Layer::Util => "UTIL",
            Layer::L0 => "L0",
            Layer::L1 => "L1",
            Layer::L2 => "L2",
            Layer::L3 => "L3",
            Layer::L4 => "L4",
            Layer::L5 => "L5",
            Layer::L6 => "L6",
            Layer::Tool => "TOOL",
        }
    }

    /// May a crate in layer `self` depend on a crate in layer `dep`?
    ///
    /// This is NOT a linear order: ASCENT (L4) and LUMEN (L5) are siblings —
    /// LUMEN may not depend on ASCENT and vice versa (AGENTS.md dependency
    /// direction). HELM (L6) may depend on everything; everything may depend
    /// on UTIL; TOOL may depend on anything but nothing may depend on TOOL.
    fn may_depend_on(self, dep: Layer) -> bool {
        use Layer::{L0, L1, L2, L3, L4, L5, L6, Tool, Util};
        if dep == Tool {
            return false;
        }
        match self {
            Util => matches!(dep, Util),
            L0 => matches!(dep, Util | L0),
            L1 => matches!(dep, Util | L0 | L1),
            L2 => matches!(dep, Util | L0 | L1 | L2),
            L3 => matches!(dep, Util | L0 | L1 | L2 | L3),
            L4 => matches!(dep, Util | L0 | L1 | L2 | L3 | L4),
            // LUMEN: MORPH, FLUX field abstractions, BEDROCK, SUBSTRATE — not ASCENT.
            L5 => matches!(dep, Util | L0 | L1 | L2 | L3 | L5),
            L6 | Tool => true,
        }
    }
}

/// The Franken constellation: the ONLY permitted external runtime dependencies
/// besides `std` (Decalogue P1). Accepts snake_case and hyphenated names.
const CONSTELLATION: &[&str] = &[
    "asupersync",
    "franken_sqlite",
    "franken-sqlite",
    "frankensqlite",
    "franken_numpy",
    "franken-numpy",
    "franken_torch",
    "franken-torch",
    "franken_scipy",
    "franken-scipy",
    "franken_pandas",
    "franken-pandas",
    "franken_networkx",
    "franken-networkx",
];

/// Required CONTRACT.md sections (contract-conformance discipline, plan §13.3).
const CONTRACT_SECTIONS: &[&str] = &[
    "## Purpose and layer",
    "## Public types and semantics",
    "## Invariants",
    "## Error model",
    "## Determinism class",
    "## Cancellation behavior",
    "## Unsafe boundary",
    "## Feature flags",
    "## Conformance tests",
    "## No-claim boundaries",
];

#[derive(Debug)]
struct Manifest {
    name: String,
    layer: Layer,
    /// `[dependencies]` keys only — dev-dependencies are exempt from the layer
    /// rule and the constellation policy (dev-only oracles are permitted when
    /// isolated; they are still REPORTED so the exemption stays visible).
    runtime_deps: Vec<String>,
    dev_deps: Vec<String>,
    dir: PathBuf,
}

#[derive(Debug)]
struct Violation {
    check: &'static str,
    crate_name: String,
    detail: String,
}

fn parse_manifest(path: &Path, text: &str) -> Result<Manifest, String> {
    let mut section = String::new();
    let mut name = None;
    let mut layer = None;
    let mut runtime_deps = Vec::new();
    let mut dev_deps = Vec::new();

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        let Some(eq) = line.find('=') else {
            return Err(format!(
                "{}:{}: unparseable line {line:?}",
                path.display(),
                lineno + 1
            ));
        };
        let key = line[..eq].trim().trim_matches('"').to_string();
        let value = line[eq + 1..].trim();
        match section.as_str() {
            "[package]" if key == "name" => name = Some(value.trim_matches('"').to_string()),
            "[package.metadata.frankensim]" if key == "layer" => {
                let v = value.trim_matches('"');
                layer = Some(
                    Layer::parse(v)
                        .ok_or_else(|| format!("{}: unknown layer {v:?}", path.display()))?,
                );
            }
            "[dependencies]" => runtime_deps.push(key),
            "[dev-dependencies]" => dev_deps.push(key),
            _ => {}
        }
    }

    Ok(Manifest {
        name: name.ok_or_else(|| format!("{}: missing package.name", path.display()))?,
        layer: layer.ok_or_else(|| {
            format!(
                "{}: missing [package.metadata.frankensim] layer — every workspace crate must \
                 declare its layer",
                path.display()
            )
        })?,
        runtime_deps,
        dev_deps,
        dir: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
    })
}

fn load_workspace(root: &Path) -> Result<Vec<Manifest>, String> {
    let crates_dir = root.join("crates");
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&crates_dir)
        .map_err(|e| format!("cannot read {}: {e}", crates_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let manifest_path = entry.path().join("Cargo.toml");
        if manifest_path.is_file() {
            let text = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
            out.push(parse_manifest(&manifest_path, &text)?);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    if out.is_empty() {
        return Err(format!(
            "no crate manifests found under {}",
            crates_dir.display()
        ));
    }
    Ok(out)
}

fn check_layers(manifests: &[Manifest]) -> Vec<Violation> {
    let layer_of: BTreeMap<&str, Layer> = manifests
        .iter()
        .map(|m| (m.name.as_str(), m.layer))
        .collect();
    let mut violations = Vec::new();
    for m in manifests {
        for dep in &m.runtime_deps {
            if let Some(&dep_layer) = layer_of.get(dep.as_str())
                && !m.layer.may_depend_on(dep_layer)
            {
                violations.push(Violation {
                    check: "layers",
                    crate_name: m.name.clone(),
                    detail: format!(
                        "{} ({}) must not depend on {} ({}); lower layers never know \
                         about higher ones, and L4/L5 are siblings (plan §4)",
                        m.name,
                        m.layer.name(),
                        dep,
                        dep_layer.name()
                    ),
                });
            }
        }
    }
    violations
}

fn check_deps(manifests: &[Manifest]) -> Vec<Violation> {
    let workspace: BTreeMap<&str, Layer> = manifests
        .iter()
        .map(|m| (m.name.as_str(), m.layer))
        .collect();
    let mut violations = Vec::new();
    for m in manifests {
        if m.layer == Layer::Tool {
            continue; // xtask itself is outside the shipped graph.
        }
        for dep in &m.runtime_deps {
            let is_workspace = workspace.contains_key(dep.as_str());
            let is_constellation = CONSTELLATION.contains(&dep.as_str());
            if !is_workspace && !is_constellation {
                violations.push(Violation {
                    check: "dependency-policy",
                    crate_name: m.name.clone(),
                    detail: format!(
                        "{} depends on {dep:?}, which is neither a workspace fs-* crate nor a \
                         Franken-constellation library (Decalogue P1: std + constellation only)",
                        m.name
                    ),
                });
            }
        }
    }
    violations
}

fn check_contracts(manifests: &[Manifest]) -> Vec<Violation> {
    let mut violations = Vec::new();
    for m in manifests {
        if m.layer == Layer::Tool {
            continue;
        }
        let contract = m.dir.join("CONTRACT.md");
        match std::fs::read_to_string(&contract) {
            Err(_) => violations.push(Violation {
                check: "contracts",
                crate_name: m.name.clone(),
                detail: format!("{} is missing CONTRACT.md", m.name),
            }),
            Ok(text) => {
                for section in CONTRACT_SECTIONS {
                    if !text.contains(section) {
                        violations.push(Violation {
                            check: "contracts",
                            crate_name: m.name.clone(),
                            detail: format!("{} CONTRACT.md missing section {section:?}", m.name),
                        });
                    }
                }
            }
        }
    }
    violations
}

/// Dev-dependencies are exempt from the constellation policy (dev-only oracles
/// are permitted when isolated) but the exemption must stay VISIBLE: emit a
/// JSON note per external dev-dependency so review sees every use of the escape
/// hatch (docs/CONVENTIONS.md "Dependency policy").
fn dev_dep_notes(manifests: &[Manifest]) -> Vec<(String, String)> {
    let workspace: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
    let mut notes = Vec::new();
    for m in manifests {
        for dep in &m.dev_deps {
            if !workspace.contains(&dep.as_str()) && !CONSTELLATION.contains(&dep.as_str()) {
                notes.push((m.name.clone(), dep.clone()));
            }
        }
    }
    notes
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn emit(
    violations: &[Violation],
    dev_notes: &[(String, String)],
    checks_run: &[&str],
    crates_checked: usize,
) -> ExitCode {
    // JSON-lines verdicts on stdout (agent-facing).
    for (krate, dep) in dev_notes {
        println!(
            "{{\"check\":\"dev-dependency-note\",\"crate\":\"{}\",\"verdict\":\"note\",\"detail\":\"external dev-dependency {} (exempt; must be an isolated, documented oracle)\"}}",
            json_escape(krate),
            json_escape(dep)
        );
    }
    for v in violations {
        println!(
            "{{\"check\":\"{}\",\"crate\":\"{}\",\"verdict\":\"violation\",\"detail\":\"{}\"}}",
            json_escape(v.check),
            json_escape(&v.crate_name),
            json_escape(&v.detail)
        );
    }
    println!(
        "{{\"check\":\"summary\",\"checks\":\"{}\",\"crates\":{},\"violations\":{}}}",
        json_escape(&checks_run.join("+")),
        crates_checked,
        violations.len()
    );
    // Human-facing summary on stderr.
    if violations.is_empty() {
        eprintln!(
            "policy OK: {} crates, checks: {}",
            crates_checked,
            checks_run.join(", ")
        );
        ExitCode::SUCCESS
    } else {
        for v in violations {
            eprintln!("VIOLATION [{}] {}: {}", v.check, v.crate_name, v.detail);
        }
        eprintln!("{} violation(s)", violations.len());
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    let cmd = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "check-all".to_string());
    let root = std::env::var("CARGO_WORKSPACE_DIR").map_or_else(
        |_| {
            // xtask runs from the workspace root or from xtask/; find the root by Cargo.toml.
            let cwd = std::env::current_dir().expect("cwd");
            if cwd.join("crates").is_dir() {
                cwd
            } else {
                cwd.parent().map(Path::to_path_buf).unwrap_or(cwd)
            }
        },
        PathBuf::from,
    );
    let manifests = match load_workspace(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (violations, checks): (Vec<Violation>, Vec<&str>) = match cmd.as_str() {
        "check-layers" => (check_layers(&manifests), vec!["layers"]),
        "check-deps" => (check_deps(&manifests), vec!["dependency-policy"]),
        "check-contracts" => (check_contracts(&manifests), vec!["contracts"]),
        "check-all" => {
            let mut v = check_layers(&manifests);
            v.extend(check_deps(&manifests));
            v.extend(check_contracts(&manifests));
            (v, vec!["layers", "dependency-policy", "contracts"])
        }
        other => {
            eprintln!(
                "unknown command {other:?}; use check-layers|check-deps|check-contracts|check-all"
            );
            return ExitCode::FAILURE;
        }
    };
    emit(
        &violations,
        &dev_dep_notes(&manifests),
        &checks,
        manifests.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, layer: &str, deps: &[&str]) -> Manifest {
        let mut toml = format!(
            "[package]\nname = \"{name}\"\n[package.metadata.frankensim]\nlayer = \"{layer}\"\n[dependencies]\n"
        );
        for d in deps {
            let _ = writeln!(toml, "{d} = {{ path = \"../{d}\" }}");
        }
        parse_manifest(Path::new(&format!("crates/{name}/Cargo.toml")), &toml)
            .expect("fixture parses")
    }

    #[test]
    fn parses_generated_manifest_shape() {
        let m = manifest("fs-exec", "L0", &["fs-qty", "asupersync"]);
        assert_eq!(m.name, "fs-exec");
        assert_eq!(m.layer, Layer::L0);
        assert_eq!(
            m.runtime_deps,
            vec!["fs-qty".to_string(), "asupersync".to_string()]
        );
    }

    #[test]
    fn missing_layer_declaration_is_an_error() {
        let toml = "[package]\nname = \"fs-x\"\n[dependencies]\n";
        let err = parse_manifest(Path::new("crates/fs-x/Cargo.toml"), toml).unwrap_err();
        assert!(
            err.contains("missing [package.metadata.frankensim] layer"),
            "got: {err}"
        );
    }

    #[test]
    fn layer_direction_allows_downward_and_util() {
        let ms = vec![
            manifest("fs-qty", "UTIL", &[]),
            manifest("fs-la", "L1", &["fs-qty", "fs-substrate"]),
            manifest("fs-substrate", "L0", &["fs-qty"]),
        ];
        assert!(check_layers(&ms).is_empty());
    }

    #[test]
    fn seeded_layer_violation_is_caught() {
        // L1 depending on L2 must fail: lower layers never know about higher ones.
        let ms = vec![
            manifest("fs-geom", "L2", &[]),
            manifest("fs-la", "L1", &["fs-geom"]),
        ];
        let v = check_layers(&ms);
        assert_eq!(v.len(), 1, "expected exactly one violation, got {v:?}");
        assert_eq!(v[0].crate_name, "fs-la");
        assert!(v[0].detail.contains("must not depend on fs-geom"));
    }

    #[test]
    fn ascent_and_lumen_are_siblings() {
        // LUMEN (L5) must not depend on ASCENT (L4) even though 4 < 5.
        let ms = vec![
            manifest("fs-opt", "L4", &[]),
            manifest("fs-render", "L5", &["fs-opt"]),
        ];
        let v = check_layers(&ms);
        assert_eq!(v.len(), 1, "L5 -> L4 must be rejected: {v:?}");
        // And HELM (L6) may depend on both.
        let ms = vec![
            manifest("fs-opt", "L4", &[]),
            manifest("fs-render", "L5", &[]),
            manifest("fs-ir", "L6", &["fs-opt", "fs-render"]),
        ];
        assert!(check_layers(&ms).is_empty());
    }

    #[test]
    fn seeded_forbidden_external_dependency_is_caught() {
        let ms = vec![manifest("fs-la", "L1", &["ndarray"])];
        let v = check_deps(&ms);
        assert_eq!(v.len(), 1, "expected policy violation for ndarray: {v:?}");
        assert!(v[0].detail.contains("Franken-constellation"));
    }

    #[test]
    fn constellation_dependencies_are_permitted() {
        let ms = vec![manifest("fs-exec", "L0", &["asupersync"])];
        assert!(check_deps(&ms).is_empty());
    }

    #[test]
    fn tool_crates_are_never_dependable() {
        let ms = vec![
            manifest("xtask", "TOOL", &[]),
            manifest("fs-la", "L1", &["xtask"]),
        ];
        assert_eq!(check_layers(&ms).len(), 1);
    }
}
