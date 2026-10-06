export interface Asset {
  id: string;
  kind: 'suite' | 'case' | 'template' | 'script';
  name: string;
  description: string;
  draft_version: number;
  updated_at: string;
  draft?: any;
}

export interface AssetRevision {
  id: string;
  asset_id: string;
  version: number;
  checksum: string;
  change_note: string;
  created_at: string;
}

export interface NodeInstance {
  id: string;
  type: string;
  type_version: number;
  name: string;
  phase?: 'setup' | 'main' | 'cleanup';
  timeout_seconds: number;
  config: Record<string, any>;
  inputs?: Record<string, any>;
  position: { x: number; y: number };
}

export interface Edge {
  id: string;
  source: string;
  target: string;
  condition?: { expression: string; label: string };
}

export interface CaseDefinition {
  id: string;
  name: string;
  description: string;
  nodes: NodeInstance[];
  edges: Edge[];
}

export interface SuiteDefinition {
  id: string;
  name: string;
  description: string;
  execution_mode: 'sequential' | 'parallel';
  cases: Array<{ case_id: string; revision_id?: string; ordinal: number }>;
}

export interface SuiteRun {
  id: string;
  suite_revision_id: string;
  environment_id: string;
  status: 'QUEUED' | 'RUNNING' | 'PASSED' | 'FAILED' | 'ERROR' | 'CANCELED' | 'INTERRUPTED';
  started_at?: string;
  finished_at?: string;
  created_at: string;
}

export interface ProgressInfo {
  mode: string;
  percent?: number;
  terminal_nodes: number;
  planned_nodes: number;
}

export interface RunStats {
  run_id: string;
  status: string;
  progress: ProgressInfo;
  stats: Record<string, any>;
  last_sequence: number;
  updated_at: string;
}

export interface Environment {
  id: string;
  name: string;
  description: string;
  variables_json: string;
}

export interface Connection {
  id: string;
  name: string;
  connector_type: string;
  settings_json: string;
}

export interface SecretItem {
  id: string;
  name: string;
  key_version: number;
  secret_version: number;
  updated_at: string;
}
