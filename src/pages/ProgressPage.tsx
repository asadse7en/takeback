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
let startProcessingOutcomeHandled = false;

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
    if (!startProcessingPromise) startProcessingOutcomeHandled = false;
    let active = true;
    let unlisten: (() => void) | undefined;
    let observedRunning = false;
    let completionHandled = false;
    let statusRequestPending = false;
    let sawEngineProgress = false;

    const observeStartPromise = (promise: Promise<ProcessingStartResult>) => {
      if (handledStartProcessingPromise === promise) return;
      handledStartProcessingPromise = promise;
      void promise.then((result) => {
        startProcessingOutcomeHandled = true;
        if (result.cancelled) {
          setStatus('Cancelled');
          setError(result.warnings.join(' ') || 'Processing was cancelled before it started.');
          return;
        }
        if (!result.accepted) {
          setStatus('Blocked');
          setError(result.warnings.join(' ') || 'There is not enough free space to complete processing.');
          return;
        }
        setReport(result.report);
        setStatus(result.report?.summary.cancelled ? 'Cancelled' : 'Complete');
        navigate('/done');
      }).catch((reason: unknown) => {
        startProcessingOutcomeHandled = true;
        setStatus('Failed');
        setError(reason instanceof Error ? reason.message : String(reason));
      }).finally(() => {
        if (startProcessingPromise === promise) startProcessingPromise = null;
      });
    };

    subscribeToEvent('processing:progress', (progress) => {
      if (!progress.stage) sawEngineProgress = true;
      setProgress(progress);
    }).then((dispose) => {
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
          if (!startProcessingOutcomeHandled) {
            setStatus(
              startProcessingPromise && !sawEngineProgress
                ? 'Preparing'
                : status.paused ? 'Paused' : 'Running'
            );
          }
          return;
        }

        if (observedRunning && !completionHandled && !startProcessingOutcomeHandled) {
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
          startProcessingOutcomeHandled = false;
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
  const progressIndeterminate = run.status === 'Preparing' || run.stage !== null || run.total === 0;
  const stageCount = run.stageItemsTotal === null
    ? 'Preparing…'
    : `${(run.stageItemsDone ?? 0).toLocaleString()} of ${run.stageItemsTotal.toLocaleString()} items`;
  const progressCount = run.stage
    ? stageCount
    : run.status === 'Preparing'
      ? 'Preparing…'
      : `${run.completed.toLocaleString()} of ${run.total.toLocaleString()} files`;
  const terminalStatus = run.status === 'Failed'
    || run.status === 'Blocked'
    || run.status === 'Cancelled';

  return (
    <PageFrame
      className="progress-page-frame"
      title="Fixing your photos"
      subtitle="Originals are never modified."
      footer={
        <>
          <span className="footer-note">{run.status}</span>
          <div className="footer-buttons">
            {terminalStatus ? (
              <Button variant="accent" onClick={() => navigate('/preferences')}>Back to preferences</Button>
            ) : (
              <>
                <Button disabled={run.status === 'Ready' || run.status === 'Preparing' || run.status === 'Cancelling'} onClick={() => void togglePause()}>
                  {paused ? <Play size={14} /> : <Pause size={14} />}{paused ? 'Resume' : 'Pause'}
                </Button>
                <Button disabled={run.status === 'Ready' || run.status === 'Cancelling'} onClick={() => void cancel()}>
                  <X size={14} />Cancel
                </Button>
              </>
            )}
          </div>
        </>
      }
    >
      {terminalStatus && error ? (
        <section className="panel progress-error" role="alert">
          <AlertTriangle className="progress-error__icon" size={22} aria-hidden="true" />
          <div>
            <h2>{run.status === 'Blocked' ? 'Processing is blocked' : run.status === 'Cancelled' ? 'Processing cancelled' : 'Processing failed'}</h2>
            <p>{error}</p>
          </div>
        </section>
      ) : null}
      <Panel className="progress-card">
        <div className="progress-current">
          <div className="progress-top">
            <div>
              <span className="eyebrow">CURRENT FILE</span>
              <h2>{run.stage ?? (run.status === 'Preparing' ? 'Preparing files…' : run.currentFile || 'Preparing files…')}</h2>
            </div>
            <strong className="progress-percent">{progressIndeterminate ? '—' : `${percent}%`}</strong>
          </div>
          <div>
            <div className="progress-count">
              {progressCount}
            </div>
            <div
              className={`progress-meter${progressIndeterminate ? ' progress-meter--indeterminate' : ''}`}
              role="progressbar"
              aria-label={run.stage ? `Preparation: ${run.stage}` : 'Files fixed'}
              aria-valuenow={progressIndeterminate ? undefined : percent}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuetext={run.stage ?? undefined}
            >
              <span style={progressIndeterminate ? undefined : { width: `${percent}%` }} />
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
      {error && !terminalStatus ? <p role="alert" className="form-error">{error}</p> : null}
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
