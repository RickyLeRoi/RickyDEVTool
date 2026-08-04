use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionCategory {
    Env,
    Install,
    Build,
    Run,
    Test,
    Clean,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSpec {
    pub id: String,
    pub label: String,
    pub category: ActionCategory,
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerInfo {
    pub kind: String,
    pub path: String,
    pub tool: String,
    pub notes: Vec<String>,
    pub actions: Vec<ActionSpec>,
}

fn act(
    id: &str,
    label: &str,
    category: ActionCategory,
    program: impl Into<String>,
    args: &[&str],
) -> ActionSpec {
    ActionSpec {
        id: id.to_string(),
        label: label.to_string(),
        category,
        program: program.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn inspect(kind: &str, path: &str) -> Result<RunnerInfo, String> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("cartella non trovata".to_string());
    }
    let actions = match kind {
        "python" => python_actions(dir),
        "rust" => rust_actions(dir),
        "tauri" => tauri_actions(dir),
        "flutter" => flutter_actions(dir),
        other => return Err(format!("runner non supportato: {other}")),
    };
    Ok(RunnerInfo {
        kind: kind.to_string(),
        path: path.to_string(),
        tool: actions.tool,
        notes: actions.notes,
        actions: actions.actions,
    })
}

pub fn resolve(kind: &str, path: &str, action_id: &str) -> Result<ActionSpec, String> {
    let info = inspect(kind, path)?;
    info.actions
        .into_iter()
        .find(|a| a.id == action_id)
        .ok_or_else(|| format!("azione \"{action_id}\" non disponibile per {kind}"))
}

struct Built {
    tool: String,
    notes: Vec<String>,
    actions: Vec<ActionSpec>,
}

fn system_python() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

fn venv_python(dir: &Path) -> Option<String> {
    for rel in [".venv/bin/python", ".venv/Scripts/python.exe"] {
        let p = dir.join(rel);
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

fn python_actions(dir: &Path) -> Built {
    let has = |f: &str| dir.join(f).is_file();
    let pyproject = std::fs::read_to_string(dir.join("pyproject.toml")).unwrap_or_default();
    let venv = venv_python(dir);
    let sys_py = system_python();

    let tool = if has("uv.lock") || pyproject.contains("[tool.uv]") {
        "uv"
    } else if has("poetry.lock") || pyproject.contains("[tool.poetry]") {
        "poetry"
    } else if has("Pipfile") {
        "pipenv"
    } else {
        "pip"
    };

    let mut notes = Vec::new();
    if venv.is_some() {
        notes.push("venv presente (.venv)".to_string());
    } else if tool == "pip" {
        notes.push("nessun venv: crea prima l'ambiente".to_string());
    }
    if has("manage.py") {
        notes.push("Django rilevato (manage.py)".to_string());
    }

    let mut actions = Vec::new();

    match tool {
        "uv" => actions.push(act("create-env", "Crea venv (uv)", ActionCategory::Env, "uv", &["venv"])),
        _ => {
            let label = if venv.is_some() { "Ricrea venv" } else { "Crea venv" };
            actions.push(act("create-env", label, ActionCategory::Env, sys_py, &["-m", "venv", ".venv"]));
        }
    }

    match tool {
        "uv" => actions.push(act("install", "Install (uv sync)", ActionCategory::Install, "uv", &["sync"])),
        "poetry" => actions.push(act("install", "Install (poetry)", ActionCategory::Install, "poetry", &["install"])),
        "pipenv" => actions.push(act("install", "Install (pipenv)", ActionCategory::Install, "pipenv", &["install"])),
        _ => {
            let py = venv.clone().unwrap_or_else(|| sys_py.to_string());
            if has("requirements.txt") {
                actions.push(act("install", "Install (requirements.txt)", ActionCategory::Install, py, &["-m", "pip", "install", "-r", "requirements.txt"]));
            } else if has("pyproject.toml") || has("setup.py") {
                actions.push(act("install", "Install (pip -e .)", ActionCategory::Install, py, &["-m", "pip", "install", "-e", "."]));
            }
        }
    }

    let entry: Option<Vec<&str>> = if has("manage.py") {
        Some(vec!["manage.py", "runserver"])
    } else if has("main.py") {
        Some(vec!["main.py"])
    } else if has("app.py") {
        Some(vec!["app.py"])
    } else {
        None
    };
    if let Some(entry) = entry {
        let (program, args): (String, Vec<String>) = match tool {
            "uv" | "poetry" | "pipenv" => {
                let mut a = vec![tool.to_string(), "run".to_string(), "python".to_string()];
                a.extend(entry.iter().map(|s| s.to_string()));
                (a.remove(0), a)
            }
            _ => {
                let py = venv.clone().unwrap_or_else(|| sys_py.to_string());
                (py, entry.iter().map(|s| s.to_string()).collect())
            }
        };
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        actions.push(act("run", "Run", ActionCategory::Run, program, &arg_refs));
    }

    match tool {
        "uv" => actions.push(act("build", "Build (uv)", ActionCategory::Build, "uv", &["build"])),
        "poetry" => actions.push(act("build", "Build (poetry)", ActionCategory::Build, "poetry", &["build"])),
        _ => {
            if has("pyproject.toml") || has("setup.py") {
                let py = venv.clone().unwrap_or_else(|| sys_py.to_string());
                actions.push(act("build", "Build (python -m build)", ActionCategory::Build, py, &["-m", "build"]));
            }
        }
    }

    Built { tool: tool.to_string(), notes, actions }
}

fn rust_actions(dir: &Path) -> Built {
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
    let is_workspace = cargo.contains("[workspace]");
    let has_bin = dir.join("src/main.rs").is_file()
        || dir.join("src/bin").is_dir()
        || cargo.contains("[[bin]]");

    let mut notes = Vec::new();
    if is_workspace {
        notes.push("workspace Cargo".to_string());
    }
    if !has_bin && !is_workspace {
        notes.push("nessun binario rilevato (probabile libreria)".to_string());
    }

    let mut actions = vec![
        act("fetch", "Fetch deps", ActionCategory::Install, "cargo", &["fetch"]),
        act("build", "Build", ActionCategory::Build, "cargo", &["build"]),
        act("build-release", "Build --release", ActionCategory::Build, "cargo", &["build", "--release"]),
    ];
    if has_bin {
        actions.push(act("run", "Run", ActionCategory::Run, "cargo", &["run"]));
    }
    actions.push(act("test", "Test", ActionCategory::Test, "cargo", &["test"]));
    actions.push(act("clean", "Clean", ActionCategory::Clean, "cargo", &["clean"]));

    Built { tool: "cargo".to_string(), notes, actions }
}

fn frontend_pm(dir: &Path) -> &'static str {
    if dir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if dir.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    }
}

fn tauri_via_pm(pm: &str, sub: &str) -> (String, Vec<String>) {
    match pm {
        "pnpm" => ("pnpm".to_string(), vec!["tauri".to_string(), sub.to_string()]),
        "yarn" => ("yarn".to_string(), vec!["tauri".to_string(), sub.to_string()]),
        _ => ("npx".to_string(), vec!["tauri".to_string(), sub.to_string()]),
    }
}

fn tauri_actions(dir: &Path) -> Built {
    let has_pkg = dir.join("package.json").is_file();
    let mut actions = Vec::new();
    let mut notes = Vec::new();
    let tool;

    if has_pkg {
        let pm = frontend_pm(dir);
        tool = pm.to_string();
        notes.push(format!("frontend: {pm}"));
        actions.push(act("install", "Install (frontend)", ActionCategory::Install, pm, &["install"]));
        for (id, label, sub, cat) in [
            ("dev", "Tauri dev", "dev", ActionCategory::Run),
            ("build", "Tauri build", "build", ActionCategory::Build),
        ] {
            let (program, args) = tauri_via_pm(pm, sub);
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            actions.push(act(id, label, cat, program, &arg_refs));
        }
    } else {
        tool = "cargo".to_string();
        notes.push("nessun package.json: CLI Tauri via cargo".to_string());
        actions.push(act("dev", "Tauri dev", ActionCategory::Run, "cargo", &["tauri", "dev"]));
        actions.push(act("build", "Tauri build", ActionCategory::Build, "cargo", &["tauri", "build"]));
    }

    Built { tool, notes, actions }
}

fn flutter_actions(dir: &Path) -> Built {
    let pubspec = std::fs::read_to_string(dir.join("pubspec.yaml")).unwrap_or_default();
    let is_flutter = pubspec.contains("flutter");
    let tool = if is_flutter { "flutter" } else { "dart" };

    let mut notes = Vec::new();
    notes.push(if is_flutter {
        "progetto Flutter".to_string()
    } else {
        "pacchetto Dart".to_string()
    });

    let mut actions = vec![act("pub-get", "Pub get", ActionCategory::Install, tool, &["pub", "get"])];

    if is_flutter {
        actions.push(act("run", "Run", ActionCategory::Run, "flutter", &["run"]));
        actions.push(act("build-apk", "Build APK", ActionCategory::Build, "flutter", &["build", "apk"]));
        actions.push(act("build-web", "Build web", ActionCategory::Build, "flutter", &["build", "web"]));
        if cfg!(target_os = "macos") {
            actions.push(act("build-macos", "Build macOS", ActionCategory::Build, "flutter", &["build", "macos"]));
        }
        if cfg!(target_os = "windows") {
            actions.push(act("build-windows", "Build Windows", ActionCategory::Build, "flutter", &["build", "windows"]));
        }
        actions.push(act("test", "Test", ActionCategory::Test, "flutter", &["test"]));
        actions.push(act("clean", "Clean", ActionCategory::Clean, "flutter", &["clean"]));
    } else {
        if dir.join("bin").is_dir() {
            actions.push(act("run", "Run", ActionCategory::Run, "dart", &["run"]));
        }
        actions.push(act("test", "Test", ActionCategory::Test, "dart", &["test"]));
    }

    Built { tool: tool.to_string(), notes, actions }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(info: &RunnerInfo) -> Vec<&str> {
        info.actions.iter().map(|a| a.id.as_str()).collect()
    }
    fn find<'a>(info: &'a RunnerInfo, id: &str) -> &'a ActionSpec {
        info.actions.iter().find(|a| a.id == id).expect("azione presente")
    }

    #[test]
    fn python_pip_con_requirements_e_django() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        std::fs::write(dir.path().join("manage.py"), "").unwrap();
        let info = inspect("python", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.tool, "pip");
        assert!(ids(&info).contains(&"create-env"));
        let install = find(&info, "install");
        assert!(install.args.iter().any(|a| a == "requirements.txt"));
        let run = find(&info, "run");
        assert_eq!(run.args, vec!["manage.py", "runserver"]);
        assert_eq!(run.program, system_python());
    }

    #[test]
    fn python_venv_e_poetry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "[tool.poetry]\nname='x'").unwrap();
        std::fs::write(dir.path().join("main.py"), "").unwrap();
        let info = inspect("python", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.tool, "poetry");
        assert_eq!(find(&info, "install").program, "poetry");
        let run = find(&info, "run");
        assert_eq!(run.program, "poetry");
        assert_eq!(run.args, vec!["run", "python", "main.py"]);
        assert_eq!(find(&info, "build").program, "poetry");
    }

    #[test]
    fn python_usa_il_python_del_venv_se_presente() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        std::fs::create_dir_all(dir.path().join(".venv/bin")).unwrap();
        std::fs::write(dir.path().join(".venv/bin/python"), "").unwrap();
        std::fs::write(dir.path().join("app.py"), "").unwrap();
        let info = inspect("python", dir.path().to_str().unwrap()).unwrap();
        let install = find(&info, "install");
        assert!(install.program.ends_with(".venv/bin/python"), "{}", install.program);
        let run = find(&info, "run");
        assert!(run.program.ends_with(".venv/bin/python"));
        assert_eq!(run.args, vec!["app.py"]);
    }

    #[test]
    fn rust_libreria_niente_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        let info = inspect("rust", dir.path().to_str().unwrap()).unwrap();
        assert!(!ids(&info).contains(&"run"));
        assert!(ids(&info).contains(&"build"));
        assert!(ids(&info).contains(&"build-release"));
    }

    #[test]
    fn rust_binario_ha_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main(){}").unwrap();
        let info = inspect("rust", dir.path().to_str().unwrap()).unwrap();
        let run = find(&info, "run");
        assert_eq!(run.program, "cargo");
        assert_eq!(run.args, vec!["run"]);
    }

    #[test]
    fn tauri_con_pnpm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("src-tauri")).unwrap();
        let info = inspect("tauri", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.tool, "pnpm");
        assert_eq!(find(&info, "install").program, "pnpm");
        let dev = find(&info, "dev");
        assert_eq!(dev.program, "pnpm");
        assert_eq!(dev.args, vec!["tauri", "dev"]);
    }

    #[test]
    fn tauri_senza_package_json_usa_cargo() {
        let dir = tempfile::tempdir().unwrap();
        let info = inspect("tauri", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.tool, "cargo");
        let dev = find(&info, "dev");
        assert_eq!(dev.program, "cargo");
        assert_eq!(dev.args, vec!["tauri", "dev"]);
    }

    #[test]
    fn flutter_completo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pubspec.yaml"),
            "name: app\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        let info = inspect("flutter", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.tool, "flutter");
        assert_eq!(find(&info, "pub-get").args, vec!["pub", "get"]);
        assert!(ids(&info).contains(&"run"));
        assert!(ids(&info).contains(&"build-apk"));
    }

    #[test]
    fn dart_puro_niente_flutter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pubspec.yaml"), "name: cli\n").unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        let info = inspect("flutter", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(info.tool, "dart");
        assert_eq!(find(&info, "pub-get").program, "dart");
        assert!(!ids(&info).contains(&"build-apk"));
        assert_eq!(find(&info, "run").program, "dart");
    }

    #[test]
    fn resolve_ritrova_azione_e_rifiuta_ignote() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let path = dir.path().to_str().unwrap();
        assert_eq!(resolve("rust", path, "build").unwrap().program, "cargo");
        assert!(resolve("rust", path, "inesistente").is_err());
        assert!(resolve("sconosciuto", path, "build").is_err());
    }
}
