import { useEffect, useMemo, useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { Disc3, File, FileText, Image, Trash2 } from 'lucide-react';
import { Button, PageFrame, Panel, PhotoThumb } from '@/components/Ui';
import { callCommand, type ReportFormat } from '@/lib/api';
import { useReportStore } from '@/store/report';

export default function ReportPage() {
  const report = useReportStore((state) => state.report);
  const setReport = useReportStore((state) => state.setReport);
  const [format, setFormat] = useState<ReportFormat>('csv');
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    if (!report) {
      callCommand('get_report', {}).then(setReport).catch((reason: unknown) => {
        setError(reason instanceof Error ? reason.message : String(reason));
      });
    }
  }, [report, setReport]);

  const rows = useMemo(() => {
    if (!report) return [];
    const settings = report.settings;
    return [
      ['Output folder', report.outputFolder],
      ['Albums', settings.albumMode],
      ['Date folders', settings.dateFolders],
      ['Favorites', settings.favoritesMode],
      ['Edited versions', settings.editedVersions],
      ['Live photo style', settings.liveStyle],
      ['Trashed items', settings.trashed],
      ['Time zone', settings.timeZone],
      ['Files without JSON', settings.noJson],
    ];
  }, [report]);
  const summary = report?.summary;
  const stats = [
    { value: (summary?.processed ?? 0).toLocaleString(), label: 'Files fixed', icon: <Image /> },
    { value: (summary?.otherFiles ?? 0).toLocaleString(), label: 'Other files', icon: <File /> },
    { value: (summary?.livePhotosMerged ?? 0).toLocaleString(), label: 'Live photos merged', icon: <Disc3 /> },
    { value: (summary?.filesWithoutJson ?? 0).toLocaleString(), label: 'No JSON found', icon: <FileText /> },
    { value: ((summary?.warnings.length ?? 0) + (summary?.problems.length ?? 0)).toLocaleString(), label: 'Warnings', icon: <Trash2 /> },
  ];

  const exportReport = async () => {
    if (!report) return;
    setError('');
    setMessage('');
    try {
      const destination = await save({
        defaultPath: `takeback-report.${format}`,
        filters: [{ name: `${format.toUpperCase()} report`, extensions: [format] }],
      });
      if (!destination) return;
      const result = await callCommand('export_report', { format, destination });
      setMessage(result.summary);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };
  const copySummary = async () => {
    if (!report) return;
    setError('');
    try {
      await navigator.clipboard.writeText(
        `Takeback run completed in ${Math.round(report.elapsedSeconds)} seconds.\n${report.summary.processed} processed, ${report.summary.skipped} skipped, ${report.summary.failed} failed.\n${report.summary.otherFiles} other files, ${report.summary.filesWithoutJson} files without JSON, ${report.summary.collisions} collisions, ${report.summary.warnings.length} warnings.\nOutput: ${report.outputFolder}`,
      );
      setMessage('Summary copied to clipboard.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  return (
    <PageFrame
      title="Export report"
      subtitle={report ? `${new Date(report.createdAt).toLocaleString()} · ${Math.round(report.elapsedSeconds / 60)} min · ${summary?.planned.toLocaleString()} files checked` : 'No completed run report is available.'}
      footer={
        <>
          <span className="footer-note">{message || (report ? `Ready to save as ${format.toUpperCase()}` : 'Run processing to create a report')}</span>
          <div className="footer-buttons">
            <label className="format-select">
              <span className="visually-hidden">Report format</span>
              <select value={format} onChange={(event) => setFormat(event.target.value as ReportFormat)}>
                <option value="csv">CSV</option><option value="json">JSON</option>
              </select>
            </label>
            <Button disabled={!report} onClick={() => void copySummary()}>Copy summary</Button>
            <Button variant="accent" disabled={!report} onClick={() => void exportReport()}>Save report</Button>
          </div>
        </>
      }
    >
      <div className="report-stats">
        {stats.map(({ value, label, icon }) => (
          <Panel className="report-stat" key={label}>
            <span className="report-stat__icon">{icon}</span>
            <span><strong>{value}</strong><small>{label}</small></span>
          </Panel>
        ))}
      </div>
      <div className="report-columns">
        <Panel className="report-panel">
          <h2 className="panel-title">Settings used</h2>
          <dl className="settings-list">
            {rows.map(([label, value]) => (
              <div className="settings-list__row" key={label}><dt>{label}</dt><dd>{value}</dd></div>
            ))}
          </dl>
        </Panel>
        <Panel className="report-panel report-panel--attention">
          <h2 className="panel-title">Needs attention</h2>
          <div className="attention-list">
            {(report?.items ?? []).map((item, index) => (
              <div className="attention-row" key={item.id}>
                <PhotoThumb tone={index % 2 ? 'blue' : 'peach'} />
                <strong>{item.file}</strong>
                <span className={item.group === 'failed' ? 'attention-row__issue is-danger' : 'attention-row__issue'}>{item.reason}</span>
                <span className="attention-row__result">{item.destination ?? item.group}</span>
              </div>
            ))}
            {!report?.items.length ? <p>No items need attention.</p> : null}
          </div>
        </Panel>
      </div>
      {error ? <p role="alert" className="form-error">{error}</p> : null}
    </PageFrame>
  );
}
