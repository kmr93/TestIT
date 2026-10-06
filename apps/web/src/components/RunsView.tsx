import React, { useEffect, useState } from 'react';
import {
  PlayCircle,
  Clock,
  RotateCcw,
  GitCompare,
  CheckCircle2,
  XCircle,
  AlertTriangle,
  Download,
} from 'lucide-react';
import { getRuns, rerunFailed, getRunComparison } from '../api/client';
import { SuiteRun } from '../types';

interface RunsViewProps {
  onSelectRun: (runId: string) => void;
}

export const RunsView: React.FC<RunsViewProps> = ({ onSelectRun }) => {
  const [runs, setRuns] = useState<SuiteRun[]>([]);
  const [comparisonModalData, setComparisonModalData] = useState<any>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    loadRuns();
  }, []);

  const loadRuns = async () => {
    setLoading(true);
    try {
      const data = await getRuns();
      setRuns(data);
    } catch (_) {}
    setLoading(false);
  };

  const handleRerun = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const res = await rerunFailed(id);
      onSelectRun(res.run_id);
    } catch (err: any) {
      alert(`Rerun failed: ${err.message}`);
    }
  };

  const handleCompare = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const res = await getRunComparison(id);
      setComparisonModalData(res);
    } catch (err: any) {
      alert(`Comparison failed: ${err.message}`);
    }
  };

  return (
    <div className="flex-1 p-8 overflow-y-auto bg-slate-950">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-xl font-bold text-slate-100 tracking-tight">Execution Runs & Reports</h2>
          <p className="text-xs text-slate-400 mt-1">
            Historical test runs, live monitoring, comparisons against successful baselines
          </p>
        </div>
        <button onClick={loadRuns} className="btn btn-secondary text-xs py-2 px-3">
          Refresh Runs
        </button>
      </div>

      <div className="glass-panel overflow-hidden">
        <table className="w-full text-left text-xs">
          <thead className="bg-slate-900/80 border-b border-slate-800 text-slate-400 font-mono text-[11px] uppercase tracking-wider">
            <tr>
              <th className="py-3 px-4">Run ID</th>
              <th className="py-3 px-4">Status</th>
              <th className="py-3 px-4">Suite Revision</th>
              <th className="py-3 px-4">Created At</th>
              <th className="py-3 px-4 text-right">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-800/60 font-medium">
            {runs.length === 0 ? (
              <tr>
                <td colSpan={5} className="py-8 text-center text-slate-500">
                  No execution runs found. Trigger a suite run from the authoring view.
                </td>
              </tr>
            ) : (
              runs.map((r) => (
                <tr
                  key={r.id}
                  onClick={() => onSelectRun(r.id)}
                  className="hover:bg-slate-800/40 cursor-pointer transition-colors"
                >
                  <td className="py-3.5 px-4 font-mono text-slate-300">
                    {r.id.slice(0, 8)}...{r.id.slice(-4)}
                  </td>
                  <td className="py-3.5 px-4">
                    <span className={`badge badge-${r.status.toLowerCase()}`}>{r.status}</span>
                  </td>
                  <td className="py-3.5 px-4 font-mono text-slate-400">
                    {r.suite_revision_id.slice(0, 8)}...
                  </td>
                  <td className="py-3.5 px-4 text-slate-400">
                    {new Date(r.created_at).toLocaleString()}
                  </td>
                  <td className="py-3.5 px-4 text-right">
                    <div className="flex items-center justify-end gap-2" onClick={(e) => e.stopPropagation()}>
                      <button
                        onClick={(e) => handleCompare(r.id, e)}
                        className="btn btn-secondary text-[11px] py-1 px-2.5 flex items-center gap-1"
                        title="Compare with last successful run"
                      >
                        <GitCompare className="w-3 h-3 text-indigo-400" />
                        Compare
                      </button>
                      {r.status === 'FAILED' && (
                        <button
                          onClick={(e) => handleRerun(r.id, e)}
                          className="btn btn-secondary text-[11px] py-1 px-2.5 flex items-center gap-1 hover:border-amber-500"
                        >
                          <RotateCcw className="w-3 h-3 text-amber-400" />
                          Rerun
                        </button>
                      )}
                      <button
                        onClick={() => onSelectRun(r.id)}
                        className="btn btn-primary text-[11px] py-1 px-2.5"
                      >
                        Monitor
                      </button>
                    </div>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Comparison Modal */}
      {comparisonModalData && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4">
          <div className="glass-panel w-full max-w-xl bg-slate-900 border border-slate-700 p-6 space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <h3 className="font-bold text-slate-100 text-sm flex items-center gap-2">
                <GitCompare className="w-4 h-4 text-indigo-400" />
                Baseline Comparison
              </h3>
              <button
                onClick={() => setComparisonModalData(null)}
                className="text-xs text-slate-400 hover:text-white"
              >
                Close
              </button>
            </div>

            <div className="space-y-3 text-xs">
              <div className="p-3 bg-slate-950 border border-slate-800 rounded-lg">
                <p className="text-slate-400">Current Run: <span className="font-mono text-slate-200">{comparisonModalData.current_run_id}</span></p>
                <p className="text-slate-400 mt-1">
                  Baseline Run:{' '}
                  {comparisonModalData.baseline_run_id ? (
                    <span className="font-mono text-emerald-400">{comparisonModalData.baseline_run_id}</span>
                  ) : (
                    <span className="text-amber-400 italic">None found</span>
                  )}
                </p>
              </div>

              {comparisonModalData.reason && (
                <p className="text-slate-400 italic">{comparisonModalData.reason}</p>
              )}

              <div className="p-3 bg-slate-950 border border-slate-800 rounded-lg">
                <h5 className="font-semibold text-slate-300 mb-2">Delta Summary:</h5>
                <pre className="font-mono text-[11px] text-indigo-300">
                  {JSON.stringify(comparisonModalData.summary_delta, null, 2)}
                </pre>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
