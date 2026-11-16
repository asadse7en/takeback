import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Database, Disc3, FileText, Image, Upload, Video, X } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { Button, PageFrame, PageHeading, Panel, StatusCheck } from '@/components/Ui';
import { callCommand, type SourceRecord } from '@/lib/api';
import { useScanStore } from '@/store/scan';

function formatBytes(bytes: number) {
  if (bytes < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

export default function AddPage() {
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const archives = useScanStore((state) => state.scan.sources);
  const scan = useScanStore((state) => state.scan);
  const setFiles = useScanStore((state) => state.setFiles);
  const setSources = useScanStore((state) => state.setSources);
  const setCounts = useScanStore((state) => state.setCounts);

  const addFiles = async (paths: string[]) => {
    if (!paths.length) return;
    setBusy(true);
    setError('');
    try {
      const result = await callCommand('add_sources', { paths });
      const updated = [...new Map([...archives, ...result.archives].map((archive) => [archive.id, archive])).values()];
      setSources(updated);
      const scanned = await callCommand('scan_sources', { paths: updated.map((archive) => archive.path) });
      setFiles(scanned.files);
      setCounts(scanned.counts);
      if (result.warnings.length || scanned.warnings.length) {
        setError([...result.warnings, ...scanned.warnings].join(' '));
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const browse = async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Takeout archives', extensions: ['zip', 'tgz', 'gz'] }],
    });
    const paths = selected ? (Array.isArray(selected) ? selected : [selected]) : [];
    await addFiles(paths);
  };

  const remove = async (archive: SourceRecord) => {
    setError('');
    try {
      await callCommand('remove_source', { sourceId: archive.id });
      const updated = archives.filter((item) => item.id !== archive.id);
      setSources(updated);
      if (updated.length) {
        const scanned = await callCommand('scan_sources', { paths: updated.map((item) => item.path) });
        setFiles(scanned.files);
        setCounts(scanned.counts);
      } else {
        setFiles([]);
        setCounts({ photos: 0, videos: 0, live: 0, totalGb: 0 });
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
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
      footer={<><span /><Button variant="accent" disabled={!archives.length || busy} onClick={() => navigate('/preferences')}>Next</Button></>}
    >
      <button className="drop-zone" type="button" aria-label="Browse for Takeout files" onClick={browse} disabled={busy}>
        <Upload size={30} strokeWidth={1.5} aria-hidden="true" />
        <strong>{busy ? 'Scanning your Takeout' : 'Drop your Takeout here'}</strong>
        <span className="drop-zone__button">{busy ? 'Please wait' : 'Browse'}</span>
      </button>

      <PageHeading>Added files ({archives.length})</PageHeading>
      <Panel className="archive-list">
        {archives.map((archive) => (
          <div className="archive-row" key={archive.id}>
            <FileText className="archive-row__file" size={20} strokeWidth={1.5} aria-hidden="true" />
            <span className="archive-row__name">{archive.path.split(/[\\/]/).pop() || archive.path}</span>
            <span className="archive-row__size">{formatBytes(archive.sizeBytes)}</span>
            <StatusCheck>{`${archive.entryCount.toLocaleString()} entries`}</StatusCheck>
            <button type="button" className="icon-button" aria-label={`Remove ${archive.path}`} onClick={() => void remove(archive)}>
              <X size={15} aria-hidden="true" />
            </button>
          </div>
        ))}
        {error ? <p role="alert" className="form-error">{error}</p> : null}
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
