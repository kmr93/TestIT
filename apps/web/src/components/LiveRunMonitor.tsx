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
import { cancelRun } from '../api/client';

interface LiveRunMonitorProps {
  runId: string;
  onClose: () => void;
}

export const LiveRunMonitor: React.FC<LiveRunMonitorProps> = ({ runId, onClose }) => {
  const [snapshot, setSnapshot] = useState<any>(null);
  const [events, setEvents] = useState<any[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [status, setStatus] = useState<string>('QUEUED');

  useEffect(() => {
    const sse = new EventSource(`/api/v1/runs/${runId}/events`);

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

    sse.addEventListener('run.started', (e: MessageEvent) => {
      setStatus('RUNNING');
      setEvents((prev) => [...prev, { type: 'run.started', data: JSON.parse(e.data) }]);
    });

    sse.addEventListener('step.finished', (e: MessageEvent) => {
      const data = JSON.parse(e.data);
      setEvents((prev) => [...prev, { type: 'step.finished', data }]);
    });

    sse.addEventListener('run.finished', (e: MessageEvent) => {
      const data = JSON.parse(e.data);
      setStatus(data.status);
      setEvents((prev) => [...prev, { type: 'run.finished', data }]);
      sse.close();
      setIsConnected(false);
    });

    sse.onerror = () => {
      setIsConnected(false);
    };

    return () => {
      sse.close();
    };
  }, [runId]);

  const handleCancel = async () => {
    try {
      await cancelRun(runId);
      setStatus('CANCELED');
    } catch (_) {}
  };

  const percent = snapshot?.progress?.percent ?? (status === 'PASSED' ? 100 : 25);

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
          {status === 'RUNNING' && (
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

      {/* Progress Bar */}
      <div>
        <div className="flex justify-between text-xs font-medium text-slate-300 mb-2">
          <span>Overall Workflow Progress</span>
          <span className="font-mono font-bold text-indigo-400">{percent}%</span>
        </div>
        <div className="w-full bg-slate-950 h-3 rounded-full overflow-hidden border border-slate-800">
          <div
            className="h-full bg-gradient-to-r from-indigo-500 to-emerald-400 transition-all duration-500 rounded-full"
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>

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
