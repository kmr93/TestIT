import React, { useState } from 'react';
import { X, ShieldCheck, Eye, AlertCircle, Play } from 'lucide-react';
import { previewVariables } from '../api/client';

interface VariablePreviewModalProps {
  nodeConfig: any;
  environmentId?: string;
  suiteVariables?: Record<string, any>;
  caseVariables?: Record<string, any>;
  onClose: () => void;
}

export const VariablePreviewModal: React.FC<VariablePreviewModalProps> = ({
  nodeConfig,
  environmentId,
  suiteVariables,
  caseVariables,
  onClose,
}) => {
  const [sampleInputs, setSampleInputs] = useState('{\n  "order_id": "ORD-9901",\n  "amount": 250\n}');
  const [sampleIteration, setSampleIteration] = useState('{\n  "email": "tester@example.test",\n  "tier": "gold"\n}');
  const [previewResult, setPreviewResult] = useState<any>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runPreview = async () => {
    setLoading(true);
    setError(null);
    try {
      let parsedInputs = {};
      let parsedIter = {};
      try { parsedInputs = JSON.parse(sampleInputs); } catch { throw new Error('Sample case inputs must be valid JSON.'); }
      try { parsedIter = JSON.parse(sampleIteration); } catch { throw new Error('Sample iteration data must be valid JSON.'); }

      const res = await previewVariables({
        node_config: nodeConfig || {
          url: '{{env.api_base_url}}/orders/{{case.order_id}}',
          headers: {
            Authorization: 'Bearer {{secret.api_token}}',
            'X-Customer-Email': '{{iteration.email}}',
          },
        },
        sample_inputs: parsedInputs,
        sample_iteration: parsedIter,
        environment_id: environmentId,
        suite_variables: suiteVariables,
        case_variables: caseVariables,
      });
      setPreviewResult(res);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4">
      <div className="glass-panel w-full max-w-2xl bg-slate-900 border border-slate-700 shadow-2xl flex flex-col max-h-[90vh]">
        <div className="flex items-center justify-between p-4 border-b border-slate-800">
          <div className="flex items-center gap-2">
            <Eye className="w-5 h-5 text-indigo-400" />
            <h2 className="font-bold text-slate-100">Deterministic Variable Preview</h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-slate-800"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="p-6 overflow-y-auto space-y-4">
          <div className="p-3 bg-indigo-950/40 border border-indigo-800/60 rounded-lg flex items-center gap-2.5 text-xs text-indigo-300">
            <ShieldCheck className="w-4 h-4 text-emerald-400 shrink-0" />
            <span>
              <strong>Zero-Network Guarantee:</strong> Preview resolves values via pure Rust evaluation without sending external requests or reading live secrets.
            </span>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-xs font-semibold text-slate-300 mb-1.5">
                Sample Case Inputs (JSON)
              </label>
              <textarea
                value={sampleInputs}
                onChange={(e) => setSampleInputs(e.target.value)}
                className="input-field font-mono text-xs h-28 resize-none"
              />
            </div>
            <div>
              <label className="block text-xs font-semibold text-slate-300 mb-1.5">
                Sample Iteration Row (JSON)
              </label>
              <textarea
                value={sampleIteration}
                onChange={(e) => setSampleIteration(e.target.value)}
                className="input-field font-mono text-xs h-28 resize-none"
              />
            </div>
          </div>

          <button
            onClick={runPreview}
            disabled={loading}
            className="btn btn-primary w-full text-xs py-2.5 flex items-center justify-center gap-2"
          >
            <Play className="w-4 h-4 fill-current" />
            {loading ? 'Resolving variables...' : 'Evaluate & Render Preview'}
          </button>

          {error && (
            <div className="p-3 bg-red-950/50 border border-red-800 rounded-lg text-xs text-red-300 flex items-center gap-2">
              <AlertCircle className="w-4 h-4 text-red-400 shrink-0" />
              {error}
            </div>
          )}

          {previewResult && (
            <div className="space-y-3 pt-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-slate-300">Resolved Output Preview:</span>
                <span className="badge badge-passed text-[10px]">
                  network_accessed: {String(previewResult.network_accessed)}
                </span>
              </div>

              <pre className="p-4 bg-slate-950 border border-slate-800 rounded-lg font-mono text-xs text-emerald-400 overflow-x-auto">
                {JSON.stringify(previewResult.resolved_config, null, 2)}
              </pre>

              {previewResult.errors?.length > 0 && <div role="alert" className="rounded-lg border border-rose-800 bg-rose-950/40 p-3 text-xs text-rose-200">
                <h3 className="font-semibold">Preview resolution issues</h3>
                <ul className="mt-1 list-disc space-y-1 pl-4">{previewResult.errors.map((item: string, index: number) => <li key={index}>{item}</li>)}</ul>
              </div>}

              {previewResult.masked_secrets?.length > 0 && (
                <div className="text-xs text-slate-400 flex items-center gap-2">
                  <span className="text-slate-500">Masked Secrets:</span>
                  {previewResult.masked_secrets.map((sec: string) => (
                    <span key={sec} className="font-mono bg-slate-800 px-2 py-0.5 rounded text-[11px] text-amber-400">
                      [SECRET:{sec}]
                    </span>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
