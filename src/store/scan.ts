import { create } from 'zustand';
import type { ScanState } from '@/types/app';

const initialScanState: ScanState = {
  files: [],
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
  setCounts: (counts: ScanState['counts']) => void;
  reset: () => void;
};

export const useScanStore = create<ScanStore>((set) => ({
  scan: initialScanState,
  setFiles: (files) => set((state) => ({ scan: { ...state.scan, files } })),
  setCounts: (counts) => set((state) => ({ scan: { ...state.scan, counts } })),
  reset: () => set({ scan: initialScanState }),
}));
