import React, { useState } from 'react';
import { X, FileCode, CheckCircle2, AlertCircle, ArrowRight } from 'lucide-react';
import { validateOpenApi, importOpenApi } from '../api/client';

interface OpenApiImportModalProps {
  onClose: () => void;
  onImportSuccess: () => void;
}

export const OpenApiImportModal: React.FC<OpenApiImportModalProps> = ({
  onClose,
  onImportSuccess,
}) => {
  const [specContent, setSpecContent] = useState(`{
  "openapi": "3.0.0",
  "info": { "title": "Customer Payment API", "version": "1.0.0" },
  "paths": {
    "/v1/customers": {
      "get": { "operationId": "listCustomers", "summary": "List customers" },
      "post": { "operationId": "createCustomer", "summary": "Create new customer" }
    },
    "/v1/payments/{id}": {
      "get": { "operationId": "getPayment", "summary": "Get payment status" }
    }
  }
}`);
  const [validationResult, setValidationResult] = useState<any>(null);
  const [selectedOps, setSelectedOps] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleValidate = async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await validateOpenApi(specContent);
      setValidationResult(res);
      setSelectedOps(res.operations.map((o: any) => o.operation_id));
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  const handleFile = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    if (file.size > 5 * 1024 * 1024) {
      setError('OpenAPI JSON must be 5 MiB or smaller.');
      event.target.value = '';
      return;
    }
    try {
      setSpecContent(await file.text());
      setValidationResult(null);
      setSelectedOps([]);
      setError(null);
    } catch {
      setError('Unable to read this OpenAPI JSON file.');
    }
    event.target.value = '';
  };

  const handleImport = async () => {
    if (selectedOps.length === 0) return;
    setLoading(true);
    try {
      await importOpenApi(specContent, selectedOps);
      onImportSuccess();
      onClose();
    } catch (err: any) {
      setError(err.message);
    } finally {
      setLoading(false);
    }
  };

  const toggleOp = (id: string) => {
    setSelectedOps((prev) =>
      prev.includes(id) ? prev.filter((o) => o !== id) : [...prev, id]
    );
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4">
      <div className="glass-panel w-full max-w-2xl bg-slate-900 border border-slate-700 shadow-2xl flex flex-col max-h-[90vh]">
        <div className="flex items-center justify-between p-4 border-b border-slate-800">
          <div className="flex items-center gap-2">
            <FileCode className="w-5 h-5 text-indigo-400" />
            <h2 className="font-bold text-slate-100">Import OpenAPI Specification</h2>
          </div>
          <button onClick={onClose} className="p-1 rounded-lg text-slate-400 hover:text-white">
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="p-6 overflow-y-auto space-y-4">
          <div>
            <label className="block text-xs font-semibold text-slate-300 mb-1.5">
              OpenAPI 3.0 / 3.1 JSON Specification
            </label>
            <input aria-label="Upload OpenAPI JSON file" type="file" accept=".json,application/json" onChange={handleFile} className="mb-2 block w-full text-xs text-slate-400 file:mr-3 file:rounded-lg file:border-0 file:bg-slate-800 file:px-3 file:py-2 file:text-xs file:font-semibold file:text-slate-200" />
            <textarea
              value={specContent}
              onChange={(e) => setSpecContent(e.target.value)}
              className="input-field font-mono text-xs h-36 resize-none"
            />
          </div>

          <button
            onClick={handleValidate}
            disabled={loading}
            className="btn btn-secondary w-full text-xs py-2"
          >
            {loading ? 'Validating schema...' : 'Validate Specification'}
          </button>

          {error && (
            <div className="p-3 bg-red-950/50 border border-red-800 rounded-lg text-xs text-red-300 flex items-center gap-2">
              <AlertCircle className="w-4 h-4 text-red-400 shrink-0" />
              {error}
            </div>
          )}

          {validationResult && (
            <div className="space-y-3 pt-2">
              <div className="flex items-center justify-between">
                <div>
                  <h4 className="text-sm font-bold text-slate-200">{validationResult.title}</h4>
                  <p className="text-xs text-slate-400">Version: {validationResult.version}</p>
                </div>
                <span className="badge badge-passed text-xs">
                  {validationResult.total_operations} operations found
                </span>
              </div>

              <div className="space-y-1.5 max-h-48 overflow-y-auto pr-1">
                {validationResult.operations.map((op: any) => (
                  <div
                    key={op.operation_id}
                    onClick={() => toggleOp(op.operation_id)}
                    className={`flex items-center justify-between p-2.5 rounded-lg border text-xs cursor-pointer transition-all ${
                      selectedOps.includes(op.operation_id)
                        ? 'bg-indigo-950/40 border-indigo-500/50 text-indigo-200'
                        : 'bg-slate-950 border-slate-800 text-slate-400'
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <span className="font-mono font-bold text-[10px] px-1.5 py-0.5 rounded bg-slate-800 text-indigo-400">
                        {op.method}
                      </span>
                      <span className="font-mono text-[11px]">{op.path}</span>
                    </div>
                    <span className="text-right text-slate-400 text-[11px]">{op.summary}<span className="block text-[10px] text-slate-500">{op.parameters?.length || 0} params{op.has_response_schema ? ' · response schema' : ''}</span></span>
                  </div>
                ))}
              </div>

              <button
                onClick={handleImport}
                disabled={loading || selectedOps.length === 0}
                className="btn btn-primary w-full text-xs py-2.5 flex items-center justify-center gap-2"
              >
                <span>Import {selectedOps.length} Operations as Case Drafts</span>
                <ArrowRight className="w-4 h-4" />
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
