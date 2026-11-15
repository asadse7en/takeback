export type PageKey = 'add' | 'preferences' | 'progress' | 'done' | 'report' | 'review';

export type Settings = {
  outputFolder: string | null;
  albumMode: 'off' | 'move' | 'link' | 'copy';
  albumLoc: 'top' | 'year';
  multiAlbum: 'first' | 'all';
  dateFolders: 'off' | 'year' | 'month';
  favoritesMode: 'off' | 'move' | 'link' | 'copy';
  editedMode: 'off' | 'move' | 'link' | 'copy';
  archiveFolder: boolean;
  trashed: 'skip' | 'include' | 'ownFolder';
  editedVersions: 'both' | 'originals' | 'edited';
  mergeLive: boolean;
  liveStyle: 'separate' | 'iphone' | 'android';
  splitByType: {
    photos: boolean;
    videos: boolean;
    live: boolean;
  };
  fileNames: 'keep' | 'dateTime' | 'dateOriginal' | 'custom';
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
};

export type ScanState = {
  files: string[];
  counts: {
    photos: number;
    videos: number;
    live: number;
    totalGb: number;
  };
};
