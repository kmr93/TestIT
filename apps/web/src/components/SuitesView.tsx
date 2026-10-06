import React, { useEffect, useState } from 'react';
import {
  Plus,
  Play,
  Save,
  Send,
  Eye,
  CheckCircle2,
  FolderGit2,
  Smartphone,
  Monitor,
  AlertCircle,
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
import { MobileListEditor } from './MobileListEditor';
import { VariablePreviewModal } from './VariablePreviewModal';

interface SuitesViewProps {
  onRunStarted: (runId: string) => void;
}

export const SuitesView: React.FC<SuitesViewProps> = ({ onRunStarted }) => {
  const [assets, setAssets] = useState<Asset[]>([]);
  const [selectedAsset, setSelectedAsset] = useState<Asset | null>(null);
  const [nodes, setNodes] = useState<NodeInstance[]>([]);
  const [selectedNode, setSelectedNode] = useState<NodeInstance | null>(null);
  const [isMobileMode, setIsMobileMode] = useState(false);
  const [showPreviewModal, setShowPreviewModal] = useState(false);
  const [environments, setEnvironments] = useState<any[]>([]);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);

  useEffect(() => {
    loadAssets();
    loadEnvironments();
  }, []);

  const loadAssets = async () => {
    try {
      const data = await getAssets();
      setAssets(data);
      if (data.length > 0 && !selectedAsset) {
        selectAsset(data[0]);
      }
    } catch (_) {}
  };

  const loadEnvironments = async () => {
    try {
      const envs = await getEnvironments();
      setEnvironments(envs);
    } catch (_) {}
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
    } catch (_) {}
  };

  const handleCreateAsset = async () => {
    const name = prompt('Enter Test Case or Suite Name:');
    if (!name) return;
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
      await loadAssets();
      selectAsset(created);
    } catch (err: any) {
      alert(err.message);
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
    } catch (err: any) {
      alert(`Save failed: ${err.message}`);
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
    } catch (err: any) {
      alert(`Publish failed: ${err.message}`);
    }
  };

  const handleTriggerRun = async () => {
    if (!selectedAsset) return;
    try {
      // First ensure draft is published or publish revision
      const pub = await publishAssetRevision(selectedAsset.id, {
        expected_draft_version: selectedAsset.draft_version,
        change_note: 'Automated publish before run',
      });

      const envId = environments[0]?.id || '00000000-0000-0000-0000-000000000001';
      const runRes = await triggerRun({
        suite_revision_id: pub.revision_id,
        environment_id: envId,
      });

      onRunStarted(runRes.run_id);
    } catch (err: any) {
      alert(`Run failed: ${err.message}`);
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

          <div className="flex items-center bg-slate-900 border border-slate-800 rounded-lg p-0.5 mr-2">
            <button
              onClick={() => setIsMobileMode(false)}
              className={`p-1.5 rounded text-xs flex items-center gap-1 ${
                !isMobileMode ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-white'
              }`}
              title="Canvas view (Desktop/Tablet)"
            >
              <Monitor className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => setIsMobileMode(true)}
              className={`p-1.5 rounded text-xs flex items-center gap-1 ${
                isMobileMode ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-white'
              }`}
              title="Touch-friendly list view (Mobile)"
            >
              <Smartphone className="w-3.5 h-3.5" />
            </button>
          </div>

          <button
            onClick={() => setShowPreviewModal(true)}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Eye className="w-3.5 h-3.5 text-indigo-400" />
            Preview
          </button>
          <button
            onClick={handleSaveDraft}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Save className="w-3.5 h-3.5" />
            Save Draft
          </button>
          <button
            onClick={handlePublish}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Send className="w-3.5 h-3.5 text-violet-400" />
            Publish
          </button>
          <button
            onClick={handleTriggerRun}
            className="btn btn-primary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Play className="w-3.5 h-3.5 fill-current" />
            Run Suite
          </button>
        </div>
      </div>

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
                className="p-1 rounded-lg bg-indigo-600/20 text-indigo-400 hover:bg-indigo-600 hover:text-white"
              >
                <Plus className="w-4 h-4" />
              </button>
            </div>

            <div className="space-y-1.5 max-h-[70vh] overflow-y-auto pr-1">
              {assets.map((asset) => (
                <div
                  key={asset.id}
                  onClick={() => selectAsset(asset)}
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

        {/* Center: Canvas or Mobile List */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {isMobileMode ? (
            <MobileListEditor
              nodes={nodes}
              onChange={setNodes}
              onEditNode={(n) => setSelectedNode(n)}
            />
          ) : (
            <WorkflowCanvas
              nodes={nodes}
              onChange={setNodes}
              onSelectNode={(n) => setSelectedNode(n)}
              selectedNodeId={selectedNode?.id || null}
            />
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

      {showPreviewModal && selectedNode && (
        <VariablePreviewModal
          nodeConfig={selectedNode.config}
          onClose={() => setShowPreviewModal(false)}
        />
      )}
    </div>
  );
};
