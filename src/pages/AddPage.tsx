import { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Database, Disc3, FileText, Image, Upload, Video, X } from 'lucide-react';
import { Channel } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import { Button, PageFrame, PageHeading, Panel, StatusCheck } from '@/components/Ui';
import { callCommand, type ScanProgress, type ScanSourcesResult, type SourceRecord } from '@/lib/api';
import { useScanStore } from '@/store/scan';

function startScan(paths: string[], onProgress: (progress: ScanProgress) => void) {
  const channel = new Channel<ScanProgress>();
  return new Promise<ScanSourcesResult>((resolve, reject) => {
    let settled = false;
    channel.onmessage = (progress) => {
      if (settled) return;
      onProgress(progress);
      if (progress.stage === 'done') {
        settled = true;
        if (progress.result) resolve(progress.result);
        else reject(new Error('Scan completed without a result.'));
      } else if (progress.stage === 'error') {
        settled = true;
        reject(new Error(progress.error || 'Could not scan Takeout sources.'));
      } else if (progress.stage === 'cancelled') {
        settled = true;
        reject(new Error('Scan cancelled.'));
      }
    };
    void callCommand('start_scan', { paths, onEvent: channel }).catch((reason: unknown) => {
      if (settled) return;
      settled = true;
      reject(reason);
    });
  });
}

function formatBytes(bytes: number) {
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

function scanErrorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

export default function AddPage() {
  const navigate = useNavigate();
  const [scanning, setScanning] = useState(false);
  const [pendingSourceIds, setPendingSourceIds] = useState<string[]>([]);
  const [error, setError] = useState('');
  const [warnings, setWarnings] = useState<string[]>([]);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const pendingIds = useRef(new Set<string>());
  const scanLoopRunning = useRef(false);
  const archives = useScanStore((state) => state.scan.sources);
  const scan = useScanStore((state) => state.scan);
  const setFiles = useScanStore((state) => state.setFiles);
  const setSources = useScanStore((state) => state.setSources);
  const setCounts = useScanStore((state) => state.setCounts);

  const updatePendingIds = useCallback((next: Set<string>) => {
    pendingIds.current = next;
    setPendingSourceIds(Array.from(next));
  }, []);

  const runScanQueue = useCallback(async () => {
    if (scanLoopRunning.current || pendingIds.current.size === 0) return;
    scanLoopRunning.current = true;
    setScanning(true);
    setError('');
    let completed = false;
    try {
      while (pendingIds.current.size > 0) {
        const batchIds = new Set(pendingIds.current);
        const sources = useScanStore.getState().scan.sources;
        const paths = sources.filter((source) => batchIds.has(source.id)).map((source) => source.path);
        if (paths.length === 0) {
          updatePendingIds(new Set(Array.from(pendingIds.current).filter((id) => !batchIds.has(id))));
          continue;
        }

        const result = await startScan(paths, setProgress);
        setFiles(result.files);
        setCounts(result.counts);
        setWarnings(result.warnings);
        updatePendingIds(new Set(Array.from(pendingIds.current).filter((id) => !batchIds.has(id))));
      }
      completed = true;
    } catch (reason) {
      if (scanErrorMessage(reason) === 'Scan cancelled.') {
        setWarnings(['Scan cancelled. Added files are still available to index.']);
      } else {
        setError(scanErrorMessage(reason));
      }
    } finally {
      scanLoopRunning.current = false;
      setScanning(false);
      setProgress(null);
      if (completed && pendingIds.current.size > 0) void runScanQueue();
    }
  }, [setCounts, setFiles, updatePendingIds]);

  const addFiles = useCallback(async (paths: string[]) => {
    if (!paths.length) return;
    setError('');
    try {
      const result = await callCommand('add_sources', { paths });
      const currentSources = useScanStore.getState().scan.sources;
      const currentIds = new Set(currentSources.map((source) => source.id));
      const newSources = result.archives.filter((source) => !currentIds.has(source.id));
      const updated = [...new Map([...currentSources, ...result.archives].map((archive) => [archive.id, archive])).values()];
      setSources(updated);
      setWarnings(result.warnings);
      if (newSources.length > 0) {
        updatePendingIds(new Set([
          ...pendingIds.current,
          ...newSources.map((source) => source.id),
        ]));
        void runScanQueue();
      }
    } catch (reason) {
      setError(scanErrorMessage(reason));
    }
  }, [runScanQueue, setSources, updatePendingIds]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'drop') void addFiles(event.payload.paths);
    }).then((stopListening) => {
      if (disposed) stopListening();
      else unlisten = stopListening;
    }).catch((reason: unknown) => {
      setError(scanErrorMessage(reason));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [addFiles]);

  const browse = async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Takeout archives', extensions: ['zip', 'tgz', 'gz'] }],
    });
    const paths = selected ? (Array.isArray(selected) ? selected : [selected]) : [];
    await addFiles(paths);
  };

  const addFolder = async () => {
    const selected = await open({ directory: true });
    const paths = selected ? (Array.isArray(selected) ? selected : [selected]) : [];
    await addFiles(paths);
  };

  const cancelScan = async () => {
    try {
      await callCommand('cancel_scan', {});
    } catch (reason) {
      setError(scanErrorMessage(reason));
    }
  };

  const remove = async (archive: SourceRecord) => {
    setError('');
    try {
      const result = await callCommand('remove_source', { sourceId: archive.id });
      const updated = archives.filter((item) => item.id !== archive.id);
      setSources(updated);
      setFiles(result.files);
      setCounts(result.counts);
      setWarnings(result.warnings);
      updatePendingIds(new Set(Array.from(pendingIds.current).filter((id) => id !== archive.id)));
    } catch (reason) {
      setError(scanErrorMessage(reason));
    }
  };

  const stats = [
    { label: 'Photos', value: scan.counts.photos.toLocaleString(), Icon: Image },
    { label: 'Videos', value: scan.counts.videos.toLocaleString(), Icon: Video },
    { label: 'Live photos', value: scan.counts.live.toLocaleString(), Icon: Disc3 },
    { label: 'Total', value: `${scan.counts.totalGb.toFixed(1)} GB`, Icon: Database },
  ];

  return (
    <PageFrame
      title="Add your Takeout files"
      subtitle="Add every zip from your Google Photos export."
      footer={<><span /><Button variant="accent" disabled={!archives.length || scanning || pendingSourceIds.length > 0} onClick={() => navigate('/preferences')}>Next</Button></>}
    >
      <div className="add-sources">
        <button className="drop-zone" type="button" aria-label="Browse for Takeout files" onClick={() => void browse()}>
          <Upload size={30} strokeWidth={1.5} aria-hidden="true" />
          <strong>Drop your Takeout here</strong>
          <span className="drop-zone__button">Browse</span>
        </button>
        <Button className="add-folder-button" onClick={() => void addFolder()}>Add folder</Button>
      </div>

      {scanning ? (
        <div className="scan-progress" role="status">
          <div className="scan-progress__summary">
            <span>{progress?.currentArchive.split(/[\\/]/).pop() || 'Preparing to index added files'}</span>
            <span>{progress ? `${progress.entriesSeen.toLocaleString()} entries indexed` : 'Starting scan'}</span>
          </div>
          <div className="scan-progress__track" role="progressbar" aria-label="Indexing Takeout files">
            <span />
          </div>
          <Button onClick={() => void cancelScan()}>Cancel</Button>
        </div>
      ) : null}

      <PageHeading>Added files ({archives.length})</PageHeading>
      <Panel className="archive-list">
        {archives.map((archive) => {
          const isPending = pendingSourceIds.includes(archive.id);
          return (
            <div className="archive-row" key={archive.id}>
              <FileText className="archive-row__file" size={20} strokeWidth={1.5} aria-hidden="true" />
              <span className="archive-row__name">{archive.path.split(/[\\/]/).pop() || archive.path}</span>
              <span className="archive-row__size">{formatBytes(archive.sizeBytes)}</span>
              {isPending ? (
                <span className="archive-row__pending">Added, indexing in background</span>
              ) : (
                <StatusCheck>
                  {archive.entryCountKnown
                    ? `${archive.entryCount.toLocaleString()} entries`
                    : 'Entry count unknown'}
                </StatusCheck>
              )}
              <button type="button" className="icon-button" aria-label={`Remove ${archive.path}`} onClick={() => void remove(archive)} disabled={scanning}>
                <X size={15} aria-hidden="true" />
              </button>
            </div>
          );
        })}
        {error ? <p role="alert" className="form-error">{error}</p> : null}
        {warnings.length ? <p className="info-message" role="status">{warnings.join(' ')}</p> : null}
        {pendingSourceIds.length > 0 && !scanning ? (
          <div className="scan-retry">
            <span>Some files still need indexing.</span>
            <Button onClick={() => void runScanQueue()}>Retry indexing</Button>
          </div>
        ) : null}
      </Panel>

      <Panel className="archive-stats">
        {stats.map(({ label, value, Icon }, index) => (
          <div className="archive-stat" key={label}>
            {index > 0 ? <span className="archive-stat__divider" aria-hidden="true" /> : null}
            <span className="archive-stat__icon"><Icon size={20} strokeWidth={1.5} aria-hidden="true" /></span>
            <span><strong>{value}</strong><small>{label}</small></span>
          </div>
        ))}
      </Panel>
    </PageFrame>
  );
}
