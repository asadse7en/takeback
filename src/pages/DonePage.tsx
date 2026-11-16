import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { openPath } from '@tauri-apps/plugin-opener';
import { AlertTriangle, CalendarDays, Check, Disc3, FolderOpen, MapPin, RotateCcw } from 'lucide-react';
import { Button, PageFrame, Panel } from '@/components/Ui';
import { callCommand } from '@/lib/api';
import { useReportStore } from '@/store/report';

export default function DonePage() {
  const navigate = useNavigate();
  const report = useReportStore((state) => state.report);
  const setReport = useReportStore((state) => state.setReport);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!report) {
      callCommand('get_report', {}).then(setReport).catch((reason: unknown) => {
        setError(reason instanceof Error ? reason.message : String(reason));
      });
    }
  }, [report, setReport]);

  const summary = report?.summary;
  const checked = summary?.planned ?? 0;
  const fixed = summary?.processed ?? 0;
  const duplicates = summary?.collisions ?? 0;
  const noJson = summary?.filesWithoutJson ?? 0;
  const warnings = (summary?.warnings.length ?? 0) + (summary?.problems.length ?? 0);
  const attention = report?.items.length ?? 0;
  const completionStats = [
    { label: 'Fixed', count: fixed, color: 'fixed' },
    { label: 'Duplicates', count: duplicates, color: 'duplicates' },
    { label: 'No JSON', count: noJson, color: 'no-json' },
    { label: 'Warnings', count: warnings, color: 'warnings' },
  ] as const;
  const subtitle = report
    ? `${new Date(report.createdAt).toLocaleString()} · ${Math.round(report.elapsedSeconds / 60)} min`
    : 'Your processing summary will appear here.';

  return (
    <PageFrame
      title="All done"
      subtitle={subtitle}
      titleAdornment={<Check className="done-check" size={36} strokeWidth={1.5} aria-hidden="true" />}
      footer={
        <>
          <Button onClick={() => navigate('/add')}><RotateCcw size={14} />Start over</Button>
          <div className="footer-buttons">
            <Button onClick={() => navigate('/report')}>Export report</Button>
            <Button variant="accent" disabled={!report} onClick={() => {
              if (report) void openPath(report.outputFolder).catch((reason: unknown) => {
                setError(reason instanceof Error ? reason.message : String(reason));
              });
            }}><FolderOpen size={14} />Open output folder</Button>
          </div>
        </>
      }
    >
      <Panel className="completion-card">
        <div className="completion-count">
          <strong>{fixed.toLocaleString()}</strong>
          <span>of {checked.toLocaleString()} files fixed</span>
        </div>
        <div className="completion-bar" role="img" aria-label="Files fixed, duplicates, no JSON, and warnings">
          {completionStats.map((stat) => (
            <span key={stat.color} className={`completion-bar__${stat.color}`} style={{ width: `${checked ? stat.count / checked * 100 : 0}%` }} />
          ))}
        </div>
        <div className="completion-legend">
          {completionStats.map((stat) => (
            <span key={stat.color}>
              <i className={`legend-dot legend-dot--${stat.color}`} />
              <span>{stat.label}</span>
              <strong>{stat.count.toLocaleString()}</strong>
            </span>
          ))}
        </div>
      </Panel>

      <div className="summary-tiles">
        {[
          { value: (summary?.datesRestored ?? 0).toLocaleString(), label: 'Dates restored', icon: <CalendarDays /> },
          { value: (summary?.locationsAdded ?? 0).toLocaleString(), label: 'Locations added', icon: <MapPin /> },
          { value: (summary?.livePhotosMerged ?? 0).toLocaleString(), label: 'Live photos merged', icon: <Disc3 /> },
          { value: attention.toLocaleString(), label: 'Need a look', icon: <AlertTriangle />, action: <Button className="small-button" onClick={() => navigate('/review')}>Review</Button> },
        ].map(({ value, label, icon, action }) => (
          <Panel className="summary-tile" key={label}>
            <span className="summary-tile__icon">{icon}</span>
            <strong>{value}</strong>
            <span>{label}</span>
            {action}
          </Panel>
        ))}
      </div>
      {error ? <p role="alert" className="form-error">{error}</p> : null}
    </PageFrame>
  );
}
