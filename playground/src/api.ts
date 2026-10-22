import { invoke } from "@tauri-apps/api/core";
import type { AppItem, RunOutput } from "./types";

const isTauri = (): boolean => {
  return typeof window !== "undefined" && Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
};

export async function loadApps(): Promise<AppItem[]> {
  if (isTauri()) {
    return await invoke<AppItem[]>("load_apps");
  }
  const saved = localStorage.getItem("rhp_playground_apps");
  if (saved) {
    try {
      return JSON.parse(saved);
    } catch {
      // fallback
    }
  }
  return [];
}

export async function saveApps(apps: AppItem[]): Promise<void> {
  if (isTauri()) {
    await invoke("save_apps", { apps });
  } else {
    localStorage.setItem("rhp_playground_apps", JSON.stringify(apps));
  }
}

export async function getDefaultCode(): Promise<string> {
  if (isTauri()) {
    return await invoke<string>("get_default_code");
  }
  return `$int_value = 1;
$float_value = 1.1;
$bool_value = true;
$string_value = 'hello world';

$arr_dict = [
    "name" => "RHP Playground",
    "mode" => "Standalone",
    "description" => "Easily run and test PHP & RHP code instantly",
];

$arr_list = [ 1, 2, 3 ];

dd(
  $int_value, 
  $float_value, 
  $bool_value, 
  $string_value, 
  $arr_dict, 
  $arr_list
);
`;
}

export async function resolveRhp(path: string): Promise<string> {
  if (isTauri()) {
    return await invoke<string>("resolve_rhp", { path });
  }
  return path || "rhp";
}

export async function pickDirectory(): Promise<string | null> {
  if (isTauri()) {
    return await invoke<string | null>("pick_directory");
  }
  return prompt("Enter directory path:") || null;
}

export async function pickFile(): Promise<string | null> {
  if (isTauri()) {
    return await invoke<string | null>("pick_file");
  }
  return prompt("Enter file path:") || null;
}

export async function runScript(
  code: string,
  projectPath: string,
  rhpPath: string
): Promise<RunOutput> {
  if (isTauri()) {
    return await invoke<RunOutput>("run_script", {
      code,
      projectPath,
      rhpPath,
    });
  }

  // Web fallback simulation
  await new Promise((r) => setTimeout(r, 300));
  return {
    formatted_code: code,
    output: `[Browser Dev Mode Demo Output]\nExecution completed.\nInput Code length: ${code.length} chars`,
    is_error: false,
    elapsed_ms: 12,
  };
}

export async function cancelRun(): Promise<void> {
  if (isTauri()) {
    await invoke("cancel_run");
  }
}
