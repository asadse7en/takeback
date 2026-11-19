import { create } from 'zustand';
import {
  mockTakeoutIndexingService,
  type IndexStats,
  type TakeoutFile,
} from '@/services/addIndexing';

type AddStore = {
  files: TakeoutFile[];
  stats: IndexStats;
  hasStartedIndexing: boolean;
  announcement: string;
  notice: string;
  dragging: boolean;
  addFiles: (paths: string[]) => void;
  showNotice: (message: string) => void;
  removeFile: (id: string) => void;
  retryFile: (id: string) => void;
  setDragging: (dragging: boolean) => void;
};

let noticeTimer: ReturnType<typeof setTimeout> | null = null;

const useAddStore = create<AddStore>((set) => ({
  files: [],
  stats: { photos: 0, videos: 0, livePhotos: 0, bytesRead: 0 },
  hasStartedIndexing: false,
  announcement: '',
  notice: '',
  dragging: false,
  addFiles: (paths) => {
    const supported: string[] = [];
    let unsupportedCount = 0;
    for (const path of paths) {
      const lowerPath = path.toLowerCase();
      if (
        lowerPath.endsWith('.zip')
        || lowerPath.endsWith('.tgz')
        || lowerPath.endsWith('.tar.gz')
      ) {
        supported.push(path);
      } else {
        unsupportedCount += 1;
      }
    }

    const { duplicates } = mockTakeoutIndexingService.addFiles(supported);
    const messages: string[] = [];
    if (unsupportedCount > 0) messages.push('Only .zip and .tgz files are supported');
    if (duplicates > 0) messages.push('Already added');
    if (messages.length > 0) {
      if (noticeTimer !== null) clearTimeout(noticeTimer);
      set({ notice: messages.join(' · ') });
      noticeTimer = setTimeout(() => {
        set({ notice: '' });
        noticeTimer = null;
      }, 3_500);
    }
  },
  showNotice: (message) => {
    if (noticeTimer !== null) clearTimeout(noticeTimer);
    set({ notice: message });
    noticeTimer = setTimeout(() => {
      set({ notice: '' });
      noticeTimer = null;
    }, 3_500);
  },
  removeFile: (id) => mockTakeoutIndexingService.removeFile(id),
  retryFile: (id) => mockTakeoutIndexingService.retryFile(id),
  setDragging: (dragging) =>
    set((state) => state.dragging === dragging ? state : { dragging }),
}));

mockTakeoutIndexingService.subscribe((snapshot) => {
  useAddStore.setState({
    files: snapshot.files,
    stats: snapshot.stats,
    hasStartedIndexing: snapshot.hasStartedIndexing,
    announcement: snapshot.announcement,
  });
});

export { useAddStore };
