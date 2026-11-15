import { useNavigate } from 'react-router-dom';
import { AlertTriangle, CalendarDays, Check, MapPin, RotateCcw, FolderOpen, Disc3 } from 'lucide-react';
import { Button, PageFrame, Panel } from '@/components/Ui';
import { mockCompletion } from '@/mock/data';

const completionStats = [
  { label: 'Fixed', count: mockCompletion.filesFixed, color: 'fixed' },
  { label: 'Duplicates', count: mockCompletion.duplicates, color: 'duplicates' },
  { label: 'No JSON', count: mockCompletion.noJson, color: 'no-json' },
  { label: 'Warnings', count: mockCompletion.warnings, color: 'warnings' },
] as const;

export default function DonePage() {
  const navigate = useNavigate();
  const summaryTiles = [
    { value: mockCompletion.datesRestored.toLocaleString(), label: 'Dates restored', icon: <CalendarDays /> },
    { value: mockCompletion.locationsAdded.toLocaleString(), label: 'Locations added', icon: <MapPin /> },
    { value: mockCompletion.livePhotosMerged.toLocaleString(), label: 'Live photos merged', icon: <Disc3 /> },
    { value: '170', label: 'Need a look', icon: <AlertTriangle />, action: <Button className="small-button" onClick={() => navigate('/review')}>Review</Button> },
  ];

  return (
    <PageFrame
      title="All done"
      subtitle={mockCompletion.saved}
      titleAdornment={<Check className="done-check" size={36} strokeWidth={1.5} aria-hidden="true" />}
      footer={
        <>
          <Button onClick={() => navigate('/add')}><RotateCcw size={14} />Start over</Button>
          <div className="footer-buttons">
            <Button onClick={() => navigate('/report')}>Export report</Button>
            <Button variant="accent" onClick={() => navigate('/report')}><FolderOpen size={14} />Open output folder</Button>
          </div>
        </>
      }
    >
      <Panel className="completion-card">
        <div className="completion-count">
          <strong>{mockCompletion.filesFixed.toLocaleString()}</strong>
          <span>of {mockCompletion.filesChecked.toLocaleString()} files fixed</span>
        </div>
        <div className="completion-bar" role="img" aria-label="Files fixed, duplicates, no JSON, and warnings">
          {completionStats.map((stat) => (
            <span key={stat.color} className={`completion-bar__${stat.color}`} style={{ width: `${stat.count / mockCompletion.filesChecked * 100}%` }} />
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
        {summaryTiles.map(({ value, label, icon, action }) => (
          <Panel className="summary-tile" key={label}>
            <span className="summary-tile__icon">{icon}</span>
            <strong>{value}</strong>
            <span>{label}</span>
            {action}
          </Panel>
        ))}
      </div>
    </PageFrame>
  );
}
