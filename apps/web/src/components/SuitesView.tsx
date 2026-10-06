import React, { useEffect, useState } from 'react';
import {
  Plus,
  Play,
  Save,
  Send,
  Eye,
  CheckCircle2,
  FolderGit2,
} from 'lucide-react';
import {
  getAssets,
  createAsset,
  getAssetDraft,
  updateAssetDraft,
  publishAssetRevision,
  triggerRun,
  getEnvironments,
} from '../api/client';
import { Asset, NodeInstance } from '../types';
import { WorkflowCanvas } from './WorkflowCanvas';
import { VariablePreviewModal } from './VariablePreviewModal';
import { InlineAlert } from './InlineAlert';

interface SuitesViewProps {
  onRunStarted: (runId: string) => void;
}

export const SuitesView: React.FC<SuitesViewProps> = ({ onRunStarted }) => {
  const [assets, setAssets] = useState<Asset[]>([]);
  const [selectedAsset, setSelectedAsset] = useState<Asset | null>(null);
  const [nodes, setNodes] = useState<NodeInstance[]>([]);
  const [selectedNode, setSelectedNode] = useState<NodeInstance | null>(null);
  const [showPreviewModal, setShowPreviewModal] = useState(false);
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [newAssetName, setNewAssetName] = useState('');
  const [isCreatingAsset, setIsCreatingAsset] = useState(false);
  const [environments, setEnvironments] = useState<any[]>([]);
  const [selectedEnvironmentId, setSelectedEnvironmentId] = useState('');
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    loadAssets();
    loadEnvironments();
  }, []);

  const loadAssets = async () => {
    try {
      const data = await getAssets();
      setAssets(data);
      setSelectedAsset((current) =>
        current ? data.find((asset) => asset.id === current.id) ?? current : current
      );
      if (data.length > 0 && !selectedAsset) {
        selectAsset(data[0]);
      }
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to load test suites.');
    }
  };

  const loadEnvironments = async () => {
    try {
      const envs = await getEnvironments();
      setEnvironments(envs);
      setSelectedEnvironmentId((current) => current || envs[0]?.id || '');
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to load environments.');
    }
  };

  const selectAsset = async (asset: Asset) => {
    setSelectedAsset(asset);
    try {
      const draftData = await getAssetDraft(asset.id);
      const parsedNodes = draftData.draft?.nodes || [];
      setNodes(parsedNodes);
      if (parsedNodes.length > 0) {
        setSelectedNode(parsedNodes[0]);
      } else {
        setSelectedNode(null);
      }
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to load this draft.');
    }
  };

  const handleCreateAsset = () => {
    setErrorMessage(null);
    setNewAssetName('');
    setShowCreateDialog(true);
  };

  const submitCreateAsset = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const name = newAssetName.trim();
    if (!name) return;
    setIsCreatingAsset(true);
    setErrorMessage(null);
    try {
      const created = await createAsset({
        kind: 'case',
        name,
        initial_draft: {
          name,
          nodes: [
            {
              id: crypto.randomUUID(),
              type: 'api.request',
              type_version: 1,
              name: 'Health Check API',
              timeout_seconds: 30,
              config: { method: 'GET', path: 'https://httpbin.org/get', expected_status: 200 },
              position: { x: 50, y: 50 },
            },
          ],
          edges: [],
        },
      });
      selectAsset(created);
      setAssets((current) => [created, ...current.filter((asset) => asset.id !== created.id)]);
      setShowCreateDialog(false);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to create the test case.');
    } finally {
      setIsCreatingAsset(false);
    }
  };

  const handleSaveDraft = async () => {
    if (!selectedAsset) return;
    try {
      await updateAssetDraft(selectedAsset.id, {
        draft_json: { nodes, edges: [] },
        expected_draft_version: selectedAsset.draft_version,
      });
      setStatusMsg('Draft saved successfully');
      setTimeout(() => setStatusMsg(null), 3000);
      loadAssets();
    } catch (err) {
      setErrorMessage(`Save failed: ${err instanceof Error ? err.message : 'Unknown error'}`);
    }
  };

  const handlePublish = async () => {
    if (!selectedAsset) return;
    try {
      const res = await publishAssetRevision(selectedAsset.id, {
        expected_draft_version: selectedAsset.draft_version,
        change_note: 'Validated and published revision',
      });
      setStatusMsg(`Published Revision v${res.version} (Checksum: ${res.checksum.slice(0, 8)}...)`);
      setTimeout(() => setStatusMsg(null), 4000);
      loadAssets();
    } catch (err) {
      setErrorMessage(`Publish failed: ${err instanceof Error ? err.message : 'Unknown error'}`);
    }
  };

  const handleTriggerRun = async () => {
    if (!selectedAsset || selectedAsset.kind !== 'suite' || !selectedEnvironmentId) return;
    try {
      // First ensure draft is published or publish revision
      const pub = await publishAssetRevision(selectedAsset.id, {
        expected_draft_version: selectedAsset.draft_version,
        change_note: 'Automated publish before run',
      });

      const runRes = await triggerRun({
        suite_revision_id: pub.revision_id,
        environment_id: selectedEnvironmentId,
      });

      onRunStarted(runRes.run_id);
    } catch (err) {
      setErrorMessage(`Run failed: ${err instanceof Error ? err.message : 'Unknown error'}`);
    }
  };

  const updateNodeConfig = (key: string, value: any) => {
    if (!selectedNode) return;
    const updated = {
      ...selectedNode,
      config: { ...selectedNode.config, [key]: value },
    };
    setSelectedNode(updated);
    setNodes(nodes.map((n) => (n.id === updated.id ? updated : n)));
  };

  return (
    <div className="flex-1 flex flex-col h-screen overflow-hidden bg-slate-950">
      {/* Top Header Bar */}
      <div className="glass-header px-6 py-4 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <FolderGit2 className="w-5 h-5 text-indigo-400" />
          <div>
            <h2 className="font-bold text-slate-100 text-sm">
              {selectedAsset ? selectedAsset.name : 'Select or Create Asset'}
            </h2>
            <p className="text-xs text-slate-400">
              Draft v{selectedAsset?.draft_version || 1} • {nodes.length} nodes
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {statusMsg && (
            <span className="text-xs text-emerald-400 font-medium px-3 py-1 bg-emerald-950/40 border border-emerald-800 rounded-lg flex items-center gap-1.5 animate-fadeIn">
              <CheckCircle2 className="w-3.5 h-3.5" /> {statusMsg}
            </span>
          )}

          <label className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-slate-500">
            Environment
            <select
              aria-label="Run environment"
              value={selectedEnvironmentId}
              onChange={(event) => setSelectedEnvironmentId(event.target.value)}
              disabled={environments.length === 0}
              className="input-field w-40 py-2 text-xs normal-case tracking-normal"
            >
              <option value="">Choose environment</option>
              {environments.map((environment) => (
                <option key={environment.id} value={environment.id}>{environment.name}</option>
              ))}
            </select>
          </label>

          <button
            onClick={() => setShowPreviewModal(true)}
            disabled={!selectedNode}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Eye className="w-3.5 h-3.5 text-indigo-400" />
            Preview
          </button>
          <button
            onClick={handleSaveDraft}
            disabled={!selectedAsset}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Save className="w-3.5 h-3.5" />
            Save Draft
          </button>
          <button
            onClick={handlePublish}
            disabled={!selectedAsset}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Send className="w-3.5 h-3.5 text-violet-400" />
            Publish
          </button>
          <button
            onClick={handleTriggerRun}
            disabled={!selectedAsset || selectedAsset.kind !== 'suite' || !selectedEnvironmentId}
            title={selectedAsset?.kind !== 'suite' ? 'Only suite assets can be run.' : undefined}
            className="btn btn-primary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Play className="w-3.5 h-3.5 fill-current" />
            Run Suite
          </button>
        </div>
      </div>

      {errorMessage && (
        <InlineAlert message={errorMessage} onDismiss={() => setErrorMessage(null)} className="mx-6 mt-3" />
      )}

      {/* Main Workspace Layout */}
      <div className="flex-1 flex overflow-hidden p-6 gap-6">
        {/* Left Side: Asset Library */}
        <div className="w-64 glass-panel p-4 flex flex-col justify-between shrink-0">
          <div>
            <div className="flex items-center justify-between mb-4">
              <span className="text-xs font-semibold text-slate-400 uppercase tracking-wider">
                Suites & Cases
              </span>
              <button
                onClick={handleCreateAsset}
                aria-label="Create test case"
                title="Create test case"
                className="p-1 rounded-lg bg-indigo-600/20 text-indigo-400 hover:bg-indigo-600 hover:text-white"
              >
                <Plus className="w-4 h-4" />
              </button>
            </div>

            <div className="space-y-1.5 max-h-[70vh] overflow-y-auto pr-1">
              {assets.length === 0 && (
                <p className="rounded-lg border border-dashed border-slate-700/80 px-3 py-4 text-xs leading-5 text-slate-500">
                  Your workspace is ready. Create a test case to start building a workflow.
                </p>
              )}
              {assets.map((asset) => (
                <div
                  key={asset.id}
                  onClick={() => selectAsset(asset)}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      selectAsset(asset);
                    }
                  }}
                  className={`p-2.5 rounded-lg border text-xs cursor-pointer transition-all ${
                    selectedAsset?.id === asset.id
                      ? 'bg-indigo-950/40 border-indigo-500/50 text-indigo-200'
                      : 'bg-slate-950/60 border-slate-800 text-slate-400 hover:border-slate-700'
                  }`}
                >
                  <p className="font-semibold truncate">{asset.name}</p>
                  <div className="flex items-center justify-between mt-1 text-[10px] text-slate-500 font-mono">
                    <span>{asset.kind}</span>
                    <span>v{asset.draft_version}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Center: ordered workflow editor */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {selectedAsset ? (
            <WorkflowCanvas
              nodes={nodes}
              onChange={setNodes}
              onSelectNode={(n) => setSelectedNode(n)}
              selectedNodeId={selectedNode?.id || null}
            />
          ) : (
            <section className="glass-panel flex flex-1 flex-col items-center justify-center px-10 text-center">
              <div className="mb-5 flex h-14 w-14 items-center justify-center rounded-2xl border border-indigo-400/20 bg-indigo-400/10 text-indigo-300 shadow-glow">
                <FolderGit2 className="h-6 w-6" />
              </div>
              <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-indigo-300">Start with a test case</p>
              <h3 className="max-w-lg text-2xl font-semibold tracking-tight text-slate-100">Turn a backend check into a repeatable workflow.</h3>
              <p className="mt-3 max-w-md text-sm leading-6 text-slate-400">Create a case, add API and data checks, then publish a version for a run.</p>
              <button onClick={handleCreateAsset} className="btn btn-primary mt-6 px-4 py-2.5">
                <Plus className="h-4 w-4" /> Create test case
              </button>
            </section>
          )}
        </div>

        {/* Right Side: Node Configuration Inspector */}
        {selectedNode && (
          <div className="w-80 glass-panel p-5 shrink-0 flex flex-col justify-between overflow-y-auto">
            <div className="space-y-4">
              <div className="pb-3 border-b border-slate-800">
                <span className="text-[10px] font-mono text-indigo-400 uppercase tracking-wider">
                  Configuring Step
                </span>
                <h3 className="font-bold text-slate-100 text-sm mt-0.5">{selectedNode.name}</h3>
                <p className="text-xs font-mono text-slate-500 mt-0.5">{selectedNode.type}</p>
              </div>

              {selectedNode.type === 'api.request' && (
                <>
                  <div>
                    <label className="block text-xs font-semibold text-slate-300 mb-1">
                      HTTP Method
                    </label>
                    <select
                      value={selectedNode.config.method || 'GET'}
                      onChange={(e) => updateNodeConfig('method', e.target.value)}
                      className="input-field text-xs font-mono"
                    >
                      <option>GET</option>
                      <option>POST</option>
                      <option>PUT</option>
                      <option>PATCH</option>
                      <option>DELETE</option>
                    </select>
                  </div>

                  <div>
                    <label className="block text-xs font-semibold text-slate-300 mb-1">
                      URL / Endpoint (supports {'{{env.base_url}}'})
                    </label>
                    <input
                      type="text"
                      value={selectedNode.config.path || ''}
                      onChange={(e) => updateNodeConfig('path', e.target.value)}
                      className="input-field text-xs font-mono"
                      placeholder="https://api.example.com/v1/users"
                    />
                  </div>

                  <div>
                    <label className="block text-xs font-semibold text-slate-300 mb-1">
                      Expected Status Code
                    </label>
                    <input
                      type="number"
                      value={selectedNode.config.expected_status || 200}
                      onChange={(e) => updateNodeConfig('expected_status', parseInt(e.target.value))}
                      className="input-field text-xs font-mono"
                    />
                  </div>
                </>
              )}

              {selectedNode.type === 'sleep.wait' && (
                <div>
                  <label className="block text-xs font-semibold text-slate-300 mb-1">
                    Wait Duration (Seconds)
                  </label>
                  <input
                    type="number"
                    value={selectedNode.config.duration_seconds || 2}
                    onChange={(e) => updateNodeConfig('duration_seconds', parseInt(e.target.value))}
                    className="input-field text-xs font-mono"
                  />
                </div>
              )}

              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">
                  Timeout (Seconds)
                </label>
                <input
                  type="number"
                  value={selectedNode.timeout_seconds || 30}
                  onChange={(e) => {
                    const updated = {
                      ...selectedNode,
                      timeout_seconds: parseInt(e.target.value),
                    };
                    setSelectedNode(updated);
                    setNodes(nodes.map((n) => (n.id === updated.id ? updated : n)));
                  }}
                  className="input-field text-xs font-mono"
                />
              </div>
            </div>
          </div>
        )}
      </div>

      {showCreateDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6 backdrop-blur-sm">
          <section role="dialog" aria-modal="true" aria-labelledby="create-test-case-title" className="glass-panel w-full max-w-lg p-6 shadow-2xl">
            <div className="mb-5 flex items-start justify-between">
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-indigo-300">New workflow</p>
                <h2 id="create-test-case-title" className="mt-1 text-lg font-semibold text-slate-100">Create a test case</h2>
                <p className="mt-1 text-sm text-slate-400">Give this backend check a clear, reusable name.</p>
              </div>
              <button onClick={() => setShowCreateDialog(false)} className="rounded-lg p-2 text-slate-400 hover:bg-slate-800 hover:text-white" aria-label="Close dialog">
                <span aria-hidden="true">×</span>
              </button>
            </div>
            <form onSubmit={submitCreateAsset}>
              <label htmlFor="new-test-case-name" className="mb-1.5 block text-xs font-semibold text-slate-300">Test case name</label>
              <input
                id="new-test-case-name"
                autoFocus
                value={newAssetName}
                onChange={(event) => setNewAssetName(event.target.value)}
                className="input-field"
                placeholder="e.g. Customer API health check"
                maxLength={120}
                required
              />
              <div className="mt-5 flex justify-end gap-2">
                <button type="button" onClick={() => setShowCreateDialog(false)} className="btn btn-secondary">Cancel</button>
                <button type="submit" disabled={isCreatingAsset || !newAssetName.trim()} className="btn btn-primary">
                  {isCreatingAsset ? 'Creating…' : 'Create test case'}
                </button>
              </div>
            </form>
          </section>
        </div>
      )}

      {showPreviewModal && selectedNode && (
        <VariablePreviewModal
          nodeConfig={selectedNode.config}
          onClose={() => setShowPreviewModal(false)}
        />
      )}
    </div>
  );
};
