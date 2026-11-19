import { create } from 'zustand';
import {
  takeoutIndexingService,
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

const useAddStore = create<AddStore>((set) => {
  const showTransientNotice = (message: string) => {
    if (noticeTimer !== null) clearTimeout(noticeTimer);
    set({ notice: message });
    noticeTimer = setTimeout(() => {
      set({ notice: '' });
      noticeTimer = null;
    }, 3_500);
  };

  return {
    files: [],
    stats: { photos: 0, videos: 0, livePhotos: 0, bytesRead: 0 },
    hasStartedIndexing: false,
    announcement: '',
    notice: '',
    dragging: false,
    addFiles: (paths) => {
      const supported: string[] = [];
      const messages: string[] = [];
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

      if (unsupportedCount > 0) messages.push('Only .zip and .tgz files are supported');
      if (supported.length === 0) {
        if (messages.length > 0) showTransientNotice(messages.join(' · '));
        return;
      }

      void takeoutIndexingService.addFiles(supported)
        .then(({ duplicates, warnings }) => {
          if (duplicates > 0) messages.push('Already added');
          messages.push(...warnings);
          if (messages.length > 0) showTransientNotice(messages.join(' · '));
        })
        .catch((error: unknown) => {
          messages.push(`Could not add files: ${error instanceof Error ? error.message : String(error)}`);
          showTransientNotice(messages.join(' · '));
        });
    },
    showNotice: showTransientNotice,
    removeFile: (id) => {
      void takeoutIndexingService.removeFile(id).catch((error: unknown) => {
        showTransientNotice(`Could not remove source: ${error instanceof Error ? error.message : String(error)}`);
      });
    },
    retryFile: (id) => {
      void takeoutIndexingService.retryFile(id).catch((error: unknown) => {
        showTransientNotice(`Could not retry source: ${error instanceof Error ? error.message : String(error)}`);
      });
    },
    setDragging: (dragging) =>
      set((state) => state.dragging === dragging ? state : { dragging }),
  };
});

takeoutIndexingService.subscribe((snapshot) => {
  useAddStore.setState({
    files: snapshot.files,
    stats: snapshot.stats,
    hasStartedIndexing: snapshot.hasStartedIndexing,
    announcement: snapshot.announcement,
  });
});

export { useAddStore };
