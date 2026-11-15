import { create } from 'zustand';
import type { RunState } from '@/types/app';

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
};

type RunStore = {
  run: RunState;
  setProgress: (completed: number, total: number, status?: string, file?: string) => void;
  setStatus: (status: string) => void;
  reset: () => void;
};

export const useRunStore = create<RunStore>((set) => ({
  run: initialRunState,
  setProgress: (completed, total, status = 'Running', file = '') =>
    set((state) => {
      const now = Date.now();
      const completedDelta = Math.max(0, completed - state.run.completed);
      const elapsedSeconds = state.run.lastEventAt === null ? 0 : (now - state.run.lastEventAt) / 1000;
      const sample = elapsedSeconds > 0 ? completedDelta / elapsedSeconds : 0;
      const speedSamples = completedDelta > 0 ? [...state.run.speedSamples, sample].slice(-5) : state.run.speedSamples;
      const recentFiles = completedDelta > 0 && file
        ? [file, ...state.run.recentFiles.filter((recentFile) => recentFile !== file)].slice(0, 5)
        : state.run.recentFiles;
      const warnings = /warn|error|skip/i.test(status) && file
        ? [...state.run.warnings, `${file}: ${status}`]
        : state.run.warnings;
      return {
        run: {
          ...state.run,
          completed,
          total,
          percent: total ? Math.round((completed / total) * 100) : 0,
          status,
          currentFile: file || state.run.currentFile,
          speedSamples,
          recentFiles,
          warnings,
          lastEventAt: now,
        },
      };
    }),
  setStatus: (status) => set((state) => ({ run: { ...state.run, status } })),
  reset: () => set({ run: initialRunState }),
}));
