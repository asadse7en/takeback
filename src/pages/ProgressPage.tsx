import { useEffect, useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { AlertTriangle, Clock3, Gauge, Pause, Play, X } from 'lucide-react';
import { Button, PageFrame, Panel } from '@/components/Ui';
import { callCommand, subscribeToEvent, type ProcessingStartResult } from '@/lib/api';
import { useReportStore } from '@/store/report';
import { useRunStore } from '@/store/run';
import { useSettingsStore } from '@/store/settings';

let startProcessingPromise: Promise<ProcessingStartResult> | null = null;
let handledStartProcessingPromise: Promise<ProcessingStartResult> | null = null;

function formatEta(seconds: number | null) {
  if (seconds === null) return 'Calculating';
  if (seconds < 60) return `${seconds}s`;
  return `${Math.ceil(seconds / 60)} min`;
}

export default function ProgressPage() {
  const navigate = useNavigate();
  const run = useRunStore((state) => state.run);
  const setProgress = useRunStore((state) => state.setProgress);
  const setCounts = useRunStore((state) => state.setCounts);
  const setStatus = useRunStore((state) => state.setStatus);
  const claimStart = useRunStore((state) => state.claimStart);
  const setReport = useReportStore((state) => state.setReport);
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    let observedRunning = false;
    let completionHandled = false;
    let statusRequestPending = false;

    const observeStartPromise = (promise: Promise<ProcessingStartResult>) => {
      if (handledStartProcessingPromise === promise) return;
      handledStartProcessingPromise = promise;
      void promise.then((result) => {
        if (!result.accepted) {
          setStatus('Blocked');
          setError(result.warnings.join(' '));
          return;
        }
        setReport(result.report);
        setStatus(result.report?.summary.cancelled ? 'Cancelled' : 'Complete');
        navigate('/done');
      }).catch((reason: unknown) => {
        setStatus('Failed');
        setError(reason instanceof Error ? reason.message : String(reason));
      }).finally(() => {
        if (startProcessingPromise === promise) startProcessingPromise = null;
      });
    };

    subscribeToEvent('processing:progress', setProgress).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    }).catch((reason: unknown) => {
      if (active) setError(reason instanceof Error ? reason.message : String(reason));
    });

    const refreshStatus = async () => {
      if (statusRequestPending) return;
      statusRequestPending = true;
      try {
        const status = await callCommand('run_status', {});
        if (!active) return;
        if (status.running) {
          observedRunning = true;
          setCounts(status.completed, status.total);
          setPaused(status.paused);
          setStatus(status.paused ? 'Paused' : 'Running');
          return;
        }

        if (observedRunning && !completionHandled) {
          completionHandled = true;
          const report = await callCommand('get_report', {});
          if (!active) return;
          setReport(report);
          setStatus(report?.summary.cancelled ? 'Cancelled' : 'Complete');
          navigate('/done');
          return;
        }

        if (!startProcessingPromise && claimStart()) {
          const settings = useSettingsStore.getState().settings;
          if (!settings) {
            setStatus('Failed');
            setError('Preferences have not finished loading.');
            return;
          }
          setStatus('Preparing');
          startProcessingPromise = callCommand('start_processing', { settings });
        }
        if (startProcessingPromise) observeStartPromise(startProcessingPromise);
      } catch (reason: unknown) {
        if (active) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        statusRequestPending = false;
      }
    };
    void refreshStatus();
    const interval = window.setInterval(refreshStatus, 1000);
    return () => {
      active = false;
      unlisten?.();
      window.clearInterval(interval);
    };
  }, [claimStart, navigate, setCounts, setProgress, setReport, setStatus]);

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
        <div className="progress-current">
          <div className="progress-top">
            <div>
              <span className="eyebrow">CURRENT FILE</span>
              <h2>{run.currentFile || 'Preparing files…'}</h2>
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
        <Metric icon={<Gauge />} value={speed.toFixed(1)} suffix="MB/s" label="Speed" />
        <Metric icon={<Clock3 />} value={formatEta(run.etaSeconds)} label="Time remaining" />
        <Metric icon={<AlertTriangle />} value={`${run.warningsCount}`} label="Warnings">
          <Button className="small-button" onClick={() => navigate('/report')}>View</Button>
        </Metric>
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
  children,
}: {
  icon: ReactNode;
  value: string;
  suffix?: string;
  label: string;
  children?: ReactNode;
}) {
  return (
    <Panel className="metric-card">
      <span className="metric-icon">{icon}</span>
      <div className="metric-copy">
        <div className="metric-value">{value} {suffix ? <span>{suffix}</span> : null}</div>
        <div className="metric-label">{label}</div>
      </div>
      {children}
    </Panel>
  );
}
