import { invoke } from '@tauri-apps/api/core';
import type { Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Settings } from '@/types/app';

export type SourceRecord = {
  id: string;
  path: string;
  archiveType: 'zip' | 'tgz' | 'folder';
  sizeBytes: number;
  entryCount: number;
  entryCountKnown: boolean;
};

export type AddSourcesResult = {
  archives: SourceRecord[];
  warnings: string[];
};

export type ScanSourcesResult = {
  files: string[];
  counts: { photos: number; videos: number; live: number; totalGb: number };
  warnings: string[];
};

export type ScanProgress = {
  archiveIndex: number;
  archiveCount: number;
  currentArchive: string;
  entriesSeen: number;
  bytesRead: number;
  totalBytes: number;
  stage: 'preparing' | 'scanning' | 'done' | 'cancelled' | 'error';
  result: ScanSourcesResult | null;
  error: string | null;
};

export type PreviewNode = {
  name: string;
  folder: boolean;
  tag: string | null;
  children: PreviewNode[];
};

export type PreviewResult = {
  files: string[];
  tree: PreviewNode[];
  warnings: string[];
  requiredBytes: number;
  freeBytes: number;
};

export type RunSummary = {
  planned: number;
  processed: number;
  skipped: number;
  failed: number;
  filesWithoutJson: number;
  otherFiles: number;
  collisions: number;
  datesRestored: number;
  locationsAdded: number;
  livePhotosMerged: number;
  archivesDeleted: number;
  dryRun: boolean;
  paused: boolean;
  cancelled: boolean;
  warnings: string[];
  problems: string[];
  attention: ReportItem[];
};

export type ReportItem = {
  id: string;
  file: string;
  destination: string | null;
  reason: string;
  group: string;
  isVideo: boolean;
};

export type ReportDocument = {
  createdAt: string;
  elapsedSeconds: number;
  outputFolder: string;
  settings: Settings;
  summary: RunSummary;
  items: ReportItem[];
};

export type ProcessingStartResult = {
  accepted: boolean;
  requiredBytes: number;
  freeBytes: number;
  shortfallBytes: number;
  warnings: string[];
  report: ReportDocument | null;
};

export type RunStatus = {
  running: boolean;
  paused: boolean;
  completed: number;
  total: number;
  summary: RunSummary | null;
};

export type ReportFormat = 'csv' | 'json';

export type ReportExport = {
  path: string;
  summary: string;
};

export type ProcessingProgress = {
  currentFile: string | null;
  completedFiles: number;
  totalFiles: number;
  processedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  etaSeconds: number | null;
  warningsCount: number;
  warnings: string[];
};

export type BackendCommandContracts = {
  add_sources: { args: { paths: string[] }; result: AddSourcesResult };
  scan_sources: { args: { paths: string[] }; result: ScanSourcesResult };
  start_scan: {
    args: { paths: string[]; onEvent: Channel<ScanProgress> };
    result: string;
  };
  cancel_scan: { args: Record<string, never>; result: boolean };
  remove_source: { args: { sourceId: string }; result: ScanSourcesResult };
  get_preview: { args: { settings: Settings }; result: PreviewResult };
  start_processing: { args: { settings: Settings }; result: ProcessingStartResult };
  run_status: { args: Record<string, never>; result: RunStatus };
  cancel_processing: { args: Record<string, never>; result: boolean };
  load_settings: { args: Record<string, never>; result: Settings };
  save_settings: { args: { settings: Settings }; result: void };
  export_report: {
    args: { format: ReportFormat; destination: string };
    result: ReportExport;
  };
  get_report: { args: Record<string, never>; result: ReportDocument | null };
  get_review_items: { args: Record<string, never>; result: ReportItem[] };
  apply_review_fix: {
    args: { itemId: string; choice: 'fileDate' | 'pickDate' | 'leave'; pickedDate: string | null };
    result: boolean;
  };
  pause_run: { args: Record<string, never>; result: boolean };
  resume_run: { args: Record<string, never>; result: boolean };
};

export type ApiEventPayloads = {
  'processing:progress': ProcessingProgress;
};

export async function callCommand<Name extends keyof BackendCommandContracts>(
  name: Name,
  payload: BackendCommandContracts[Name]['args'],
): Promise<BackendCommandContracts[Name]['result']> {
  return invoke<BackendCommandContracts[Name]['result']>(name, payload);
}

export async function subscribeToEvent<EventName extends keyof ApiEventPayloads>(
  event: EventName,
  handler: (payload: ApiEventPayloads[EventName]) => void,
) {
  return listen<ApiEventPayloads[EventName]>(event, (message) => handler(message.payload));
}
