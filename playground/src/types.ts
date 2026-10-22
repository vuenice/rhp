export interface AppItem {
  id: string;
  name: string;
  project_path: string;
  rhp_path: string;
  code: string;
  output: string;
  color_idx: number;
}

export interface RunOutput {
  formatted_code: string | null;
  output: string;
  is_error: boolean;
  elapsed_ms: number;
}
