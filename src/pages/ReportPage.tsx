import { useState } from 'react';
import { Disc3, FileText, Image, Trash2 } from 'lucide-react';
import { Button, PageFrame, Panel, PhotoThumb } from '@/components/Ui';
import { mockCompletion, mockReportItems, mockReportRows } from '@/mock/data';

const reportStats = [
  { value: mockCompletion.filesFixed.toLocaleString(), label: 'Files fixed', icon: <Image /> },
  { value: mockCompletion.livePhotosMerged.toLocaleString(), label: 'Live photos merged', icon: <Disc3 /> },
  { value: mockCompletion.noJson.toLocaleString(), label: 'No JSON found', icon: <FileText /> },
  { value: mockCompletion.warnings.toLocaleString(), label: 'Warnings', icon: <Trash2 /> },
];

export default function ReportPage() {
  const [format, setFormat] = useState('HTML');

  return (
    <PageFrame
      title="Export report"
      subtitle="9 Nov 2026 · 31 min · 13,710 files checked"
      footer={
        <>
          <span className="footer-note">Saved next to your photos as report.{format.toLowerCase()}</span>
          <div className="footer-buttons">
            <label className="format-select">
              <span className="visually-hidden">Report format</span>
              <select value={format} onChange={(event) => setFormat(event.target.value)}>
                <option>HTML</option><option>CSV</option>
              </select>
            </label>
            <Button>Copy summary</Button>
            <Button variant="accent">Save report</Button>
          </div>
        </>
      }
    >
      <div className="report-stats">
        {reportStats.map(({ value, label, icon }) => (
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
            {mockReportRows.map(([label, value]) => (
              <div className="settings-list__row" key={label}><dt>{label}</dt><dd>{value}</dd></div>
            ))}
          </dl>
        </Panel>
        <Panel className="report-panel report-panel--attention">
          <h2 className="panel-title">Needs attention</h2>
          <div className="attention-list">
            {mockReportItems.map(({ file, issue, result, tone }) => (
              <div className="attention-row" key={file}>
                <PhotoThumb tone={tone} />
                <strong>{file}</strong>
                <span className={issue === 'Unreadable EXIF' || issue === 'Corrupt video' ? 'attention-row__issue is-danger' : 'attention-row__issue'}>{issue}</span>
                <span className="attention-row__result">{result}</span>
              </div>
            ))}
          </div>
        </Panel>
      </div>
    </PageFrame>
  );
}
