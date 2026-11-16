import { listen } from '@tauri-apps/api/event';
import type { Settings } from '@/types/app';

export type ApiEventName =
  | 'scan:progress'
  | 'run:progress'
  | 'run:finished'
  | 'processing:progress';

export type BackendCommandName =
  | 'add_sources'
  | 'scan_sources'
  | 'remove_source'
  | 'get_preview'
  | 'start_processing'
  | 'cancel_processing'
  | 'load_settings'
  | 'save_settings'
  | 'export_report';

export type SourceRecord = {
  id: string;
  path: string;
  archiveType: 'zip' | 'tgz' | 'folder';
  sizeBytes: number;
  entryCount: number;
};

export type AddSourcesResult = {
  archives: SourceRecord[];
  warnings: string[];
};

export type ScanSourcesResult = {
  files: string[];
  counts: {
    photos: number;
    videos: number;
    live: number;
    totalGb: number;
  };
  warnings: string[];
};

export type EntryType = 'photo' | 'video' | 'jsonSidecar' | 'other';

export type IndexedEntry = {
  archivePath: string;
  path: string;
  size: number;
  entryType: EntryType;
  album: string | null;
  trashed: boolean;
  archived: boolean;
  sidecarArchivePath: string | null;
  sidecarPath: string | null;
  sidecarMatch: string | null;
};

export type PreviewResult = {
  files: string[];
};

export type ProcessingStartResult = {
  accepted: boolean;
};

export type ReportFormat = 'csv' | 'json';

export type ReportExport = {
  path: string;
};

export type BackendCommandContracts = {
  add_sources: { args: { paths: string[] }; result: AddSourcesResult };
  scan_sources: { args: { paths: string[] }; result: ScanSourcesResult };
  remove_source: { args: { sourceId: string }; result: boolean };
  get_preview: { args: { settings: Settings }; result: PreviewResult };
  start_processing: { args: { settings: Settings }; result: ProcessingStartResult };
  cancel_processing: { args: Record<string, never>; result: boolean };
  load_settings: { args: Record<string, never>; result: Settings };
  save_settings: { args: { settings: Settings }; result: void };
  export_report: { args: { format: ReportFormat }; result: ReportExport };
};

export type ProcessingProgress = {
  currentFile: string | null;
  completedFiles: number;
  totalFiles: number;
  processedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  etaSeconds: number | null;
  warnings: string[];
};

export type ApiEventPayloads = {
  'scan:progress': ScanProgress;
  'run:progress': RunProgress;
  'run:finished': { ok: boolean };
  'processing:progress': ProcessingProgress;
};

export type CommandName =
  | 'scan_sources'
  | 'preview_tree'
  | 'start_run'
  | 'pause_run'
  | 'resume_run'
  | 'cancel_run'
  | 'get_report'
  | 'get_review_items'
  | 'apply_review_fix'
  | 'disk_space';

export type ScanProgress = {
  step: string;
  processed: number;
  total: number;
  message: string;
};

export type RunProgress = {
  file: string;
  completed: number;
  total: number;
  status: string;
};

export const fakeApi = {
  scanSources: async () => ({
    files: ['sample-1.zip', 'sample-2.zip'],
    counts: { photos: 184, videos: 36, live: 8, totalGb: 28.2 },
  }),
  previewTree: async () => ({ nodes: [{ name: '2024', children: [{ name: 'Paris trip' }] }] }),
  startRun: async () => ({ ok: true }),
  pauseRun: async () => ({ ok: true }),
  resumeRun: async () => ({ ok: true }),
  cancelRun: async () => ({ ok: true }),
  getReport: async () => ({ summary: 'Report ready' }),
  getReviewItems: async () => ({ items: [] }),
  applyReviewFix: async () => ({ ok: true }),
  diskSpace: async () => ({ free: 128, total: 512 }),
};

export async function callCommand<T>(name: CommandName, payload?: object): Promise<T> {
  void name;
  void payload;
  return fakeApi[name.replace(/_([a-z])/g, (_, char) => char.toUpperCase()) as keyof typeof fakeApi] as unknown as Promise<T>;
}

export async function subscribeToEvent<EventName extends ApiEventName>(
  event: EventName,
  handler: (payload: ApiEventPayloads[EventName]) => void,
) {
  return listen<ApiEventPayloads[EventName]>(event, (message) => handler(message.payload));
}
