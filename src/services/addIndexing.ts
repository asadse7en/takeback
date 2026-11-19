export type TakeoutFile = {
  id: string;
  name: string;
  sizeBytes: number;
  status: 'waiting' | 'indexing' | 'indexed' | 'failed';
  itemsRead: number;
  itemsTotal: number;
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
};

export interface TakeoutIndexingService {
  addFiles(paths: string[]): AddFilesResult;
  removeFile(id: string): void;
  retryFile(id: string): void;
  subscribe(listener: (snapshot: IndexingSnapshot) => void): () => void;
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

function fileName(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

function mockedSizeBytes(name: string) {
  return 8_000_000_000 + hash(name.toLowerCase()) % 42_000_000_000;
}

function mockedItemCount(name: string) {
  return 20_000 + hash(name) % 80_000;
}

function fileCounts(file: TakeoutFile) {
  const photos = Math.round(file.itemsTotal * 0.86);
  const videos = Math.round(file.itemsTotal * 0.11);
  return {
    photos,
    videos,
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

  addFiles(paths: string[]) {
    let duplicates = 0;
    for (const path of paths) {
      const name = fileName(path);
      const sizeBytes = mockedSizeBytes(name);
      if (this.files.some((file) =>
        file.name.toLowerCase() === name.toLowerCase()
        && file.sizeBytes === sizeBytes
      )) {
        duplicates += 1;
        continue;
      }

      this.files.push({
        id: `takeout-${this.nextId++}`,
        name,
        sizeBytes,
        status: 'waiting',
        itemsRead: 0,
        itemsTotal: mockedItemCount(name),
        currentPath: '',
      });
    }

    this.startNext();
    this.publish();
    return { duplicates };
  }

  removeFile(id: string) {
    if (this.activeId === id) {
      this.stopTimer();
      this.activeId = null;
    }
    this.files = this.files.filter((file) => file.id !== id);
    this.startNext();
    this.publish();
  }

  retryFile(id: string) {
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
    const totals = { photos: 0, videos: 0, livePhotos: 0, bytesRead: 0 };
    for (const file of this.files) {
      if (file.status !== 'indexed' && file.status !== 'indexing') continue;
      const fraction = file.status === 'indexed'
        ? 1
        : file.itemsRead / Math.max(1, file.itemsTotal);
      const counts = fileCounts(file);
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

export const mockTakeoutIndexingService: TakeoutIndexingService =
  new MockTakeoutIndexingService();
