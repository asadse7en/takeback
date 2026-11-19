import { create } from 'zustand';
import type { Settings } from '@/types/app';

type SettingsStore = {
  settings: Settings | null;
  updateSettings: (patch: Partial<Settings>) => void;
  replaceSettings: (settings: Settings) => void;
  setOutputFolder: (path: string | null) => void;
};

export const useSettingsStore = create<SettingsStore>((set) => ({
  settings: null,
  updateSettings: (patch) =>
    set((state) => state.settings
      ? { settings: { ...state.settings, ...patch } }
      : state),
  replaceSettings: (settings) => set({ settings }),
  setOutputFolder: (path) =>
    set((state) => state.settings
      ? { settings: { ...state.settings, outputFolder: path } }
      : state),
}));
