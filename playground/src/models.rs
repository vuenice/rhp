use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppItem {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub rhp_path: String,
    pub code: String,
    pub output: String,
    pub color_idx: usize,
}

impl AppItem {
    pub fn initial_letter(&self) -> String {
        self.name
            .trim()
            .chars()
            .next()
            .unwrap_or('A')
            .to_uppercase()
            .to_string()
    }
}

pub fn default_standalone_code() -> String {
    r#"$data = [
    "name" => "RHP Playground",
    "mode" => "Standalone",
    "description" => "Easily run and test PHP & RHP code instantly",
];

print_r($data);
"#.to_string()
}

pub fn is_demo_connection(app: &AppItem) -> bool {
    app.id == "local-1"
        || app.id == "vapor-1"
        || app.id == "laravel-app"
        || app.id == "vue-app"
        || app.id.starts_with("demo-")
        || (app.name == "Local" && app.project_path == "/home/yogesh/code/rhp")
        || (app.name == "Vapor" && app.project_path == "/home/yogesh/code/rhp/examples/todo")
        || (app.name == "Laravel" && app.project_path == "/var/www/laravel")
}

pub fn default_apps() -> Vec<AppItem> {
    vec![]
}

fn get_storage_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    if let Ok(home) = std::env::var("HOME") {
        path = PathBuf::from(home);
    }
    path.join(".rhp_playground_apps.json")
}

pub fn load_saved_apps() -> Vec<AppItem> {
    let path = get_storage_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(mut apps) = serde_json::from_str::<Vec<AppItem>>(&content) {
            let original_len = apps.len();
            apps.retain(|app| !is_demo_connection(app));
            if apps.len() != original_len {
                save_apps(&apps);
            }
            return apps;
        }
    }
    default_apps()
}

pub fn save_apps(apps: &[AppItem]) {
    let path = get_storage_path();
    if let Ok(content) = serde_json::to_string_pretty(apps) {
        let _ = fs::write(path, content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_demo_connection() {
        let demo1 = AppItem {
            id: "local-1".to_string(),
            name: "Local".to_string(),
            project_path: "/home/yogesh/code/rhp".to_string(),
            rhp_path: "rhp".to_string(),
            code: "".to_string(),
            output: "".to_string(),
            color_idx: 0,
        };
        assert!(is_demo_connection(&demo1));

        let demo2 = AppItem {
            id: "vapor-1".to_string(),
            name: "Vapor".to_string(),
            project_path: "/home/yogesh/code/rhp/examples/todo".to_string(),
            rhp_path: "rhp".to_string(),
            code: "".to_string(),
            output: "".to_string(),
            color_idx: 1,
        };
        assert!(is_demo_connection(&demo2));

        let user_app = AppItem {
            id: "app-1729500000".to_string(),
            name: "MyCustomApp".to_string(),
            project_path: "/home/yogesh/projects/custom".to_string(),
            rhp_path: "rhp".to_string(),
            code: "".to_string(),
            output: "".to_string(),
            color_idx: 2,
        };
        assert!(!is_demo_connection(&user_app));
    }

    #[test]
    fn test_default_standalone_code() {
        let code = default_standalone_code();
        assert!(!code.contains("<?php"), "RHP code does not need <?php");
        assert!(!code.starts_with("<?"));

        // Verify it runs cleanly in RHP VM
        let result = rhp::vm::run_source_with_vm_and_jit(&code, false);
        assert!(result.is_ok(), "Default standalone code must execute successfully");
        let (vm, _) = result.unwrap();
        assert!(vm.output().contains("RHP Playground"));
    }
}
