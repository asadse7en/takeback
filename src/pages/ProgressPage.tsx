import { useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { AlertTriangle, Check, Clock3, Gauge, MapPin, Pause, Play, X } from 'lucide-react';
import { Button, PageFrame, PageHeading, Panel, PhotoThumb } from '@/components/Ui';
import { mockProgress } from '@/mock/data';

export default function ProgressPage() {
  const navigate = useNavigate();
  const [paused, setPaused] = useState(false);

  return (
    <PageFrame
      title="Fixing your photos"
      subtitle="Originals are never modified."
      footer={
        <>
          <span className="footer-note">Safe to close: progress is saved</span>
          <div className="footer-buttons">
            <Button onClick={() => setPaused((value) => !value)}>
              {paused ? <Play size={14} /> : <Pause size={14} />}{paused ? 'Resume' : 'Pause'}
            </Button>
            <Button onClick={() => navigate('/done')}><X size={14} />Cancel</Button>
          </div>
        </>
      }
    >
      <Panel className="progress-card">
        <PhotoThumb tone="peach" className="progress-current-preview" />
        <div className="progress-current">
          <div className="progress-top">
            <div>
              <span className="eyebrow">NOW FIXING</span>
              <h2>{mockProgress.currentFile}</h2>
              <div className="progress-steps">
                <span><Check size={15} />Read JSON</span>
                <span><Check size={15} />Write date</span>
                <span className="is-pending"><MapPin size={15} />Write GPS</span>
              </div>
            </div>
            <strong className="progress-percent">{mockProgress.percent}%</strong>
          </div>
          <div>
            <div className="progress-count">{mockProgress.completedFiles.toLocaleString()} of {mockProgress.totalFiles.toLocaleString()} files</div>
            <div className="progress-meter" role="progressbar" aria-label="Files fixed" aria-valuenow={mockProgress.percent} aria-valuemin={0} aria-valuemax={100}>
              <span style={{ width: `${mockProgress.percent}%` }} />
            </div>
          </div>
        </div>
      </Panel>

      <div className="progress-metrics">
        <Metric icon={<Gauge />} value={`${mockProgress.filesPerSecond}`} suffix="files/s" label="Speed" graph />
        <Metric icon={<Clock3 />} value={`${mockProgress.minutesRemaining}`} suffix="min" label="Done around 7:42 PM" />
        <Metric icon={<AlertTriangle />} value={`${mockProgress.warnings}`} label="Warnings">
          <Button className="small-button" onClick={() => navigate('/report')}>View</Button>
        </Metric>
      </div>

      <PageHeading>Recently fixed</PageHeading>
      <div className="recent-files">
        {mockProgress.recentFiles.map((file) => (
          <Panel className="recent-file" key={file.name}>
            <PhotoThumb tone={file.tone} className="recent-file__thumb" />
            <strong>{file.name}</strong>
            <span className={file.status === 'No JSON' ? 'recent-file__status is-warning' : 'recent-file__status'}>
              {file.status === 'No JSON' ? <AlertTriangle size={14} /> : <Check size={14} />}
              {file.status}
            </span>
          </Panel>
        ))}
      </div>
    </PageFrame>
  );
}

function Metric({
  icon,
  value,
  suffix,
  label,
  graph = false,
  children,
}: {
  icon: ReactNode;
  value: string;
  suffix?: string;
  label: string;
  graph?: boolean;
  children?: ReactNode;
}) {
  return (
    <Panel className="metric-card">
      <span className="metric-icon">{icon}</span>
      <div className="metric-copy">
        <div className="metric-value">{value} {suffix ? <span>{suffix}</span> : null}</div>
        <div className="metric-label">{label}</div>
      </div>
      {graph ? (
        <svg className="progress-sparkline" viewBox="0 0 96 32" fill="none" aria-hidden="true">
          <path d="M2 24 16 18 30 22 44 12 58 16 72 7 94 11" stroke="var(--tb-color-accent-deep)" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      ) : children}
    </Panel>
  );
}
