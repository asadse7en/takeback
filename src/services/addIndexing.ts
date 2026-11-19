import { Channel } from '@tauri-apps/api/core';
import { callCommand, type ScanProgress } from '@/lib/api';

declare global {
  interface ImportMetaEnv {
    readonly VITE_MOCK_INDEXING?: string;
  }
  interface ImportMeta {
    readonly env: ImportMetaEnv;
  }
}

export type TakeoutFile = {
  id: string;
  path: string;
  name: string;
  sizeBytes: number;
  status: 'waiting' | 'indexing' | 'indexed' | 'failed';
  itemsRead: number;
  itemsTotal: number;
  itemsTotalKnown: boolean;
  currentPath: string;
  error?: string;
};

export type IndexStats = {
  photos: number;
  videos: number;
  livePhotos: number;
  bytesRead: number;
};

export type IndexingSnapshot = {
  files: TakeoutFile[];
  stats: IndexStats;
  hasStartedIndexing: boolean;
  announcement: string;
};

export type AddFilesResult = {
  duplicates: number;
  warnings: string[];
};

export interface TakeoutIndexingService {
  addFiles(paths: string[]): Promise<AddFilesResult>;
  removeFile(id: string): Promise<void>;
  retryFile(id: string): Promise<void>;
  subscribe(listener: (snapshot: IndexingSnapshot) => void): () => void;
}

const emptyStats = (): IndexStats => ({
  photos: 0,
  videos: 0,
  livePhotos: 0,
  bytesRead: 0,
});

function normalizePath(path: string) {
  const slashed = path
    .replace(/\\/g, '/')
    .replace(/^\/\/\?\/UNC\//i, '//')
    .replace(/^\/\/\?\//, '');
  const root = slashed.startsWith('//') ? '//' : slashed.startsWith('/') ? '/' : '';
  const segments: string[] = [];
  for (const segment of slashed.slice(root.length).split('/')) {
    if (!segment || segment === '.') continue;
    if (segment === '..') {
      const last = segments[segments.length - 1];
      if (last && last !== '..') segments.pop();
      else if (!root) segments.push(segment);
    } else {
      segments.push(segment);
    }
  }
  return `${root}${segments.join('/')}`.toLowerCase();
}

function fileName(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function countStats(progress: ScanProgress): IndexStats | null {
  const counts = progress.result?.counts;
  if (!counts) return null;
  return {
    photos: counts.photos,
    videos: counts.videos,
    livePhotos: counts.live,
    bytesRead: Math.round(counts.totalGb * 1_000_000_000),
  };
}

export class TauriTakeoutIndexingService implements TakeoutIndexingService {
  private files: TakeoutFile[] = [];
  private listeners = new Set<(snapshot: IndexingSnapshot) => void>();
  private stats = emptyStats();
  private activeId: string | null = null;
  private activePhase: 'scanning' | 'cleaning' | null = null;
  private activeStart: Promise<string> | null = null;
  private addSourcesQueue: Promise<void> = Promise.resolve();
  private pendingPaths = new Set<string>();
  private pathKeysById = new Map<string, Set<string>>();
  private removeAfterCleanup = new Set<string>();
  private hasStartedIndexing = false;
  private announcement = '';

  async addFiles(paths: string[]): Promise<AddFilesResult> {
    const knownPaths = new Set(this.pendingPaths);
    for (const keys of this.pathKeysById.values()) {
      for (const key of keys) knownPaths.add(key);
    }

    const uniquePaths: string[] = [];
    let duplicates = 0;
    for (const path of paths) {
      const key = normalizePath(path);
      if (knownPaths.has(key)) {
        duplicates += 1;
        continue;
      }
      knownPaths.add(key);
      this.pendingPaths.add(key);
      uniquePaths.push(path);
    }
    if (uniquePaths.length === 0) return { duplicates, warnings: [] };

    const request = this.addSourcesQueue.then(() =>
      this.inspectAndQueue(uniquePaths, duplicates)
    );
    this.addSourcesQueue = request.then(() => undefined, () => undefined);
    try {
      return await request;
    } finally {
      for (const path of uniquePaths) this.pendingPaths.delete(normalizePath(path));
    }
  }

  private async inspectAndQueue(paths: string[], duplicates: number): Promise<AddFilesResult> {
    const result = await callCommand('add_sources', { paths });
    const fileKeys = new Set(this.pathKeysById.keys());
    const existingPaths = new Set<string>();
    for (const keys of this.pathKeysById.values()) {
      for (const key of keys) existingPaths.add(key);
    }

    for (const source of result.archives) {
      const sourceKey = normalizePath(source.path);
      if (fileKeys.has(source.id) || existingPaths.has(sourceKey)) {
        duplicates += 1;
        continue;
      }

      const keys = new Set([sourceKey]);
      const inputMatch = paths.find((path) => normalizePath(path) === sourceKey);
      if (inputMatch) keys.add(normalizePath(inputMatch));
      this.pathKeysById.set(source.id, keys);
      for (const key of keys) existingPaths.add(key);
      fileKeys.add(source.id);
      this.files.push({
        id: source.id,
        path: source.path,
        name: fileName(source.path),
        sizeBytes: source.sizeBytes,
        status: 'waiting',
        itemsRead: 0,
        itemsTotal: source.entryCount,
        itemsTotalKnown: source.entryCountKnown,
        currentPath: '',
      });
    }

    this.startNext();
    this.publish();
    return { duplicates, warnings: result.warnings };
  }

  async removeFile(id: string): Promise<void> {
    const file = this.files.find((item) => item.id === id);
    if (!file) return;

    if (this.activeId === id && this.activePhase === 'scanning') {
      const start = this.activeStart;
      if (start) {
        try {
          await start;
        } catch {
          return;
        }
      }
      if (this.activeId === id && this.activePhase === 'scanning') {
        await callCommand('cancel_scan', {});
      }
      return;
    }

    if (this.activeId === id && this.activePhase === 'cleaning') {
      this.removeAfterCleanup.add(id);
      this.pathKeysById.delete(id);
      this.files = this.files.filter((item) => item.id !== id);
      this.publish();
      return;
    }

    await callCommand('remove_source', { sourceId: file.id });
    this.pathKeysById.delete(id);
    this.files = this.files.filter((item) => item.id !== id);
    this.publish();
  }

  async retryFile(id: string): Promise<void> {
    const file = this.files.find((item) => item.id === id);
    if (!file || file.status !== 'failed') return;

    this.announcement = '';
    this.files = [
      ...this.files.filter((item) => item.id !== id),
      { ...file, status: 'waiting', itemsRead: 0, currentPath: '', error: undefined },
    ];
    this.startNext();
    this.publish();
  }

  subscribe(listener: (snapshot: IndexingSnapshot) => void) {
    this.listeners.add(listener);
    listener(this.snapshot());
    return () => this.listeners.delete(listener);
  }

  private startNext() {
    if (this.activeId) return;
    const file = this.files.find((item) => item.status === 'waiting');
    if (!file) return;

    this.activeId = file.id;
    this.activePhase = 'scanning';
    this.hasStartedIndexing = true;
    this.files = this.files.map((item) =>
      item.id === file.id ? { ...item, status: 'indexing', error: undefined } : item
    );

    const channel = new Channel<ScanProgress>((progress) => this.onProgress(file.id, progress));
    const start = callCommand('start_scan', { paths: [file.path], onEvent: channel });
    this.activeStart = start;
    this.publish();
    void start.catch((error: unknown) => this.failActive(file.id, errorMessage(error)));
  }

  private onProgress(id: string, progress: ScanProgress) {
    if (this.activeId !== id) return;
    this.files = this.files.map((file) =>
      file.id === id
        ? { ...file, itemsRead: progress.entriesSeen, currentPath: progress.currentArchive }
        : file
    );
    if (progress.stage === 'preparing' || progress.stage === 'scanning') {
      this.publish();
      return;
    }

    if (progress.stage === 'done') {
      const nextStats = countStats(progress);
      const file = this.files.find((item) => item.id === id);
      if (nextStats) this.stats = nextStats;
      this.files = this.files.map((item) =>
        item.id === id
          ? { ...item, status: 'indexed', itemsRead: progress.entriesSeen, currentPath: '', error: undefined }
          : item
      );
      if (file) {
        this.announcement = nextStats
          ? `Indexed: ${file.name}`
          : `Indexed: ${file.name}. Scan counts were not returned.`;
      }
      setTimeout(() => this.finishActive(id), 0);
      return;
    }

    this.failActive(
      id,
      progress.stage === 'cancelled'
        ? 'Scan cancelled.'
        : progress.error || 'The scan failed.'
    );
  }

  private failActive(id: string, message: string) {
    if (this.activeId !== id || this.activePhase !== 'scanning') return;
    const file = this.files.find((item) => item.id === id);
    if (!file) {
      this.finishActive(id);
      return;
    }

    this.activePhase = 'cleaning';
    this.files = this.files.map((item) =>
      item.id === id ? { ...item, status: 'failed', error: message } : item
    );
    this.announcement = `Failed: ${file.name}`;
    this.publish();

    void callCommand('remove_source', { sourceId: file.id })
      .catch((error: unknown) => {
        const cleanupMessage = `Could not clear the partial index: ${errorMessage(error)}`;
        this.files = this.files.map((item) =>
          item.id === id && item.status === 'failed'
            ? { ...item, error: `${item.error || message} ${cleanupMessage}` }
            : item
        );
      })
      .then(() => {
        if (this.removeAfterCleanup.delete(id)) {
          this.files = this.files.filter((item) => item.id !== id);
        }
        this.finishActive(id);
      });
  }

  private finishActive(id: string) {
    if (this.activeId !== id) return;
    this.activeId = null;
    this.activePhase = null;
    this.activeStart = null;
    this.startNext();
    this.publish();
  }

  private snapshot(): IndexingSnapshot {
    return {
      files: [...this.files],
      stats: this.stats,
      hasStartedIndexing: this.hasStartedIndexing,
      announcement: this.announcement,
    };
  }

  private publish() {
    const snapshot = this.snapshot();
    for (const listener of this.listeners) listener(snapshot);
  }
}

const samplePaths = [
  'Google Photos/Photos from 2024/IMG_4821.HEIC',
  'Google Photos/Trip to Lisbon/IMG_4822.JPG',
  'Google Photos/Favorites/VID_1048.MP4',
  'Google Photos/Photos from 2023/IMG_0194.JPG',
  'Google Photos/Family/IMG_2027.HEIC',
  'Google Photos/Photos from 2022/VID_5102.MOV',
];

const failedFileMessage =
  "Couldn't finish reading this file. It may be incomplete, so download it again.";

function hash(value: string) {
  let result = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    result = Math.imul(result ^ value.charCodeAt(index), 16777619);
  }
  return result >>> 0;
}

function mockedSizeBytes(name: string) {
  return 8_000_000_000 + hash(name.toLowerCase()) % 42_000_000_000;
}

function mockedItemCount(name: string) {
  return 20_000 + hash(name) % 80_000;
}

function mockFileCounts(file: TakeoutFile) {
  return {
    photos: Math.round(file.itemsTotal * 0.86),
    videos: Math.round(file.itemsTotal * 0.11),
    livePhotos: Math.round(file.itemsTotal * 0.03),
  };
}

class MockTakeoutIndexingService implements TakeoutIndexingService {
  private files: TakeoutFile[] = [];
  private listeners = new Set<(snapshot: IndexingSnapshot) => void>();
  private activeId: string | null = null;
  private activeSince = 0;
  private activeDuration = 0;
  private timer: ReturnType<typeof setInterval> | null = null;
  private nextId = 1;
  private hasStartedIndexing = false;
  private announcement = '';

  async addFiles(paths: string[]): Promise<AddFilesResult> {
    let duplicates = 0;
    const knownPaths = new Set(this.files.map((file) => normalizePath(file.path)));
    for (const path of paths) {
      const key = normalizePath(path);
      if (knownPaths.has(key)) {
        duplicates += 1;
        continue;
      }
      knownPaths.add(key);

      const name = fileName(path);
      const sizeBytes = mockedSizeBytes(name);
      this.files.push({
        id: `takeout-${this.nextId++}`,
        path,
        name,
        sizeBytes,
        status: 'waiting',
        itemsRead: 0,
        itemsTotal: mockedItemCount(name),
        itemsTotalKnown: !name.toLowerCase().endsWith('.tgz')
          && !name.toLowerCase().endsWith('.tar.gz'),
        currentPath: '',
      });
    }

    this.startNext();
    this.publish();
    return { duplicates, warnings: [] };
  }

  async removeFile(id: string): Promise<void> {
    if (this.activeId === id) {
      this.stopTimer();
      this.activeId = null;
    }
    this.files = this.files.filter((file) => file.id !== id);
    this.startNext();
    this.publish();
  }

  async retryFile(id: string): Promise<void> {
    const file = this.files.find((item) => item.id === id);
    if (!file || file.status !== 'failed') return;

    this.announcement = '';
    this.files = this.files.filter((item) => item.id !== id);
    this.files.push({
      ...file,
      status: 'waiting',
      itemsRead: 0,
      currentPath: '',
      error: undefined,
    });
    this.startNext();
    this.publish();
  }

  subscribe(listener: (snapshot: IndexingSnapshot) => void) {
    this.listeners.add(listener);
    listener(this.snapshot());
    return () => this.listeners.delete(listener);
  }

  private snapshot(): IndexingSnapshot {
    return {
      files: [...this.files],
      stats: this.calculateStats(),
      hasStartedIndexing: this.hasStartedIndexing,
      announcement: this.announcement,
    };
  }

  private calculateStats(): IndexStats {
    const totals = emptyStats();
    for (const file of this.files) {
      if (file.status !== 'indexed' && file.status !== 'indexing') continue;
      const fraction = file.status === 'indexed'
        ? 1
        : file.itemsRead / Math.max(1, file.itemsTotal);
      const counts = mockFileCounts(file);
      totals.photos += Math.floor(counts.photos * fraction);
      totals.videos += Math.floor(counts.videos * fraction);
      totals.livePhotos += Math.floor(counts.livePhotos * fraction);
      totals.bytesRead += Math.floor(file.sizeBytes * fraction);
    }
    return totals;
  }

  private startNext() {
    if (this.activeId) return;
    const file = this.files.find((item) => item.status === 'waiting');
    if (!file) return;

    this.activeId = file.id;
    this.activeSince = Date.now();
    this.activeDuration = 4_000 + (hash(file.name) % 5) * 1_000;
    this.hasStartedIndexing = true;
    this.files = this.files.map((item) =>
      item.id === file.id ? { ...item, status: 'indexing' } : item
    );
    this.timer = setInterval(() => this.tick(), 100);
  }

  private tick() {
    const file = this.files.find((item) => item.id === this.activeId);
    if (!file) {
      this.stopTimer();
      this.activeId = null;
      this.startNext();
      this.publish();
      return;
    }

    const fraction = Math.min(1, (Date.now() - this.activeSince) / this.activeDuration);
    const pathIndex = Math.floor(fraction * samplePaths.length * 2) % samplePaths.length;
    const itemsRead = Math.floor(file.itemsTotal * fraction);
    if (fraction < 1) {
      this.files = this.files.map((item) =>
        item.id === file.id
          ? { ...item, itemsRead, currentPath: samplePaths[pathIndex] }
          : item
      );
      this.publish();
      return;
    }

    const failed = file.name.toLowerCase().endsWith('-004.tgz');
    this.files = this.files.map((item) =>
      item.id === file.id
        ? failed
          ? { ...item, status: 'failed', itemsRead: 0, error: failedFileMessage }
          : { ...item, status: 'indexed', itemsRead: item.itemsTotal, currentPath: '' }
        : item
    );
    this.announcement = failed
      ? `Failed: ${file.name}`
      : `Indexed: ${file.name}`;
    this.stopTimer();
    this.activeId = null;
    this.startNext();
    this.publish();
  }

  private stopTimer() {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  private publish() {
    const snapshot = this.snapshot();
    for (const listener of this.listeners) listener(snapshot);
  }
}

export const takeoutIndexingService: TakeoutIndexingService =
  import.meta.env.VITE_MOCK_INDEXING
    ? new MockTakeoutIndexingService()
    : new TauriTakeoutIndexingService();
