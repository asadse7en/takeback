export type PageKey = 'add' | 'preferences' | 'progress' | 'done' | 'report' | 'review';

export type Settings = {
  outputFolder: string | null;
  albumMode: 'off' | 'move' | 'link' | 'copy';
  albumLoc: 'top' | 'year';
  multiAlbum: 'first' | 'all';
  dateFolders: 'off' | 'year' | 'yearMonth';
  favoritesMode: 'off' | 'move' | 'link' | 'copy';
  editedMode: 'off' | 'move' | 'link' | 'copy';
  archiveFolder: boolean;
  trashed: 'skip' | 'include' | 'ownFolder';
  editedVersions: 'both' | 'originals' | 'edited';
  mergeLive: boolean;
  liveStyle: 'separate' | 'iphonePair' | 'androidMotionPhoto';
  splitByType: {
    photos: boolean;
    videos: boolean;
    live: boolean;
  };
  fileNames: 'keep' | 'dateTime' | 'dateOriginal' | 'custom';
  customNamePattern: string;
  sameName: 'number' | 'dateTime' | 'skip';
  ignoreDuplicates: boolean;
  noJson: 'leave' | 'unsorted';
  timeZone: 'local' | 'computer' | 'utc';
  noDate: 'fileName' | 'fileDate' | 'blank';
  writeGps: boolean;
  writeDescriptions: boolean;
  rateFavorites: boolean;
  deleteArchivesAfter: boolean;
  speed: 'balanced' | 'gentle';
  dryRun: boolean;
};

export const FREE_SPACE_CHECK_ENABLED = true as const;

export type RunState = {
  completed: number;
  total: number;
  percent: number;
  status: string;
  currentFile: string;
  speedSamples: number[];
  recentFiles: string[];
  warnings: string[];
  lastEventAt: number | null;
  processedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  etaSeconds: number | null;
  warningsCount: number;
};

export type ScanState = {
  files: string[];
  sources: {
    id: string;
    path: string;
    archiveType: 'zip' | 'tgz' | 'folder';
    sizeBytes: number;
    entryCount: number;
    entryCountKnown: boolean;
  }[];
  counts: {
    photos: number;
    videos: number;
    live: number;
    totalGb: number;
  };
};
