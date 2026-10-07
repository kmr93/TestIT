import React, { useEffect, useMemo, useState } from 'react';
import {
  Activity,
  AlertTriangle,
  Ban,
  CheckCircle2,
  Clock,
  FileCode,
  FileText,
  RefreshCw,
  Radio,
  WifiOff,
  X,
  XCircle,
} from 'lucide-react';
import { cancelRun, getRunStats } from '../api/client';

interface LiveRunMonitorProps {
  runId: string;
  onClose: () => void;
  userRole: string;
}

type MonitorTab = 'overview' | 'timeline' | 'statistics';

const count = (value: unknown) => (typeof value === 'number' && Number.isFinite(value) ? value : 0);

const formatDuration = (seconds: number) => {
  const value = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const remainder = value % 60;
  if (hours > 0) return hours + 'h ' + minutes + 'm';
  if (minutes > 0) return minutes + 'm ' + remainder + 's';
  return remainder + 's';
};

const formatTime = (value?: string | null) => {
  if (!value) return 'Not started';
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? 'Unavailable'
    : date.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'medium' });
};

const eventHeadline = (type: string, data: Record<string, any>) => {
  if (type.startsWith('case.')) {
    const ordinal = Number.isFinite(data.ordinal) ? 'Case ' + data.ordinal : 'Case';
    const iteration = Number.isFinite(data.iteration_index) ? ' · iteration ' + (data.iteration_index + 1) : '';
    return ordinal + iteration;
  }
  if (type.startsWith('step.')) return data.node_name || 'Workflow step';
  if (type.startsWith('suite.hook.')) return data.scope === 'suite_cleanup' ? 'Suite cleanup' : 'Suite setup';
  if (type === 'run.waiting_for_resource') return 'Waiting for an exclusive resource';
  if (type === 'run.planned') return 'Execution plan created';
  if (type === 'run.started') return 'Run started';
  if (type === 'run.finished') return 'Run finished';
  return type.split('.').join(' ');
};

const isSuccess = (status?: string) => ['SUCCEEDED', 'PASSED'].includes(status || '');
const isFailure = (status?: string) =>
  ['FAILED', 'ASSERTION_FAILED', 'ERROR', 'TIMED_OUT', 'INTERRUPTED'].includes(status || '');

export const LiveRunMonitor: React.FC<LiveRunMonitorProps> = ({ runId, onClose, userRole }) => {
  const canCancel = userRole === 'ADMIN' || userRole === 'RUNNER';
  const [snapshot, setSnapshot] = useState<any>(null);
  const [events, setEvents] = useState<any[]>([]);
  const [isConnected, setIsConnected] = useState(false);
  const [pollFailed, setPollFailed] = useState(false);
  const [lastPollAt, setLastPollAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const [status, setStatus] = useState<string>('QUEUED');
  const [monitorError, setMonitorError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<MonitorTab>('overview');
  const [canceling, setCanceling] = useState(false);

  useEffect(() => {
    let active = true;
    const refreshStats = async () => {
      try {
        const latest = await getRunStats(runId);
        if (!active) return;
        setSnapshot(latest);
        setStatus(latest.status);
        setPollFailed(false);
        setLastPollAt(Date.now());
      } catch {
        if (active) setPollFailed(true);
      }
    };
    const addEvent = (type: string, event: MessageEvent) => {
      try {
        const data = JSON.parse(event.data);
        if (type === 'run.started') setStatus('RUNNING');
        if (type === 'run.finished') setStatus(data.status || 'ERROR');
        setEvents((previous) => {
          const sequence = Number(data.sequence);
          if (Number.isFinite(sequence) && previous.some((item) => item.data.sequence === sequence)) {
            return previous;
          }
          return [...previous, { type, data }].slice(-250);
        });
      } catch {
        // Ignore malformed stream events. The durable snapshot remains authoritative.
      }
    };

    void refreshStats();
    const polling = window.setInterval(() => void refreshStats(), 2000);
    const clock = window.setInterval(() => setNow(Date.now()), 1000);
    const stream = new EventSource('/api/v1/runs/' + runId + '/events');
    stream.onopen = () => setIsConnected(true);
    stream.onerror = () => setIsConnected(false);
    stream.addEventListener('run.snapshot', (event) => {
      try {
        const latest = JSON.parse((event as MessageEvent).data);
        setSnapshot((previous: any) => ({ ...previous, ...latest }));
        setStatus(latest.status);
      } catch {
        // A later snapshot or the bounded poll will recover state.
      }
    });
    [
      'run.started',
      'run.waiting_for_resource',
      'run.planned',
      'suite.hook.started',
      'suite.hook.finished',
      'case.started',
      'case.variable_error',
      'case.finished',
      'step.started',
      'step.finished',
      'run.finished',
    ].forEach((type) => stream.addEventListener(type, (event) => addEvent(type, event as MessageEvent)));

    return () => {
      active = false;
      window.clearInterval(polling);
      window.clearInterval(clock);
      stream.close();
    };
  }, [runId]);

  const run = snapshot?.run || {};
  const stats = snapshot?.stats || {};
  const progress = snapshot?.progress || {};
  const caseCounts = stats.case_counts || {};
  const nodeCounts = stats.node_counts || {};
  const api = snapshot?.api_statistics || {};
  const waitingResources: string[] = stats.waiting_for_resources || [];
  const isTerminal = ['PASSED', 'FAILED', 'ERROR', 'CANCELED', 'INTERRUPTED'].includes(status);
  const pollingAge = lastPollAt === null ? null : Math.max(0, Math.floor((now - lastPollAt) / 1000));
  const connectionLabel = isConnected
    ? 'Live stream'
    : pollingAge !== null && pollingAge <= 6 && !pollFailed
      ? 'Polling fallback'
      : 'Data may be stale';
  const nodeTotal = count(progress.planned_nodes);
  const nodeDone = count(progress.terminal_nodes);
  const caseTotal = count(stats.cases_total ?? progress.planned_cases);
  const caseDone = count(stats.cases_completed ?? progress.terminal_cases);
  const nodePercent = nodeTotal > 0 ? Math.max(0, Math.min(100, Math.round((nodeDone / nodeTotal) * 100))) : null;
  const casePercent = caseTotal > 0 ? Math.max(0, Math.min(100, Math.round((caseDone / caseTotal) * 100))) : null;
  const elapsedSeconds = useMemo(() => {
    if (typeof run.elapsed_seconds === 'number') return run.elapsed_seconds;
    if (!run.started_at) return 0;
    const start = new Date(run.started_at).getTime();
    const end = run.finished_at ? new Date(run.finished_at).getTime() : now;
    return Number.isFinite(start) && Number.isFinite(end) ? Math.max(0, (end - start) / 1000) : 0;
  }, [now, run.elapsed_seconds, run.finished_at, run.started_at]);

  const handleCancel = async () => {
    if (!window.confirm('Cancel this run? Active work will stop and cleanup will be attempted.')) return;
    setCanceling(true);
    setMonitorError(null);
    try {
      await cancelRun(runId);
      setStatus('CANCELED');
      const latest = await getRunStats(runId);
      setSnapshot(latest);
      setStatus(latest.status);
    } catch (error) {
      setMonitorError(error instanceof Error ? error.message : 'Unable to cancel this run.');
    } finally {
      setCanceling(false);
    }
  };

  const refreshNow = async () => {
    try {
      const latest = await getRunStats(runId);
      setSnapshot(latest);
      setStatus(latest.status);
      setPollFailed(false);
      setLastPollAt(Date.now());
    } catch (error) {
      setPollFailed(true);
      setMonitorError(error instanceof Error ? error.message : 'Unable to refresh run status.');
    }
  };

  const renderCountCard = (label: string, value: number, tone: string) => (
    <div key={label} className="rounded-xl border border-slate-800 bg-slate-950/70 px-4 py-3">
      <div className="text-[10px] font-semibold uppercase tracking-[0.12em] text-slate-500">{label}</div>
      <div className={'mt-2 font-mono text-xl font-semibold ' + tone}>{value}</div>
    </div>
  );

  const renderTimeline = (limit?: number) => {
    const items = limit ? events.slice(-limit).reverse() : events;
    if (items.length === 0) {
      return (
        <div className="flex min-h-40 flex-col items-center justify-center rounded-xl border border-dashed border-slate-800 bg-slate-950/50 text-center">
          <Clock className="mb-2 h-5 w-5 text-slate-600" />
          <p className="text-sm font-medium text-slate-300">Waiting for run events</p>
          <p className="mt-1 text-xs text-slate-500">Step and case transitions will appear here.</p>
        </div>
      );
    }
    return (
      <ol className="divide-y divide-slate-800/80">
        {items.map((item, index) => {
          const itemStatus = String(item.data.status || '');
          const Icon = isSuccess(itemStatus) ? CheckCircle2 : isFailure(itemStatus) ? XCircle : Activity;
          const iconTone = isSuccess(itemStatus) ? 'text-emerald-400' : isFailure(itemStatus) ? 'text-rose-400' : 'text-indigo-300';
          return (
            <li key={item.data.sequence || item.type + '-' + index} className="flex items-start gap-3 py-3">
              <Icon className={'mt-0.5 h-4 w-4 shrink-0 ' + iconTone} aria-hidden="true" />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="text-sm font-medium text-slate-200">{eventHeadline(item.type, item.data)}</span>
                  <span className="text-[11px] text-slate-500">{item.type.split('.').join(' · ')}</span>
                </div>
                <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-slate-500">
                  {item.data.status && <span>Status: <strong className="font-medium text-slate-300">{item.data.status}</strong></span>}
                  {item.data.duration_ms != null && <span>{Math.round(Number(item.data.duration_ms))} ms</span>}
                  {item.data.attempt != null && <span>Attempt {item.data.attempt}</span>}
                  {item.data.occurred_at && <time dateTime={item.data.occurred_at}>{formatTime(item.data.occurred_at)}</time>}
                </div>
              </div>
            </li>
          );
        })}
      </ol>
    );
  };

  return (
    <section className="glass-panel flex h-full min-h-0 flex-col overflow-hidden" aria-label="Run monitor">
      <header className="flex shrink-0 items-center justify-between gap-5 border-b border-slate-800 px-6 py-4">
        <div className="flex min-w-0 items-center gap-3">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-indigo-400/20 bg-indigo-400/10">
            <Activity className="h-5 w-5 text-indigo-300" />
          </div>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="truncate text-base font-semibold text-slate-100">{run.suite_name || 'Suite run'}</h2>
              <span className={'badge badge-' + status.toLowerCase()}>{status}</span>
              <span className={'inline-flex items-center gap-1.5 text-[11px] ' + (pollingAge !== null && pollingAge > 6 ? 'text-amber-300' : 'text-slate-400')}>
                {connectionLabel === 'Live stream' ? <Radio className="h-3.5 w-3.5 text-emerald-400" /> : connectionLabel === 'Polling fallback' ? <RefreshCw className="h-3.5 w-3.5" /> : <WifiOff className="h-3.5 w-3.5" />}
                {connectionLabel}
              </span>
            </div>
            <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-slate-500">
              <span>Revision {run.suite_revision ? 'v' + run.suite_revision : '—'}</span>
              <span>{run.environment_name || 'Environment unavailable'}</span>
              <span>Started by {run.initiated_by || 'Unknown'}</span>
              <span className="font-mono">Run {runId.slice(0, 8)}</span>
            </div>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button onClick={() => void refreshNow()} className="btn btn-secondary text-xs" title="Refresh run data">
            <RefreshCw className="h-3.5 w-3.5" /> Refresh
          </button>
          {canCancel && !isTerminal && (
            <button onClick={() => void handleCancel()} disabled={canceling} className="btn btn-danger text-xs">
              <Ban className="h-3.5 w-3.5" /> {canceling ? 'Canceling…' : 'Cancel run'}
            </button>
          )}
          <a href={'/api/v1/runs/' + runId + '/exports/junit'} download className="btn btn-secondary text-xs" title="Download JUnit XML">
            <FileCode className="h-3.5 w-3.5 text-amber-300" /> JUnit
          </a>
          <a href={'/api/v1/runs/' + runId + '/exports/csv'} download className="btn btn-secondary text-xs" title="Download CSV">
            <FileText className="h-3.5 w-3.5 text-sky-300" /> CSV
          </a>
          <a href={'/api/v1/runs/' + runId + '/exports/html'} target="_blank" rel="noreferrer" className="btn btn-primary text-xs">
            View report
          </a>
          <button onClick={onClose} className="btn btn-secondary !px-2.5" aria-label="Close run monitor" title="Close">
            <X className="h-4 w-4" />
          </button>
        </div>
      </header>

      <div className="flex shrink-0 items-center justify-between gap-4 border-b border-slate-800/80 bg-slate-950/40 px-6 py-2.5 text-[11px]">
        <div className="flex flex-wrap gap-x-5 gap-y-1 text-slate-400">
          <span>Created <strong className="font-medium text-slate-200">{formatTime(run.created_at)}</strong></span>
          <span>Started <strong className="font-medium text-slate-200">{formatTime(run.started_at)}</strong></span>
          <span>{run.finished_at ? 'Finished' : 'Elapsed'} <strong className="font-medium text-slate-200">{run.finished_at ? formatTime(run.finished_at) : formatDuration(elapsedSeconds)}</strong></span>
        </div>
        <span className="shrink-0 text-slate-500">{pollingAge === null ? 'Syncing…' : 'Updated ' + pollingAge + 's ago'}</span>
      </div>

      {monitorError && <div role="alert" className="mx-6 mt-4 rounded-lg border border-rose-800 bg-rose-950/50 px-3 py-2 text-xs text-rose-200">{monitorError}</div>}
      {waitingResources.length > 0 && status === 'QUEUED' && (
        <div className="mx-6 mt-4 flex items-start gap-2 rounded-lg border border-amber-800/70 bg-amber-950/30 px-4 py-3 text-xs text-amber-200">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <div><strong className="font-semibold">Waiting for exclusive resources</strong><p className="mt-1">{waitingResources.join(', ')}</p></div>
        </div>
      )}

      <nav className="flex shrink-0 items-center gap-1 border-b border-slate-800 px-6 pt-3" aria-label="Run report views">
        {([
          ['overview', 'Overview'],
          ['timeline', 'Timeline'],
          ['statistics', 'Statistics'],
        ] as Array<[MonitorTab, string]>).map(([tab, label]) => (
          <button
            key={tab}
            type="button"
            onClick={() => setActiveTab(tab)}
            aria-current={activeTab === tab ? 'page' : undefined}
            className={'border-b-2 px-3 pb-3 text-xs font-medium transition-colors ' + (activeTab === tab ? 'border-indigo-400 text-indigo-200' : 'border-transparent text-slate-500 hover:text-slate-200')}
          >{label}</button>
        ))}
      </nav>

      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
        {activeTab === 'overview' && (
          <div className="space-y-5">
            <section className="grid grid-cols-[minmax(0,1.6fr)_minmax(360px,1fr)] gap-4">
              <div className="rounded-xl border border-slate-800 bg-slate-950/70 p-5">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <h3 className="text-sm font-semibold text-slate-100">Node execution</h3>
                    <p className="mt-1 text-xs text-slate-500">Completed nodes include passed, failed, canceled, interrupted, and skipped steps.</p>
                  </div>
                  <div className="text-right font-mono text-sm text-slate-200">
                    {nodeDone.toLocaleString()} <span className="text-slate-500">/ {nodeTotal ? nodeTotal.toLocaleString() : '—'}</span>
                    <div className="mt-1 text-[11px] text-indigo-300">{nodePercent === null ? 'Progress estimate unavailable' : nodePercent + '% complete'}</div>
                  </div>
                </div>
                <div className="mt-4 h-2 overflow-hidden rounded-full bg-slate-800">
                  <div className="h-full rounded-full bg-indigo-400 transition-[width] duration-500" style={{ width: (nodePercent ?? 0) + '%' }} />
                </div>
                <div className="mt-4 flex items-center justify-between border-t border-slate-800 pt-3 text-xs">
                  <span className="text-slate-500">Case iterations</span>
                  <span className="font-mono text-slate-200">{caseDone.toLocaleString()} / {caseTotal ? caseTotal.toLocaleString() : '—'} <span className="ml-1 text-slate-500">{casePercent === null ? '' : '(' + casePercent + '%)'}</span></span>
                </div>
              </div>
              <div className="rounded-xl border border-slate-800 bg-slate-950/70 p-5">
                <h3 className="text-sm font-semibold text-slate-100">Current activity</h3>
                <div className="mt-3 space-y-3">
                  <div><div className="text-[10px] font-semibold uppercase tracking-wider text-slate-500">Case or hook</div><div className="mt-1 truncate text-sm text-slate-200">{stats.current_case || (status === 'QUEUED' ? 'Queued for execution' : 'No active case')}</div></div>
                  <div><div className="text-[10px] font-semibold uppercase tracking-wider text-slate-500">Current step</div><div className="mt-1 truncate text-sm text-indigo-200">{stats.current_step || 'No step is running'}</div></div>
                </div>
              </div>
            </section>

            <section className="grid grid-cols-5 gap-3" aria-label="Case outcome counts">
              {renderCountCard('Passed', count(caseCounts.passed), 'text-emerald-300')}
              {renderCountCard('Failed', count(caseCounts.failed), 'text-rose-300')}
              {renderCountCard('Errors', count(caseCounts.error), 'text-amber-300')}
              {renderCountCard('Active', count(nodeCounts.running), 'text-indigo-200')}
              {renderCountCard('Queued', count(nodeCounts.queued), 'text-slate-200')}
            </section>

            <section className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-4">
              <div className="rounded-xl border border-slate-800 bg-slate-950/70 p-5">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-slate-100">Case outcomes</h3>
                  <span className="text-[11px] text-slate-500">{caseTotal || 0} planned iterations</span>
                </div>
                <div className="mt-4 grid grid-cols-3 gap-2">
                  {[
                    ['Passed', count(caseCounts.passed), 'text-emerald-300'],
                    ['Failed', count(caseCounts.failed), 'text-rose-300'],
                    ['Error', count(caseCounts.error), 'text-amber-300'],
                    ['Canceled', count(caseCounts.canceled), 'text-slate-300'],
                    ['Skipped', count(caseCounts.skipped), 'text-slate-400'],
                    ['Interrupted', count(caseCounts.interrupted), 'text-orange-300'],
                  ].map(([label, value, tone]) => (
                    <div key={String(label)} className="rounded-lg border border-slate-800 bg-slate-900/50 px-3 py-2">
                      <div className="text-[10px] uppercase tracking-wide text-slate-500">{label}</div>
                      <div className={'mt-1 font-mono text-sm ' + tone}>{Number(value)}</div>
                    </div>
                  ))}
                </div>
              </div>
              <div className="rounded-xl border border-slate-800 bg-slate-950/70 p-5">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-slate-100">API activity</h3>
                  <span className="text-[11px] text-slate-500">Observed run traffic</span>
                </div>
                {count(api.requests) === 0 ? (
                  <p className="mt-4 rounded-lg border border-dashed border-slate-800 px-4 py-5 text-xs text-slate-500">API request metrics will appear when API steps finish.</p>
                ) : (
                  <div className="mt-4 grid grid-cols-7 gap-2 text-center">
                    <div><div className="text-[10px] uppercase tracking-wide text-slate-500">Requests</div><div className="mt-1 font-mono text-sm text-slate-100">{count(api.requests)}</div></div>
                    <div><div className="text-[10px] uppercase tracking-wide text-slate-500">2xx</div><div className="mt-1 font-mono text-sm text-emerald-300">{count(api.status_classes?.['2xx'])}</div></div>
                    <div><div className="text-[10px] uppercase tracking-wide text-slate-500">3xx</div><div className="mt-1 font-mono text-sm text-slate-300">{count(api.status_classes?.['3xx'])}</div></div>
                    <div><div className="text-[10px] uppercase tracking-wide text-slate-500">4xx</div><div className="mt-1 font-mono text-sm text-amber-300">{count(api.status_classes?.['4xx'])}</div></div>
                    <div><div className="text-[10px] uppercase tracking-wide text-slate-500">5xx</div><div className="mt-1 font-mono text-sm text-rose-300">{count(api.status_classes?.['5xx'])}</div></div>
                    <div><div className="text-[10px] uppercase tracking-wide text-slate-500">p50</div><div className="mt-1 font-mono text-sm text-slate-100">{api.p50_ms == null ? '—' : Math.round(api.p50_ms) + ' ms'}</div></div>
                    <div><div className="text-[10px] uppercase tracking-wide text-slate-500">p95</div><div className="mt-1 font-mono text-sm text-slate-100">{api.p95_ms == null ? '—' : Math.round(api.p95_ms) + ' ms'}</div></div>
                  </div>
                )}
                {api.requests_per_minute != null && <p className="mt-3 text-right text-[10px] text-slate-500">Observed rate: {Number(api.requests_per_minute).toFixed(1)} requests/min · diagnostic only</p>}
              </div>
            </section>

            <section className="rounded-xl border border-slate-800 bg-slate-950/70 px-5 py-4">
              <div className="mb-2 flex items-center justify-between">
                <h3 className="text-sm font-semibold text-slate-100">Recent activity</h3>
                <button type="button" onClick={() => setActiveTab('timeline')} className="text-xs font-medium text-indigo-300 hover:text-indigo-200">View timeline</button>
              </div>
              {renderTimeline(6)}
            </section>
          </div>
        )}

        {activeTab === 'timeline' && (
          <section className="mx-auto max-w-5xl rounded-xl border border-slate-800 bg-slate-950/70 px-5 py-4">
            <div className="mb-2 flex items-center justify-between border-b border-slate-800 pb-3">
              <div><h3 className="text-sm font-semibold text-slate-100">Run timeline</h3><p className="mt-1 text-xs text-slate-500">Sanitized state changes in sequence order.</p></div>
              <span className="font-mono text-[11px] text-slate-500">{events.length} events since monitor opened</span>
            </div>
            {renderTimeline()}
          </section>
        )}

        {activeTab === 'statistics' && (
          <div className="mx-auto grid max-w-5xl grid-cols-2 gap-4">
            <section className="rounded-xl border border-slate-800 bg-slate-950/70 p-5">
              <h3 className="text-sm font-semibold text-slate-100">Node status counts</h3>
              <p className="mt-1 text-xs text-slate-500">Terminal skipped steps count toward completion.</p>
              <div className="mt-4 divide-y divide-slate-800">
                {Object.entries(nodeCounts).length === 0 ? <p className="py-4 text-xs text-slate-500">No node outcomes recorded yet.</p> : Object.entries(nodeCounts).sort(([a], [b]) => a.localeCompare(b)).map(([key, value]) => (
                  <div key={key} className="flex items-center justify-between py-2 text-xs"><span className="uppercase tracking-wide text-slate-400">{key.split('_').join(' ')}</span><span className="font-mono text-slate-100">{count(value)}</span></div>
                ))}
              </div>
            </section>
            <section className="rounded-xl border border-slate-800 bg-slate-950/70 p-5">
              <h3 className="text-sm font-semibold text-slate-100">API response statistics</h3>
              <p className="mt-1 text-xs text-slate-500">Aggregate request timing and status classes for this run.</p>
              <div className="mt-4 grid grid-cols-2 gap-3">
                {[
                  ['Requests', count(api.requests)],
                  ['Observed rate', api.requests_per_minute == null ? '—' : Number(api.requests_per_minute).toFixed(1) + ' / min'],
                  ['p50 latency', api.p50_ms == null ? '—' : Math.round(api.p50_ms) + ' ms'],
                  ['p95 latency', api.p95_ms == null ? '—' : Math.round(api.p95_ms) + ' ms'],
                ].map(([label, value]) => <div key={String(label)} className="rounded-lg border border-slate-800 bg-slate-900/50 p-3"><div className="text-[10px] uppercase tracking-wide text-slate-500">{label}</div><div className="mt-1 font-mono text-sm text-slate-100">{value}</div></div>)}
              </div>
              <div className="mt-4 space-y-2">
                {Object.entries(api.status_classes || {}).sort(([a], [b]) => a.localeCompare(b)).map(([key, value]) => (
                  <div key={key} className="flex items-center justify-between rounded-lg border border-slate-800 bg-slate-900/50 px-3 py-2 text-xs"><span className="text-slate-400">{key} responses</span><span className="font-mono text-slate-100">{count(value)}</span></div>
                ))}
                {count(api.requests) === 0 && <p className="text-xs text-slate-500">No API request metrics are available yet.</p>}
              </div>
              <p className="mt-4 border-t border-slate-800 pt-3 text-[10px] leading-4 text-slate-500">These are observed workflow request statistics, not a load-test result.</p>
            </section>
          </div>
        )}
      </div>
    </section>
  );
};
