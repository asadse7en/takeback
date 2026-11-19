import { memo, useCallback, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  AlertTriangle, CheckCircle2, Database, Disc3, FileText, Image, Upload, Video, X,
} from 'lucide-react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import { Button, PageFrame, PageHeading } from '@/components/Ui';
import { useAddStore } from '@/store/add';
import type { TakeoutFile } from '@/services/addIndexing';

function formatSize(bytes: number) {
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

function progressPercent(file: TakeoutFile) {
  return file.itemsTotalKnown && file.itemsTotal
    ? Math.min(100, Math.floor((file.itemsRead / file.itemsTotal) * 100))
    : 0;
}

function indexedCount(files: TakeoutFile[]) {
  return files.filter((file) => file.status === 'indexed').length;
}

function footerMessage(files: TakeoutFile[]) {
  const indexed = indexedCount(files);
  const failed = files.filter((file) => file.status === 'failed').length;
  if (failed > 0) return `${indexed} of ${files.length} files indexed. ${failed} couldn't be read.`;
  const allFinished = files.every((file) => file.status === 'indexed' || file.status === 'failed');
  if (allFinished) return `${indexed} of ${files.length} files indexed.`;
  return `${indexed} of ${files.length} files indexed. You can continue while the rest finish.`;
}

function stateLabel(file: TakeoutFile) {
  if (file.status === 'indexed') return (
    <span className="add-file__status add-file__status--indexed">
      <CheckCircle2 size={16} aria-hidden="true" />Indexed
    </span>
  );
  if (file.status === 'indexing') return (
    <span className="add-file__status add-file__status--indexing">
      <span className="add-file__spinner" aria-hidden="true" />
      Indexing{file.itemsTotalKnown ? ` ${progressPercent(file)}%` : ''}
    </span>
  );
  if (file.status === 'failed') return null;
  return <span className="add-file__status add-file__status--waiting">Waiting</span>;
}

const FileRow = memo(function FileRow({ file }: { file: TakeoutFile }) {
  const removeFile = useAddStore((state) => state.removeFile);
  const retryFile = useAddStore((state) => state.retryFile);
  const percent = progressPercent(file);

  return (
    <div className={`add-file-row add-file-row--${file.status}`} role="listitem">
      {file.status === 'failed' ? (
        <AlertTriangle className="add-file__icon add-file__icon--warning" size={20} strokeWidth={1.5} aria-hidden="true" />
      ) : (
        <FileText
          className={`add-file__icon${file.status === 'indexing' ? ' add-file__icon--indexing' : ''}`}
          size={20}
          strokeWidth={1.5}
          aria-hidden="true"
        />
      )}
      <div className="add-file__details">
        <span className="add-file__name" title={file.name}>{file.name}</span>
        {file.status === 'indexing' ? (
          <>
            <div
              className={`add-file__progress${file.itemsTotalKnown ? '' : ' add-file__progress--indeterminate'}`}
              role="progressbar"
              aria-label={`Indexing ${file.name}`}
              aria-valuenow={file.itemsTotalKnown ? percent : undefined}
              aria-valuemin={0}
              aria-valuemax={100}
            >
              <span style={file.itemsTotalKnown ? { width: `${percent}%` } : undefined} />
            </div>
            <span className="add-file__detail">
              Reading {file.itemsRead.toLocaleString()}
              {file.itemsTotalKnown ? ` of ${file.itemsTotal.toLocaleString()}` : ''} items
              {file.currentPath ? ` · ${file.currentPath}` : ''}
            </span>
          </>
        ) : null}
        {file.status === 'failed' ? (
          <span className="add-file__detail add-file__detail--error">
            {file.error || "Couldn't finish reading this file. It may be incomplete, so download it again."}
          </span>
        ) : null}
      </div>
      <span className="add-file__size">{formatSize(file.sizeBytes)}</span>
      <span className="add-file__status-cell">
        {file.status === 'failed' ? (
          <Button className="add-file__retry" onClick={() => retryFile(file.id)}>Retry</Button>
        ) : stateLabel(file)}
      </span>
      <button
        className="icon-button add-file__remove"
        type="button"
        aria-label={`${file.status === 'indexing' ? 'Cancel indexing' : 'Remove'} ${file.name}`}
        onClick={() => removeFile(file.id)}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
});

export default function AddPage() {
  const navigate = useNavigate();
  const files = useAddStore((state) => state.files);
  const stats = useAddStore((state) => state.stats);
  const hasStartedIndexing = useAddStore((state) => state.hasStartedIndexing);
  const announcement = useAddStore((state) => state.announcement);
  const notice = useAddStore((state) => state.notice);
  const dragging = useAddStore((state) => state.dragging);
  const addFiles = useAddStore((state) => state.addFiles);
  const setDragging = useAddStore((state) => state.setDragging);
  const showNotice = useAddStore((state) => state.showNotice);
  const readyCount = indexedCount(files);

  const handleDrop = useCallback((paths: string[]) => {
    addFiles(paths);
  }, [addFiles]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        setDragging(true);
      } else if (event.payload.type === 'leave') {
        setDragging(false);
      } else if (event.payload.type === 'drop') {
        setDragging(false);
        handleDrop(event.payload.paths);
      }
    }).then((stopListening) => {
      if (disposed) stopListening();
      else unlisten = stopListening;
    }).catch((reason: unknown) => {
      showNotice(`Could not listen for dropped files: ${reason instanceof Error ? reason.message : String(reason)}`);
    });
    return () => {
      disposed = true;
      setDragging(false);
      unlisten?.();
    };
  }, [handleDrop, setDragging, showNotice]);

  const browse = async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: 'Takeout archives', extensions: ['zip', 'tgz', 'gz'] }],
      });
      if (selected) addFiles(Array.isArray(selected) ? selected : [selected]);
    } catch (reason) {
      showNotice(`Could not open files: ${reason instanceof Error ? reason.message : String(reason)}`);
    }
  };

  const statItems = [
    { label: 'Photos', value: stats.photos.toLocaleString(), Icon: Image },
    { label: 'Videos', value: stats.videos.toLocaleString(), Icon: Video },
    { label: 'Live photos', value: stats.livePhotos.toLocaleString(), Icon: Disc3 },
    { label: 'Read so far', value: formatSize(stats.bytesRead), Icon: Database },
  ];

  return (
    <PageFrame
      className="add-page-frame"
      title="Add your Takeout files"
      subtitle="Add every zip from your Google Photos export."
      footer={
        <>
          <span className="add-footer__message">{footerMessage(files)}</span>
          <Button variant="accent" disabled={readyCount === 0} onClick={() => navigate('/preferences')}>Next</Button>
        </>
      }
    >
      <div className="add-page-body">
        <button
          className={`drop-zone${files.length ? ' drop-zone--compact' : ''}${dragging ? ' drop-zone--dragging' : ''}`}
          type="button"
          aria-label="Browse for Takeout files"
          onClick={() => void browse()}
        >
          <Upload size={28} strokeWidth={1.5} aria-hidden="true" />
          <strong>{dragging ? 'Release to add' : 'Drop your Takeout here'}</strong>
          <span className="drop-zone__button">Browse</span>
        </button>
        {notice ? <p className="add-notice">{notice}</p> : null}

        <PageHeading>Added files ({files.length})</PageHeading>
        <div className={`add-file-list${files.length ? '' : ' add-file-list--empty'}`} role="list" aria-label="Added files">
          {files.map((file) => <FileRow file={file} key={file.id} />)}
        </div>

        {hasStartedIndexing && files.length > 0 ? (
          <>
            <PageHeading>Found so far</PageHeading>
            <section className="found-stats panel" aria-label="Indexing totals">
              {statItems.map(({ label, value, Icon }, index) => (
                <div className="found-stat" key={label}>
                  {index > 0 ? <span className="found-stat__divider" aria-hidden="true" /> : null}
                  <span className="found-stat__icon"><Icon size={20} strokeWidth={1.5} aria-hidden="true" /></span>
                  <span className="found-stat__copy">
                    <strong>{value}</strong>
                    <small>{label}</small>
                  </span>
                </div>
              ))}
            </section>
          </>
        ) : null}
      </div>
      <span className="visually-hidden" aria-live="polite" aria-atomic="true">{announcement}</span>
    </PageFrame>
  );
}
