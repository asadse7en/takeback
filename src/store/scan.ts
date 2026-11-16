import { create } from 'zustand';
import type { ScanState } from '@/types/app';

const initialScanState: ScanState = {
  files: [],
  sources: [],
  counts: {
    photos: 0,
    videos: 0,
    live: 0,
    totalGb: 0,
  },
};

type ScanStore = {
  scan: ScanState;
  setFiles: (files: string[]) => void;
  setSources: (sources: ScanState['sources']) => void;
  setCounts: (counts: ScanState['counts']) => void;
  reset: () => void;
};

export const useScanStore = create<ScanStore>((set) => ({
  scan: initialScanState,
  setFiles: (files) => set((state) => ({ scan: { ...state.scan, files } })),
  setSources: (sources) => set((state) => ({ scan: { ...state.scan, sources } })),
  setCounts: (counts) => set((state) => ({ scan: { ...state.scan, counts } })),
  reset: () => set({ scan: initialScanState }),
}));
