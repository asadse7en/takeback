import { create } from 'zustand';
import type { RunState } from '@/types/app';
import type { ProcessingProgress } from '@/lib/api';

const initialRunState: RunState = {
  completed: 0,
  total: 0,
  percent: 0,
  status: 'Ready',
  currentFile: '',
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
  setCounts: (completed: number, total: number) => void;
  setStatus: (status: string) => void;
  claimStart: () => boolean;
  reset: () => void;
};

export const useRunStore = create<RunStore>((set) => ({
  run: initialRunState,
  claimed: false,
  setProgress: (progress) =>
    set((state) => ({
      run: {
        ...state.run,
        completed: progress.completedFiles,
        total: progress.totalFiles,
        percent: progress.totalFiles ? Math.round((progress.completedFiles / progress.totalFiles) * 100) : 0,
        status: 'Running',
        currentFile: progress.currentFile ?? state.run.currentFile,
        warnings: progress.warnings,
        processedBytes: progress.processedBytes,
        totalBytes: progress.totalBytes,
        bytesPerSecond: progress.bytesPerSecond,
        etaSeconds: progress.etaSeconds,
        warningsCount: progress.warningsCount,
        lastEventAt: Date.now(),
      },
    })),
  setCounts: (completed, total) =>
    set((state) => ({
      run: {
        ...state.run,
        completed,
        total,
        percent: total ? Math.round((completed / total) * 100) : 0,
      },
    })),
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
