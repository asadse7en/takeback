import { create } from 'zustand';
import type { RunState } from '@/types/app';
import type { ProcessingProgress } from '@/lib/api';

const initialRunState: RunState = {
  completed: 0,
  total: 0,
  percent: 0,
  status: 'Ready',
  currentFile: '',
  speedSamples: [],
  recentFiles: [],
  warnings: [],
  lastEventAt: null,
  processedBytes: 0,
  totalBytes: 0,
  bytesPerSecond: 0,
  etaSeconds: null,
  warningsCount: 0,
};

type RunStore = {
  run: RunState;
  claimed: boolean;
  setProgress: (progress: ProcessingProgress) => void;
  setStatus: (status: string) => void;
  claimStart: () => boolean;
  reset: () => void;
};

export const useRunStore = create<RunStore>((set) => ({
  run: initialRunState,
  claimed: false,
  setProgress: (progress) =>
    set((state) => {
      const currentFile = progress.currentFile ?? state.run.currentFile;
      const recentFiles = progress.currentFile
        ? [progress.currentFile, ...state.run.recentFiles.filter((file) => file !== progress.currentFile)].slice(0, 5)
        : state.run.recentFiles;
      return {
        run: {
          ...state.run,
          completed: progress.completedFiles,
          total: progress.totalFiles,
          percent: progress.totalFiles ? Math.round((progress.completedFiles / progress.totalFiles) * 100) : 0,
          status: 'Running',
          currentFile,
          recentFiles,
          warnings: progress.warnings,
          processedBytes: progress.processedBytes,
          totalBytes: progress.totalBytes,
          bytesPerSecond: progress.bytesPerSecond,
          etaSeconds: progress.etaSeconds,
          warningsCount: progress.warningsCount,
          lastEventAt: Date.now(),
        },
      };
    }),
  setStatus: (status) => set((state) => ({ run: { ...state.run, status } })),
  claimStart: () => {
    let claimed = false;
    set((state) => {
      if (state.claimed) return state;
      claimed = true;
      return { ...state, claimed: true };
    });
    return claimed;
  },
  reset: () => set({ run: initialRunState, claimed: false }),
}));
