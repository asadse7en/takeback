import { create } from 'zustand';
import type { Settings } from '@/types/app';

const defaultSettings: Settings = {
  outputFolder: null,
  albumMode: 'move',
  albumLoc: 'top',
  multiAlbum: 'first',
  dateFolders: 'yearMonth',
  favoritesMode: 'link',
  editedMode: 'move',
  archiveFolder: true,
  trashed: 'skip',
  editedVersions: 'both',
  mergeLive: true,
  liveStyle: 'separate',
  splitByType: { photos: false, videos: false, live: false },
  fileNames: 'keep',
  customNamePattern: '',
  sameName: 'number',
  ignoreDuplicates: true,
  noJson: 'unsorted',
  timeZone: 'local',
  noDate: 'fileName',
  writeGps: true,
  writeDescriptions: true,
  rateFavorites: true,
  deleteArchivesAfter: false,
  speed: 'balanced',
  dryRun: false,
};

type SettingsStore = {
  settings: Settings;
  updateSettings: (patch: Partial<Settings>) => void;
  replaceSettings: (settings: Settings) => void;
  setOutputFolder: (path: string | null) => void;
  reset: () => void;
};

export const useSettingsStore = create<SettingsStore>((set) => ({
  settings: defaultSettings,
  updateSettings: (patch) =>
    set((state) => ({
      settings: { ...state.settings, ...patch },
    })),
  replaceSettings: (settings) => set({ settings }),
  setOutputFolder: (path) =>
    set((state) => ({
      settings: { ...state.settings, outputFolder: path },
    })),
  reset: () => set({ settings: defaultSettings }),
}));
