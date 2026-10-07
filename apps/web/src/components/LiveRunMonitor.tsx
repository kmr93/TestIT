import React, { useEffect, useState } from 'react';
import {
  Activity,
  CheckCircle2,
  XCircle,
  Clock,
  Download,
  Ban,
  Radio,
  FileCode,
  FileText,
} from 'lucide-react';
import { cancelRun, getRunStats } from '../api/client';

interface LiveRunMonitorProps {
  runId: string;
  onClose: () => void;
  userRole: string;
}

export const LiveRunMonitor: React.FC<LiveRunMonitorProps> = ({ runId, onClose, userRole }) => {
  const canCancel = userRole === 'ADMIN' || userRole === 'RUNNER';
  const [snapshot, setSnapshot] = useState<any>(null);
  const [events, setEvents] = useState<any[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [status, setStatus] = useState<string>('QUEUED');
  const [monitorError, setMonitorError] = useState<string | null>(null);

  useEffect(() => {
    const sse = new EventSource(`/api/v1/runs/${runId}/events`);
    const refreshStats = () => {
      void getRunStats(runId).then((latest) => {
        setSnapshot(latest);
        setStatus(latest.status);
      }).catch(() => {});
    };
    refreshStats();
    const polling = window.setInterval(refreshStats, 2000);

    sse.onopen = () => {
      setIsConnected(true);
    };

    sse.addEventListener('run.snapshot', (e: MessageEvent) => {
      try {
        const snap = JSON.parse(e.data);
        setSnapshot(snap);
        setStatus(snap.status);
      } catch (_) {}
    });

    const trackEvent = (type: string) => (e: MessageEvent) => {
      try {
        const data = JSON.parse(e.data);
        if (type === 'run.started') setStatus('RUNNING');
        setEvents((prev) => [...prev.slice(-99), { type, data }]);
      } catch (_) {}
    };
    ['run.started', 'run.waiting_for_resource', 'run.planned', 'case.started', 'case.finished', 'step.started', 'step.finished']
      .forEach((type) => sse.addEventListener(type, trackEvent(type)));

    sse.addEventListener('run.finished', (e: MessageEvent) => {
      const data = JSON.parse(e.data);
      setStatus(data.status);
      setEvents((prev) => [...prev, { type: 'run.finished', data }]);
      sse.close();
      setIsConnected(false);
      refreshStats();
    });

    sse.onerror = () => {
      setIsConnected(false);
    };

    return () => {
      window.clearInterval(polling);
      sse.close();
    };
  }, [runId]);

  const handleCancel = async () => {
    try {
      await cancelRun(runId);
      setStatus('CANCELED');
      refreshAfterCancel();
    } catch (error) {
      setMonitorError(error instanceof Error ? error.message : 'Unable to cancel this run.');
    }
  };

  const refreshAfterCancel = () => {
    void getRunStats(runId).then(setSnapshot).catch(() => {});
  };

  const snapshotPercent = snapshot?.progress?.percent;
  const waitingResources: string[] = snapshot?.stats?.waiting_for_resources || [];
  const percent = Number.isFinite(snapshotPercent)
    ? Math.max(0, Math.min(100, Number(snapshotPercent)))
    : status === 'PASSED'
      ? 100
      : null;

  return (
    <div className="glass-panel p-6 bg-slate-900 border border-slate-700 rounded-xl space-y-6">
      {/* Top Header */}
      <div className="flex items-center justify-between pb-4 border-b border-slate-800">
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-xl bg-indigo-950 border border-indigo-700/50 flex items-center justify-center">
            <Activity className="w-5 h-5 text-indigo-400" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h3 className="font-bold text-slate-100">Live Execution Monitor</h3>
              <span className={`badge badge-${status.toLowerCase()}`}>{status}</span>
              {isConnected && (
                <span className="flex items-center gap-1 text-[11px] text-emerald-400 font-mono">
                  <Radio className="w-3 h-3 animate-pulse" /> SSE Live
                </span>
              )}
            </div>
            <p className="text-xs font-mono text-slate-400 mt-0.5">Run ID: {runId}</p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {canCancel && ['QUEUED', 'RUNNING'].includes(status) && (
            <button onClick={handleCancel} className="btn btn-danger text-xs py-1.5 px-3">
              <Ban className="w-3.5 h-3.5" />
              Cancel Run
            </button>
          )}
          <a
            href={`/api/v1/runs/${runId}/exports/junit`}
            download
            className="btn btn-secondary text-xs py-1.5 px-2.5 flex items-center gap-1.5"
            title="Download JUnit XML"
          >
            <FileCode className="w-3.5 h-3.5 text-amber-400" />
            JUnit
          </a>
          <a
            href={`/api/v1/runs/${runId}/exports/csv`}
            download
            className="btn btn-secondary text-xs py-1.5 px-2.5 flex items-center gap-1.5"
            title="Download CSV"
          >
            <FileText className="w-3.5 h-3.5 text-sky-400" />
            CSV
          </a>
          <a
            href={`/api/v1/runs/${runId}/exports/html`}
            target="_blank"
            rel="noreferrer"
            className="btn btn-primary text-xs py-1.5 px-2.5 flex items-center gap-1.5"
            title="View HTML Report"
          >
            <Download className="w-3.5 h-3.5" />
            Report
          </a>
          <button onClick={onClose} className="btn btn-secondary text-xs py-1.5 px-3 ml-2">
            Close
          </button>
        </div>
      </div>

      {status === 'QUEUED' && waitingResources.length > 0 && (
        <div className="rounded-lg border border-amber-900/70 bg-amber-950/30 px-4 py-3 text-xs text-amber-200">
          <p className="font-semibold">Waiting for exclusive resources</p>
          <p className="mt-1">Another run currently holds: <span className="font-mono">{waitingResources.join(', ')}</span></p>
          {snapshot?.stats?.lock_wait_timeout_seconds && <p className="mt-1 text-amber-300/80">This run will time out after {snapshot.stats.lock_wait_timeout_seconds} seconds.</p>}
        </div>
      )}

      {monitorError && <div role="alert" className="rounded-lg border border-rose-800 bg-rose-950/50 px-3 py-2 text-xs text-rose-200">{monitorError}</div>}

      {/* Progress Bar */}
      <div>
        <div className="flex justify-between text-xs font-medium text-slate-300 mb-2">
          <span>Overall Workflow Progress</span>
          <span className="font-mono font-bold text-indigo-400">
            {percent === null ? 'Unavailable' : `${percent}%`}
          </span>
        </div>
        <div className="w-full bg-slate-950 h-3 rounded-full overflow-hidden border border-slate-800">
          <div
            className="h-full bg-gradient-to-r from-indigo-500 to-emerald-400 transition-all duration-500 rounded-full"
            style={{ width: `${percent ?? 0}%` }}
          />
        </div>
      </div>

      <div className="grid grid-cols-4 gap-3" aria-label="Live run counts">
        <div className="rounded-lg border border-slate-800 bg-slate-950/70 p-3">
          <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-500">Cases</div>
          <div className="mt-1 font-mono text-sm text-slate-100">{snapshot?.stats?.cases_completed ?? 0} / {snapshot?.stats?.cases_total ?? '—'}</div>
        </div>
        {['passed', 'failed', 'error'].map((name) => (
          <div key={name} className="rounded-lg border border-slate-800 bg-slate-950/70 p-3">
            <div className="text-[10px] font-semibold uppercase tracking-wider text-slate-500">{name}</div>
            <div className="mt-1 font-mono text-sm text-slate-100">{snapshot?.stats?.case_counts?.[name] ?? 0}</div>
          </div>
        ))}
      </div>
      {(snapshot?.stats?.current_case || snapshot?.stats?.current_step) && (
        <div className="rounded-lg border border-indigo-900/70 bg-indigo-950/30 px-4 py-3 text-xs text-slate-300">
          Active case <span className="font-mono text-indigo-200">{snapshot?.stats?.current_case || '—'}</span>
          <span className="mx-2 text-slate-600">·</span>
          Current step <span className="font-medium text-indigo-200">{snapshot?.stats?.current_step || '—'}</span>
        </div>
      )}

      {/* Real-time Timeline Events */}
      <div>
        <h4 className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-3">
          Step Execution Stream
        </h4>
        <div className="space-y-2 max-h-60 overflow-y-auto pr-1">
          {events.length === 0 ? (
            <div className="p-4 bg-slate-950 border border-slate-800/80 rounded-lg text-xs text-slate-500 flex items-center gap-2">
              <Clock className="w-4 h-4 animate-spin text-indigo-400" />
              Initializing worker container and waiting for step transitions...
            </div>
          ) : (
            events.map((ev, i) => (
              <div
                key={i}
                className="flex items-center justify-between p-3 bg-slate-950 border border-slate-800 rounded-lg text-xs font-mono"
              >
                <div className="flex items-center gap-2">
                  {ev.data?.status === 'SUCCEEDED' || ev.data?.status === 'PASSED' ? (
                    <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                  ) : (
                    <XCircle className="w-4 h-4 text-red-400" />
                  )}
                  <span className="text-slate-300">{ev.type}</span>
                </div>
                <div className="flex items-center gap-3 text-slate-400 text-[11px]">
                  {ev.data?.duration_ms && (
                    <span>{Math.round(ev.data.duration_ms)}ms</span>
                  )}
                  <span className={`badge badge-${(ev.data?.status || 'info').toLowerCase()}`}>
                    {ev.data?.status || 'OK'}
                  </span>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
};
