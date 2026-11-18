import { useEffect, useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { AlertTriangle, Check, Clock3, Gauge, MapPin, Pause, Play, X } from 'lucide-react';
import { Button, PageFrame, PageHeading, Panel, PhotoThumb } from '@/components/Ui';
import { callCommand, subscribeToEvent } from '@/lib/api';
import { useReportStore } from '@/store/report';
import { useRunStore } from '@/store/run';
import { useSettingsStore } from '@/store/settings';

function formatEta(seconds: number | null) {
  if (seconds === null) return 'Calculating';
  if (seconds < 60) return `${seconds}s`;
  return `${Math.ceil(seconds / 60)} min`;
}

export default function ProgressPage() {
  const navigate = useNavigate();
  const run = useRunStore((state) => state.run);
  const setProgress = useRunStore((state) => state.setProgress);
  const setStatus = useRunStore((state) => state.setStatus);
  const claimStart = useRunStore((state) => state.claimStart);
  const settings = useSettingsStore((state) => state.settings);
  const setReport = useReportStore((state) => state.setReport);
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    subscribeToEvent('processing:progress', setProgress).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    }).catch((reason: unknown) => {
      if (active) setError(reason instanceof Error ? reason.message : String(reason));
    });

    if (claimStart()) {
      setStatus('Preparing');
      callCommand('start_processing', { settings }).then((result) => {
        if (!active) return;
        if (!result.accepted) {
          setStatus('Blocked');
          setError(result.warnings.join(' '));
          return;
        }
        setReport(result.report);
        setStatus(result.report?.summary.cancelled ? 'Cancelled' : 'Complete');
        navigate('/done');
      }).catch((reason: unknown) => {
        if (active) {
          setStatus('Failed');
          setError(reason instanceof Error ? reason.message : String(reason));
        }
      });
    }

    return () => {
      active = false;
      unlisten?.();
    };
  }, [claimStart, navigate, setProgress, setReport, setStatus, settings]);

  useEffect(() => {
    let active = true;
    const refreshStatus = () => {
      callCommand('run_status', {}).then((status) => {
        if (!active || !status.running) return;
        setPaused(status.paused);
        setStatus(status.paused ? 'Paused' : 'Running');
      }).catch((reason: unknown) => {
        if (active) setError(reason instanceof Error ? reason.message : String(reason));
      });
    };
    refreshStatus();
    const interval = window.setInterval(refreshStatus, 1000);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [setStatus]);

  const togglePause = async () => {
    try {
      if (paused) await callCommand('resume_run', {});
      else await callCommand('pause_run', {});
      setPaused(!paused);
      setStatus(paused ? 'Running' : 'Paused');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };
  const cancel = async () => {
    try {
      setStatus('Cancelling');
      await callCommand('cancel_processing', {});
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };
  const percent = run.percent;
  const speed = run.bytesPerSecond / 1_000_000;

  return (
    <PageFrame
      title="Fixing your photos"
      subtitle="Originals are never modified."
      footer={
        <>
          <span className="footer-note">{run.status}</span>
          <div className="footer-buttons">
            <Button onClick={() => void togglePause()}>
              {paused ? <Play size={14} /> : <Pause size={14} />}{paused ? 'Resume' : 'Pause'}
            </Button>
            <Button onClick={() => void cancel()}><X size={14} />Cancel</Button>
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
              <h2>{run.currentFile || 'Preparing files…'}</h2>
              <div className="progress-steps">
                <span><Check size={15} />Read JSON</span>
                <span><Check size={15} />Write date</span>
                <span className="is-pending"><MapPin size={15} />Write GPS</span>
              </div>
            </div>
            <strong className="progress-percent">{percent}%</strong>
          </div>
          <div>
            <div className="progress-count">{run.completed.toLocaleString()} of {run.total.toLocaleString()} files</div>
            <div className="progress-meter" role="progressbar" aria-label="Files fixed" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
              <span style={{ width: `${percent}%` }} />
            </div>
          </div>
        </div>
      </Panel>

      <div className="progress-metrics">
        <Metric icon={<Gauge />} value={speed.toFixed(1)} suffix="MB/s" label="Speed" graph />
        <Metric icon={<Clock3 />} value={formatEta(run.etaSeconds)} label="Time remaining" />
        <Metric icon={<AlertTriangle />} value={`${run.warningsCount}`} label="Warnings">
          <Button className="small-button" onClick={() => navigate('/report')}>View</Button>
        </Metric>
      </div>

      <PageHeading>Recently fixed</PageHeading>
      <div className="recent-files">
        {run.recentFiles.map((file, index) => (
          <Panel className="recent-file" key={file}>
            <PhotoThumb tone={index % 2 ? 'blue' : 'peach'} className="recent-file__thumb" />
            <strong>{file}</strong>
            <span className={run.warnings.length ? 'recent-file__status is-warning' : 'recent-file__status'}>
              {run.warnings.length ? <AlertTriangle size={14} /> : <Check size={14} />}
              {run.warnings.length ? 'Warnings' : 'Processing'}
            </span>
          </Panel>
        ))}
      </div>
      {error ? <p role="alert" className="form-error">{error}</p> : null}
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
