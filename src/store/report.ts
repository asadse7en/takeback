import { create } from 'zustand';
import type { ReportDocument } from '@/lib/api';

type ReportStore = {
  report: ReportDocument | null;
  setReport: (report: ReportDocument | null) => void;
};

export const useReportStore = create<ReportStore>((set) => ({
  report: null,
  setReport: (report) => set({ report }),
}));
