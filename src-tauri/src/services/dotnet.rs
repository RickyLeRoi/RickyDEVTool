use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DotnetProject {
    pub path: String,
    pub sln_path: Option<String>,
    pub projects: Vec<CsProject>,
    pub startup_project_path: Option<String>,
    pub selected_profile: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsProject {
    pub csproj_path: String,
    pub name: String,
    pub is_executable: bool,
    pub target_frameworks: Vec<String>,
    pub launch_profiles: Vec<LaunchProfile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProfile {
    pub name: String,
    pub command_name: String,
    pub application_url: Option<String>,
    pub runnable_cross_platform: bool,
}

pub fn inspect(
    path: &str,
    startup_override: Option<&str>,
    profile_override: Option<&str>,
) -> Result<DotnetProject, String> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("cartella non trovata".to_string());
    }

    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .collect();

    let sln_path = entries
        .iter()
        .find(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(),
                Some("sln") | Some("slnx")
            )
        })
        .cloned();

    let csproj_paths: Vec<PathBuf> = if let Some(sln) = &sln_path {
        let content = std::fs::read_to_string(sln).map_err(|e| e.to_string())?;
        parse_sln(&content)
            .into_iter()
            .map(|rel| sln.parent().unwrap_or(dir).join(rel.replace('\\', "/")))
            .filter(|p| p.is_file())
            .collect()
    } else {
        entries
            .iter()
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("csproj"))
            .cloned()
            .collect()
    };

    if csproj_paths.is_empty() {
        return Err("nessun progetto .NET trovato (.sln o .csproj)".to_string());
    }

    let mut projects = Vec::new();
    for csproj in &csproj_paths {
        let content = std::fs::read_to_string(csproj).unwrap_or_default();
        let (is_executable, target_frameworks) = parse_csproj(&content);
        let launch_profiles = csproj
            .parent()
            .map(|p| read_launch_profiles(p))
            .unwrap_or_default();
        projects.push(CsProject {
            csproj_path: csproj.to_string_lossy().to_string(),
            name: csproj
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            is_executable,
            target_frameworks,
            launch_profiles,
        });
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));

    let executables: Vec<&CsProject> = projects.iter().filter(|p| p.is_executable).collect();
    let startup_project_path = startup_override
        .filter(|o| executables.iter().any(|p| p.csproj_path == *o))
        .map(String::from)
        .or_else(|| (executables.len() == 1).then(|| executables[0].csproj_path.clone()));

    let selected_profile = startup_project_path.as_ref().and_then(|startup| {
        let project = projects.iter().find(|p| &p.csproj_path == startup)?;
        profile_override
            .filter(|o| project.launch_profiles.iter().any(|lp| lp.name == *o))
            .map(String::from)
            .or_else(|| {
                project
                    .launch_profiles
                    .iter()
                    .find(|lp| lp.runnable_cross_platform)
                    .map(|lp| lp.name.clone())
            })
    });

    Ok(DotnetProject {
        path: path.to_string(),
        sln_path: sln_path.map(|p| p.to_string_lossy().to_string()),
        projects,
        startup_project_path,
        selected_profile,
    })
}

pub fn parse_sln(content: &str) -> Vec<String> {
    let mut result = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Project(") {
            let parts: Vec<&str> = rest.split('"').collect();
            if let Some(path) = parts.iter().find(|p| p.to_lowercase().ends_with(".csproj")) {
                result.push(path.to_string());
            }
        } else if line.contains("<Project ") && line.contains("Path=") {
            if let Some(start) = line.find("Path=\"") {
                let rest = &line[start + 6..];
                if let Some(end) = rest.find('"') {
                    let path = &rest[..end];
                    if path.to_lowercase().ends_with(".csproj") {
                        result.push(path.to_string());
                    }
                }
            }
        }
    }
    result
}

pub fn parse_csproj(content: &str) -> (bool, Vec<String>) {
    let is_web_sdk = content.contains("Sdk=\"Microsoft.NET.Sdk.Web\"")
        || content.contains("Sdk=\"Microsoft.NET.Sdk.Worker\"");
    let output_exe = extract_tag(content, "OutputType")
        .map(|v| v.eq_ignore_ascii_case("Exe") || v.eq_ignore_ascii_case("WinExe"))
        .unwrap_or(false);

    let mut tfms = Vec::new();
    if let Some(single) = extract_tag(content, "TargetFramework") {
        tfms.push(single);
    }
    if let Some(multi) = extract_tag(content, "TargetFrameworks") {
        tfms.extend(multi.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }

    (is_web_sdk || output_exe, tfms)
}

fn extract_tag(content: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = content.find(&open)? + open.len();
    let end = content[start..].find(&close)? + start;
    Some(content[start..end].trim().to_string())
}

fn read_launch_profiles(project_dir: &Path) -> Vec<LaunchProfile> {
    let path = project_dir.join("Properties").join("launchSettings.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(profiles) = parsed.get("profiles").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    profiles
        .iter()
        .map(|(name, profile)| {
            let command_name = profile
                .get("commandName")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            LaunchProfile {
                name: name.clone(),
                runnable_cross_platform: command_name == "Project",
                application_url: profile
                    .get("applicationUrl")
                    .and_then(|u| u.as_str())
                    .map(String::from),
                command_name,
            }
        })
        .collect()
}

pub fn command_for(project: &DotnetProject, action: &str) -> Result<(String, Vec<String>), String> {
    let build_target = project
        .sln_path
        .clone()
        .or_else(|| project.startup_project_path.clone())
        .ok_or("nessuna solution o progetto di avvio")?;

    match action {
        "run" => {
            let startup = project
                .startup_project_path
                .clone()
                .ok_or("scegli prima il progetto di avvio")?;
            let mut args = vec!["run".to_string(), "--project".to_string(), startup];
            if let Some(profile) = &project.selected_profile {
                args.push("--launch-profile".to_string());
                args.push(profile.clone());
            }
            Ok(("dotnet".to_string(), args))
        }
        "build" => Ok(("dotnet".to_string(), vec!["build".to_string(), build_target])),
        "rebuild" => Ok((
            "dotnet".to_string(),
            vec!["build".to_string(), build_target, "-t:Rebuild".to_string()],
        )),
        "clean" => Ok(("dotnet".to_string(), vec!["clean".to_string(), build_target])),
        other => Err(format!("azione sconosciuta: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLN_FIXTURE: &str = r#"
Microsoft Visual Studio Solution File, Format Version 12.00
# Visual Studio Version 17
Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "Api", "src\Api\Api.csproj", "{11111111-1111-1111-1111-111111111111}"
EndProject
Project("{FAE04EC0-301F-11D3-BF4B-00C04F79EFBC}") = "Core", "src\Core\Core.csproj", "{22222222-2222-2222-2222-222222222222}"
EndProject
Project("{2150E333-8FDC-42A3-9474-1A3956D46DE8}") = "Soluzione", "Soluzione", "{33333333-3333-3333-3333-333333333333}"
EndProject
"#;

    #[test]
    fn parse_sln_estrae_solo_csproj() {
        let projects = parse_sln(SLN_FIXTURE);
        assert_eq!(projects, vec!["src\\Api\\Api.csproj", "src\\Core\\Core.csproj"]);
    }

    #[test]
    fn parse_csproj_web_ed_exe() {
        let web = r#"<Project Sdk="Microsoft.NET.Sdk.Web"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>"#;
        let (exe, tfms) = parse_csproj(web);
        assert!(exe);
        assert_eq!(tfms, vec!["net9.0"]);

        let lib = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFrameworks>net9.0;netstandard2.0</TargetFrameworks></PropertyGroup></Project>"#;
        let (exe, tfms) = parse_csproj(lib);
        assert!(!exe);
        assert_eq!(tfms, vec!["net9.0", "netstandard2.0"]);

        let console = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>"#;
        assert!(parse_csproj(console).0);
    }

    fn setup_solution() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/Api/Properties")).unwrap();
        std::fs::create_dir_all(root.join("src/Core")).unwrap();
        std::fs::write(root.join("Soluzione.sln"), SLN_FIXTURE).unwrap();
        std::fs::write(
            root.join("src/Api/Api.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk.Web"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/Core/Core.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/Api/Properties/launchSettings.json"),
            r#"{"profiles":{"http":{"commandName":"Project","applicationUrl":"http://localhost:5010"},"IIS Express":{"commandName":"IISExpress"}}}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn inspect_solution_completa() {
        let dir = setup_solution();
        let project = inspect(dir.path().to_str().unwrap(), None, None).expect("inspect");
        assert!(project.sln_path.is_some());
        assert_eq!(project.projects.len(), 2);

        let api = project.projects.iter().find(|p| p.name == "Api").unwrap();
        assert!(api.is_executable);
        assert_eq!(api.launch_profiles.len(), 2);
        let iis = api.launch_profiles.iter().find(|p| p.name == "IIS Express").unwrap();
        assert!(!iis.runnable_cross_platform);

        assert!(project.startup_project_path.as_ref().unwrap().ends_with("Api.csproj"));
        assert_eq!(project.selected_profile.as_deref(), Some("http"));

        let (program, args) = command_for(&project, "run").expect("run");
        assert_eq!(program, "dotnet");
        assert!(args.contains(&"--launch-profile".to_string()));
        assert!(args.contains(&"http".to_string()));

        let (_, rebuild_args) = command_for(&project, "rebuild").expect("rebuild");
        assert!(rebuild_args.iter().any(|a| a.ends_with(".sln")));
        assert!(rebuild_args.contains(&"-t:Rebuild".to_string()));
    }
}
