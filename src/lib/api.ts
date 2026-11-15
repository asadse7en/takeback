import { listen } from '@tauri-apps/api/event';

export type ApiEventName =
  | 'scan:progress'
  | 'run:progress'
  | 'run:finished';

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

export async function subscribeToEvent(event: ApiEventName, handler: (payload: unknown) => void) {
  return listen(event, (message) => handler(message.payload));
}
