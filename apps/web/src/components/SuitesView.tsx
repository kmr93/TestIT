import React, { useEffect, useRef, useState } from 'react';
import {
  Plus,
  Play,
  Save,
  Send,
  Eye,
  CheckCircle2,
  FolderGit2,
  ArrowDown,
  ArrowUp,
  X,
  Download,
  Upload,
} from 'lucide-react';
import {
  getAssets,
  createAsset,
  getAssetDraft,
  getAssetRevision,
  updateAssetDraft,
  publishAssetRevision,
  getAssetRevisions,
  triggerRun,
  getEnvironments,
  getConnections,
  downloadProjectBundle,
  previewProjectBundle,
  importProjectBundle,
} from '../api/client';
import { Asset, NodeInstance } from '../types';
import { WorkflowCanvas } from './WorkflowCanvas';
import { VariablePreviewModal } from './VariablePreviewModal';
import { InlineAlert } from './InlineAlert';

interface SuitesViewProps {
  onRunStarted: (runId: string) => void;
  userRole: string;
}

export const SuitesView: React.FC<SuitesViewProps> = ({ onRunStarted, userRole }) => {
  const canEdit = userRole === 'ADMIN' || userRole === 'AUTHOR';
  const canRun = userRole === 'ADMIN' || userRole === 'RUNNER';
  const [assets, setAssets] = useState<Asset[]>([]);
  const [selectedAsset, setSelectedAsset] = useState<Asset | null>(null);
  const [nodes, setNodes] = useState<NodeInstance[]>([]);
  const [suiteSetupNodes, setSuiteSetupNodes] = useState<NodeInstance[]>([]);
  const [suiteCleanupNodes, setSuiteCleanupNodes] = useState<NodeInstance[]>([]);
  const [datasetFormat, setDatasetFormat] = useState<'json' | 'csv'>('json');
  const [datasetText, setDatasetText] = useState('');
  const [suiteCases, setSuiteCases] = useState<Array<{ case_id: string; revision_id?: string; ordinal: number }>>([]);
  const [caseVariables, setCaseVariables] = useState<Record<string, any>>({});
  const [suiteVariables, setSuiteVariables] = useState<Record<string, any>>({});
  const [variableEditorError, setVariableEditorError] = useState<string | null>(null);
  const [publishedCaseRevisions, setPublishedCaseRevisions] = useState<Array<{ assetId: string; name: string; revisionId: string; version: number }>>([]);
  const [candidateRevisionId, setCandidateRevisionId] = useState('');
  const [selectedNode, setSelectedNode] = useState<NodeInstance | null>(null);
  const [selectedNodeScope, setSelectedNodeScope] = useState<'case' | 'suite_setup' | 'suite_cleanup'>('case');
  const [showPreviewModal, setShowPreviewModal] = useState(false);
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [newAssetName, setNewAssetName] = useState('');
  const [newAssetKind, setNewAssetKind] = useState<'case' | 'suite'>('case');
  const [isCreatingAsset, setIsCreatingAsset] = useState(false);
  const [environments, setEnvironments] = useState<any[]>([]);
  const [connections, setConnections] = useState<any[]>([]);
  const [selectedEnvironmentId, setSelectedEnvironmentId] = useState('');
  const [runInputsText, setRunInputsText] = useState('{}');
  const [inputSchemaText, setInputSchemaText] = useState('{}');
  const [resourceLocksText, setResourceLocksText] = useState('');
  const [lockWaitTimeoutText, setLockWaitTimeoutText] = useState('300');
  const [hasInputSchemaContract, setHasInputSchemaContract] = useState(true);
  const [selectedRevisionId, setSelectedRevisionId] = useState('');
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const bundleInputRef = useRef<HTMLInputElement>(null);
  const [bundleFile, setBundleFile] = useState<File | null>(null);
  const [bundlePreview, setBundlePreview] = useState<any | null>(null);
  const [isHandlingBundle, setIsHandlingBundle] = useState(false);

  useEffect(() => {
    loadAssets();
    if (userRole !== 'VIEWER') loadEnvironments();
    if (canEdit || canRun) {
      getConnections().then(setConnections).catch((err) => setErrorMessage(err instanceof Error ? err.message : 'Unable to load connection profiles.'));
    }
  }, []);

  const loadAssets = async () => {
    try {
      const data = await getAssets();
      setAssets(data);
      const cases = data.filter((asset) => asset.kind === 'case');
      const revisions = await Promise.all(cases.map(async (asset) => {
        try {
          const published = await getAssetRevisions(asset.id);
          return published[0] ? {
            assetId: asset.id,
            name: asset.name,
            revisionId: published[0].id,
            version: published[0].version,
          } : null;
        } catch {
          return null;
        }
      }));
      const availableRevisions = revisions.filter((item): item is NonNullable<typeof item> => item !== null);
      setPublishedCaseRevisions(availableRevisions);
      setCandidateRevisionId((current) => current || availableRevisions[0]?.revisionId || '');
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
    setSelectedRevisionId('');
    setErrorMessage(null);
    setVariableEditorError(null);
    try {
      let definition: any;
      if (canEdit) {
        const draftData = await getAssetDraft(asset.id);
        definition = draftData.draft;
        setSelectedRevisionId('');
      } else {
        const revisions = await getAssetRevisions(asset.id);
        const latest = revisions[0];
        if (!latest) throw new Error('This asset has no published revision yet. Ask an author to publish it.');
        const published = await getAssetRevision(asset.id, latest.id);
        definition = published.definition;
        setSelectedRevisionId(latest.id);
      }
      if (asset.kind === 'suite') {
        setSuiteCases(definition?.cases || []);
        setSuiteVariables(definition?.variables || {});
        setInputSchemaText(JSON.stringify(definition?.input_schema || {}, null, 2));
        setResourceLocksText((definition?.resource_locks || []).join('\n'));
        setLockWaitTimeoutText(String(definition?.lock_wait_timeout_seconds ?? 300));
        setHasInputSchemaContract(Object.prototype.hasOwnProperty.call(definition || {}, 'input_schema'));
        setCaseVariables({});
        setNodes([]);
        setSuiteSetupNodes(definition?.setup_nodes || []);
        setSuiteCleanupNodes(definition?.cleanup_nodes || []);
        setDatasetFormat('json');
        setDatasetText('');
        setSelectedNodeScope('case');
        setSelectedNode(null);
      } else {
        setInputSchemaText('{}');
        setResourceLocksText('');
        setLockWaitTimeoutText('300');
        setHasInputSchemaContract(false);
        const parsedNodes = definition?.nodes || [];
        setNodes(parsedNodes);
        setSuiteSetupNodes([]);
        setSuiteCleanupNodes([]);
        setCaseVariables(definition?.variables || {});
        setSuiteVariables({});
        const dataset = definition?.data_set || definition?.dataset;
        const format = dataset?.format === 'csv' ? 'csv' : 'json';
        setDatasetFormat(format);
        setDatasetText(!dataset ? '' : format === 'csv'
          ? (dataset?.content || '')
          : JSON.stringify(dataset?.rows || [], null, 2));
        setSuiteCases([]);
        setSelectedNodeScope('case');
        setSelectedNode(parsedNodes[0] || null);
      }
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to load this draft.');
    }
  };

  const handleCreateAsset = () => {
    if (!canEdit) return;
    setErrorMessage(null);
    setNewAssetName('');
    setNewAssetKind('case');
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
        kind: newAssetKind,
        name,
        initial_draft: newAssetKind === 'suite'
          ? { id: crypto.randomUUID(), name, description: '', execution_mode: 'sequential', input_schema: {}, setup_nodes: [], cleanup_nodes: [], cases: [] }
          : {
              id: crypto.randomUUID(),
              name,
              description: '',
              nodes: [{
                id: crypto.randomUUID(),
                type: 'api.request',
                type_version: 1,
                name: 'Health Check API',
                timeout_seconds: 30,
                config: { method: 'GET', path: '', expected_status: 200, headers: {}, assertions: [] },
                position: { x: 50, y: 50 },
              }],
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
    if (variableEditorError) { setErrorMessage(variableEditorError); return; }
    if (selectedAsset.kind === 'suite' && suiteCases.length === 0) {
      setErrorMessage('Add at least one published case to the suite before saving.');
      return;
    }
    try {
      const saved = await updateAssetDraft(selectedAsset.id, {
        draft_json: buildDraft(selectedAsset, nodes, suiteSetupNodes, suiteCleanupNodes, suiteCases, caseVariables, suiteVariables, datasetFormat, datasetText, inputSchemaText, resourceLocksText, lockWaitTimeoutText),
        expected_draft_version: selectedAsset.draft_version,
      });
      setSelectedAsset({ ...selectedAsset, draft_version: saved.draft_version });
      setStatusMsg('Draft saved successfully');
      setTimeout(() => setStatusMsg(null), 3000);
      loadAssets();
    } catch (err) {
      setErrorMessage(`Save failed: ${err instanceof Error ? err.message : 'Unknown error'}`);
    }
  };

  const handlePublish = async () => {
    if (!selectedAsset) return;
    if (variableEditorError) { setErrorMessage(variableEditorError); return; }
    if (selectedAsset.kind === 'suite' && suiteCases.length === 0) {
      setErrorMessage('Add at least one published case to the suite before publishing.');
      return;
    }
    try {
      const saved = await updateAssetDraft(selectedAsset.id, {
        draft_json: buildDraft(selectedAsset, nodes, suiteSetupNodes, suiteCleanupNodes, suiteCases, caseVariables, suiteVariables, datasetFormat, datasetText, inputSchemaText, resourceLocksText, lockWaitTimeoutText),
        expected_draft_version: selectedAsset.draft_version,
      });
      const nextVersion = saved.draft_version as number;
      setSelectedAsset({ ...selectedAsset, draft_version: nextVersion });
      const res = await publishAssetRevision(selectedAsset.id, {
        expected_draft_version: nextVersion,
        change_note: 'Validated and published revision',
      });
      setStatusMsg(`Published Revision v${res.version} (Checksum: ${res.checksum.slice(0, 8)}...)`);
      setTimeout(() => setStatusMsg(null), 4000);
      loadAssets();
    } catch (err) {
      setErrorMessage(`Publish failed: ${err instanceof Error ? err.message : 'Unknown error'}`);
    }
  };

  const addCaseToSuite = () => {
    const selected = publishedCaseRevisions.find((item) => item.revisionId === candidateRevisionId);
    if (!selected) return;
    setSuiteCases((current) => [
      ...current,
      { case_id: selected.assetId, revision_id: selected.revisionId, ordinal: current.length },
    ]);
    setErrorMessage(null);
  };

  const moveSuiteCase = (index: number, offset: number) => {
    const destination = index + offset;
    if (destination < 0 || destination >= suiteCases.length) return;
    setSuiteCases((current) => {
      const reordered = [...current];
      [reordered[index], reordered[destination]] = [reordered[destination], reordered[index]];
      return reordered.map((item, ordinal) => ({ ...item, ordinal }));
    });
  };

  const removeSuiteCase = (index: number) => {
    setSuiteCases((current) => current.filter((_, itemIndex) => itemIndex !== index).map((item, ordinal) => ({ ...item, ordinal })));
  };

  const handleTriggerRun = async () => {
    if (!selectedAsset || selectedAsset.kind !== 'suite' || !selectedEnvironmentId || !canRun) return;
    if (variableEditorError) { setErrorMessage(variableEditorError); return; }
    if (suiteCases.length === 0 || suiteCases.some((item) => !item.revision_id)) {
      setErrorMessage('Add published, pinned cases to this suite before running it.');
      return;
    }
    try {
      const parsedRunInputs = parseRunInputs(runInputsText);
      if (canEdit || hasInputSchemaContract) validateRunInputValues(inputSchemaText, parsedRunInputs);
      let revisionId = selectedRevisionId;
      if (canEdit) {
        const saved = await updateAssetDraft(selectedAsset.id, {
          draft_json: buildDraft(selectedAsset, nodes, suiteSetupNodes, suiteCleanupNodes, suiteCases, caseVariables, suiteVariables, datasetFormat, datasetText, inputSchemaText, resourceLocksText, lockWaitTimeoutText),
          expected_draft_version: selectedAsset.draft_version,
        });
        const nextVersion = saved.draft_version as number;
        setSelectedAsset({ ...selectedAsset, draft_version: nextVersion });

        // Publish the exact saved composition before creating the run.
        const pub = await publishAssetRevision(selectedAsset.id, {
          expected_draft_version: nextVersion,
          change_note: 'Automated publish before run',
        });
        revisionId = pub.revision_id;
      }
      if (!revisionId) throw new Error('This suite has no published revision yet.');

      const runRes = await triggerRun({
        suite_revision_id: revisionId,
        environment_id: selectedEnvironmentId,
        inputs: parsedRunInputs,
      });

      onRunStarted(runRes.run_id);
    } catch (err) {
      setErrorMessage(`Run failed: ${err instanceof Error ? err.message : 'Unknown error'}`);
    }
  };

  const handleExportBundle = async () => {
    setIsHandlingBundle(true);
    setErrorMessage(null);
    try {
      const blob = await downloadProjectBundle();
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = 'TestIT-project.zip';
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
      setStatusMsg('Project bundle downloaded');
      setTimeout(() => setStatusMsg(null), 3000);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to export the project bundle.');
    } finally {
      setIsHandlingBundle(false);
    }
  };

  const handleBundleSelected = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    setIsHandlingBundle(true);
    setErrorMessage(null);
    try {
      const preview = await previewProjectBundle(file);
      setBundleFile(file);
      setBundlePreview(preview);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to read this project bundle.');
      event.target.value = '';
    } finally {
      setIsHandlingBundle(false);
    }
  };

  const handleImportBundle = async () => {
    if (!bundleFile) return;
    setIsHandlingBundle(true);
    setErrorMessage(null);
    try {
      const result = await importProjectBundle(bundleFile);
      setBundlePreview(null);
      setBundleFile(null);
      if (bundleInputRef.current) bundleInputRef.current.value = '';
      setStatusMsg(`Imported ${result.asset_count} assets and ${result.connection_count} connections`);
      setTimeout(() => setStatusMsg(null), 5000);
      await loadAssets();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to import this project bundle.');
    } finally {
      setIsHandlingBundle(false);
    }
  };

  const updateNodeConfig = (key: string, value: any) => {
    if (!selectedNode) return;
    const updated = {
      ...selectedNode,
      config: { ...selectedNode.config, [key]: value },
    };
    updateSelectedNode(updated);
  };

  const updateWaitTarget = (target: 'api' | 'mysql' | 'mongodb') => {
    if (!selectedNode) return;
    const updated = {
      ...selectedNode,
      config: { ...selectedNode.config, target, connection_id: '' },
    };
    updateSelectedNode(updated);
  };

  const selectNode = (scope: 'case' | 'suite_setup' | 'suite_cleanup', node: NodeInstance | null) => {
    setSelectedNodeScope(scope);
    setSelectedNode(node);
  };

  const updateSelectedNode = (updated: NodeInstance) => {
    setSelectedNode(updated);
    const replace = (current: NodeInstance[]) => current.map((node) => node.id === updated.id ? updated : node);
    if (selectedNodeScope === 'suite_setup') setSuiteSetupNodes(replace);
    else if (selectedNodeScope === 'suite_cleanup') setSuiteCleanupNodes(replace);
    else setNodes(replace);
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
              Draft v{selectedAsset?.draft_version || 1} • {selectedAsset?.kind === 'suite' ? `${suiteCases.length} cases` : `${nodes.length} nodes`}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {canEdit && <>
            <input
              ref={bundleInputRef}
              type="file"
              accept=".zip,application/zip"
              className="sr-only"
              aria-label="Choose a TestIT project bundle to import"
              onChange={handleBundleSelected}
            />
            <button
              type="button"
              onClick={() => bundleInputRef.current?.click()}
              disabled={isHandlingBundle}
              className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
              title="Preview and import a project bundle"
            >
              <Upload className="w-3.5 h-3.5" /> Import
            </button>
            <button
              type="button"
              onClick={handleExportBundle}
              disabled={isHandlingBundle}
              className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
              title="Download a portable project bundle"
            >
              <Download className="w-3.5 h-3.5" /> Export
            </button>
          </>}
          {statusMsg && (
            <span className="text-xs text-emerald-400 font-medium px-3 py-1 bg-emerald-950/40 border border-emerald-800 rounded-lg flex items-center gap-1.5 animate-fadeIn">
              <CheckCircle2 className="w-3.5 h-3.5" /> {statusMsg}
            </span>
          )}

          {canRun && <label className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-slate-500">
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
          </label>}

          <button
            onClick={() => setShowPreviewModal(true)}
            disabled={!selectedNode}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Eye className="w-3.5 h-3.5 text-indigo-400" />
            Preview
          </button>
          {canEdit && <button
            onClick={handleSaveDraft}
            disabled={!selectedAsset}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Save className="w-3.5 h-3.5" />
            Save Draft
          </button>}
          {canEdit && <button
            onClick={handlePublish}
            disabled={!selectedAsset}
            className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Send className="w-3.5 h-3.5 text-violet-400" />
            Publish
          </button>}
          {canRun && <button
            onClick={handleTriggerRun}
            disabled={!selectedAsset || selectedAsset.kind !== 'suite' || !selectedEnvironmentId}
            title={selectedAsset?.kind !== 'suite' ? 'Only suite assets can be run.' : undefined}
            className="btn btn-primary text-xs py-1.5 px-3 flex items-center gap-1.5"
          >
            <Play className="w-3.5 h-3.5 fill-current" />
            Run Suite
          </button>}
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
              {canEdit && <button
                onClick={handleCreateAsset}
                aria-label="Create suite or test case"
                title="Create suite or test case"
                className="p-1 rounded-lg bg-indigo-600/20 text-indigo-400 hover:bg-indigo-600 hover:text-white"
              >
                <Plus className="w-4 h-4" />
              </button>}
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

        {/* Center: workflow editor or suite composer */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {selectedAsset?.kind === 'suite' ? (
            <section className="glass-panel flex flex-1 flex-col overflow-hidden p-6">
              <div className="mb-5 flex items-start justify-between border-b border-slate-800 pb-4">
                <div>
                  <h3 className="text-sm font-semibold text-slate-100">Suite composition</h3>
                  <p className="mt-1 text-xs text-slate-400">Choose published case revisions and order them for this suite.</p>
                </div>
                <span className="badge badge-queued">{suiteCases.length} pinned cases</span>
              </div>
              {canEdit && <div className="flex items-end gap-3 rounded-xl border border-slate-800 bg-slate-950/50 p-4">
                <label className="flex-1 text-xs font-semibold text-slate-300">
                  Published case revision
                  <select className="input-field mt-2 text-xs" value={candidateRevisionId} onChange={(event) => setCandidateRevisionId(event.target.value)}>
                    <option value="">Choose a published case</option>
                    {publishedCaseRevisions.map((item) => (
                      <option key={item.revisionId} value={item.revisionId}>{item.name} · v{item.version}</option>
                    ))}
                  </select>
                </label>
                      {canEdit && <button className="btn btn-secondary" onClick={addCaseToSuite} disabled={!candidateRevisionId} type="button">
                  <Plus className="h-4 w-4" /> Add case
                      </button>}
              </div>}
              {publishedCaseRevisions.length === 0 && (
                <p className="mt-4 rounded-lg border border-dashed border-slate-700 px-4 py-3 text-xs leading-5 text-slate-400">
                  Publish at least one test case before adding it to a suite.
                </p>
              )}
              <ol className="mt-4 flex-1 space-y-2 overflow-y-auto" aria-label="Cases in this suite">
                {suiteCases.map((caseRef, index) => {
                  const caseInfo = publishedCaseRevisions.find((item) => item.assetId === caseRef.case_id);
                  const isOutdated = Boolean(caseInfo && caseRef.revision_id && caseInfo.revisionId !== caseRef.revision_id);
                  return (
                    <li key={`${caseRef.case_id}-${caseRef.revision_id || index}`} className="flex items-center gap-3 rounded-xl border border-slate-800 bg-slate-900/60 p-3">
                      <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-slate-950 text-xs font-mono text-slate-400">{index + 1}</span>
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-xs font-semibold text-slate-200">{caseInfo?.name || `Missing case ${caseRef.case_id}`}</div>
                        <div className="mt-1 text-[10px] font-mono text-slate-500">Pinned revision {caseRef.revision_id || 'latest'}</div>
                        {isOutdated && caseInfo && <div className="mt-1 text-[10px] font-medium text-amber-300">A newer revision is available (v{caseInfo.version})</div>}
                      </div>
                      {canEdit && <>
                        <button className="btn btn-secondary !p-2" type="button" aria-label={`Move case ${index + 1} up`} disabled={index === 0} onClick={() => moveSuiteCase(index, -1)}><ArrowUp className="h-3.5 w-3.5" /></button>
                        <button className="btn btn-secondary !p-2" type="button" aria-label={`Move case ${index + 1} down`} disabled={index === suiteCases.length - 1} onClick={() => moveSuiteCase(index, 1)}><ArrowDown className="h-3.5 w-3.5" /></button>
                        <button className="btn btn-secondary !p-2 text-rose-300" type="button" aria-label={`Remove case ${index + 1}`} onClick={() => removeSuiteCase(index)}><X className="h-3.5 w-3.5" /></button>
                      </>}
                    </li>
                  );
                })}
              </ol>
              {canEdit && <VariableDefinitionsEditor
                key={`${selectedAsset?.id}-suite-variables`}
                title="Suite variables"
                value={suiteVariables}
                onCommit={setSuiteVariables}
                onValidationChange={setVariableEditorError}
              />}
              <section className="space-y-3 rounded-xl border border-slate-800 bg-slate-950/50 p-3">
                <div>
                  <h4 className="text-xs font-semibold text-slate-200">Suite lifecycle hooks</h4>
                  <p className="mt-1 text-[10px] leading-4 text-slate-500">Setup runs once before case iterations. Cleanup runs once afterward, including when setup, a case, or cancellation ends the main run.</p>
                </div>
                <details className="rounded-lg border border-slate-800 bg-slate-950/70 p-3">
                  <summary className="cursor-pointer text-xs font-semibold text-slate-300">Suite setup · {suiteSetupNodes.length} steps</summary>
                  <div className="mt-3 min-h-64">
                    <WorkflowCanvas
                      nodes={suiteSetupNodes}
                      onChange={setSuiteSetupNodes}
                      onSelectNode={(node) => selectNode('suite_setup', node)}
                      selectedNodeId={selectedNodeScope === 'suite_setup' ? selectedNode?.id || null : null}
                      readOnly={!canEdit}
                      hidePhase
                    />
                  </div>
                </details>
                <details className="rounded-lg border border-slate-800 bg-slate-950/70 p-3">
                  <summary className="cursor-pointer text-xs font-semibold text-slate-300">Suite cleanup · {suiteCleanupNodes.length} steps</summary>
                  <div className="mt-3 min-h-64">
                    <WorkflowCanvas
                      nodes={suiteCleanupNodes}
                      onChange={setSuiteCleanupNodes}
                      onSelectNode={(node) => selectNode('suite_cleanup', node)}
                      selectedNodeId={selectedNodeScope === 'suite_cleanup' ? selectedNode?.id || null : null}
                      readOnly={!canEdit}
                      hidePhase
                    />
                  </div>
                </details>
              </section>
              <div className="grid grid-cols-[1fr_180px] gap-3 rounded-xl border border-slate-800 bg-slate-950/50 p-3">
                <label className="block text-xs font-semibold text-slate-300">Exclusive resource locks (one per line)
                  <textarea
                    rows={3}
                    value={resourceLocksText}
                    disabled={!canEdit}
                    onChange={(event) => setResourceLocksText(event.target.value)}
                    className="input-field mt-2 resize-y font-mono text-[11px] disabled:opacity-70"
                    placeholder={'env:staging\ntenant:demo-account'}
                  />
                  <span className="mt-1 block text-[10px] font-normal text-slate-500">Names are exclusive within this workspace (case-insensitive) and stay held until the run finishes cleanup.</span>
                </label>
                <label className="block text-xs font-semibold text-slate-300">Lock wait limit (seconds)
                  <input
                    type="number"
                    min={1}
                    max={3600}
                    value={lockWaitTimeoutText}
                    disabled={!canEdit}
                    onChange={(event) => setLockWaitTimeoutText(event.target.value)}
                    className="input-field mt-2 text-xs font-mono disabled:opacity-70"
                  />
                  <span className="mt-1 block text-[10px] font-normal text-slate-500">A run that cannot acquire its locks before this deadline ends with ERROR.</span>
                </label>
              </div>
              <label className="block rounded-xl border border-slate-800 bg-slate-950/50 p-3 text-xs font-semibold text-slate-300">Declared run inputs (JSON object)
                <textarea rows={5} value={inputSchemaText} disabled={!canEdit} onChange={(event) => { setInputSchemaText(event.target.value); setHasInputSchemaContract(true); }} className="input-field mt-2 resize-y font-mono text-[11px] disabled:opacity-70" placeholder={'{\n  "customer_id": { "type": "integer", "required": true }\n}'} />
                <span className="mt-1 block text-[10px] font-normal text-slate-500">Declare input name, type, and required flag. Undeclared or mismatched values are rejected before a run starts.</span>
              </label>
              {canRun && <label className="mt-4 block rounded-xl border border-slate-800 bg-slate-950/50 p-3 text-xs font-semibold text-slate-300">Run inputs (JSON object)
                <textarea rows={4} value={runInputsText} onChange={(event) => setRunInputsText(event.target.value)} className="input-field mt-2 resize-y font-mono text-[11px]" placeholder={'{\n  "customer_id": 1042\n}'} />
                <span className="mt-1 block text-[10px] font-normal text-slate-500">Reference values with {'{{run.customer_id}}'}. Inputs are bounded and cannot contain credential fields.</span>
              </label>}
              <p className="mt-4 text-[11px] leading-5 text-slate-500">Each suite entry keeps its selected case revision. Publishing a newer case does not change this suite.</p>
            </section>
          ) : selectedAsset ? (
            <WorkflowCanvas
              nodes={nodes}
              onChange={setNodes}
              onSelectNode={(node) => selectNode('case', node)}
              selectedNodeId={selectedNodeScope === 'case' ? selectedNode?.id || null : null}
              readOnly={!canEdit}
            />
          ) : (
            <section className="glass-panel flex flex-1 flex-col items-center justify-center px-10 text-center">
              <div className="mb-5 flex h-14 w-14 items-center justify-center rounded-2xl border border-indigo-400/20 bg-indigo-400/10 text-indigo-300 shadow-glow">
                <FolderGit2 className="h-6 w-6" />
              </div>
              <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-indigo-300">Start with a suite or test case</p>
              <h3 className="max-w-lg text-2xl font-semibold tracking-tight text-slate-100">Turn a backend check into a repeatable workflow.</h3>
              <p className="mt-3 max-w-md text-sm leading-6 text-slate-400">Create a case, add API and data checks, then publish a version for a run.</p>
              {canEdit && <button onClick={handleCreateAsset} className="btn btn-primary mt-6 px-4 py-2.5">
                <Plus className="h-4 w-4" /> Create workflow asset
              </button>}
            </section>
          )}
        </div>

        {/* Right Side: Node Configuration Inspector */}
        {selectedNode && (
          <fieldset disabled={!canEdit} className="w-80 glass-panel p-5 shrink-0 overflow-y-auto disabled:opacity-90">
            <div className="space-y-4">
              <div className="pb-3 border-b border-slate-800">
                <span className="text-[10px] font-mono text-indigo-400 uppercase tracking-wider">{canEdit ? 'Configure step' : 'Step details'}</span>
                <p className="text-xs font-mono text-slate-500 mt-1">{selectedNode.type}</p>
                <label className="mt-3 block text-xs font-semibold text-slate-300">Step name
                  <input value={selectedNode.name} maxLength={128} onChange={(event) => {
                    const updated = { ...selectedNode, name: event.target.value };
                    updateSelectedNode(updated);
                  }} className="input-field mt-1.5 text-xs" />
                </label>
                {selectedNodeScope === 'case' && <label className="mt-3 block text-xs font-semibold text-slate-300">Execution phase
                  <select value={selectedNode.phase || 'main'} onChange={(event) => {
                    const updated = { ...selectedNode, phase: event.target.value as 'setup' | 'main' | 'cleanup' };
                    updateSelectedNode(updated);
                  }} className="input-field mt-1.5 text-xs">
                    <option value="setup">Setup · runs before main steps</option>
                    <option value="main">Main · skipped if setup fails</option>
                    <option value="cleanup">Cleanup · runs after failures and cancellation</option>
                  </select>
                </label>}
              </div>

              {['api.request', 'wait.until', 'db.mysql', 'db.mongodb', 'data.tabular'].includes(selectedNode.type) && (
                <label className="block text-xs font-semibold text-slate-300">Connection profile
                  <select value={selectedNode.config.connection_id || ''} onChange={(event) => updateNodeConfig('connection_id', event.target.value)} className="input-field mt-1.5 text-xs">
                    <option value="">Select a profile</option>
                    {connections.filter((connection) => {
                      const allowed = selectedNode.type === 'wait.until'
                        ? (selectedNode.config.target || 'api') === 'api' ? ['api', 'http'] : [selectedNode.config.target]
                        : selectedNode.type === 'api.request' ? ['api', 'http'] : selectedNode.type === 'db.mysql' ? ['mysql'] : selectedNode.type === 'db.mongodb' ? ['mongodb'] : ['parquet', 'delta'];
                      return allowed.includes(connection.connector_type);
                    }).map((connection) => <option key={connection.id} value={connection.id}>{connection.name} · {connection.connector_type}</option>)}
                  </select>
                  {canEdit && <span className="mt-1 block text-[10px] font-normal text-slate-500">Connection profiles and encrypted credentials are managed by an administrator.</span>}
                </label>
              )}

              {selectedNode.type === 'wait.until' && <label className="block text-xs font-semibold text-slate-300">Condition source
                <select value={selectedNode.config.target || 'api'} onChange={(event) => updateWaitTarget(event.target.value as 'api' | 'mysql' | 'mongodb')} className="input-field mt-1.5 text-xs">
                  <option value="api">HTTP API response</option>
                  <option value="mysql">MySQL query row count</option>
                  <option value="mongodb">MongoDB document count</option>
                </select>
              </label>}

              {(selectedNode.type === 'api.request' || (selectedNode.type === 'wait.until' && (selectedNode.config.target || 'api') === 'api')) && <>
                <label className="block text-xs font-semibold text-slate-300">HTTP method
                  <select value={selectedNode.config.method || 'GET'} onChange={(event) => updateNodeConfig('method', event.target.value)} className="input-field mt-1.5 text-xs font-mono">
                    {(selectedNode.type === 'wait.until' ? ['GET', 'HEAD', 'OPTIONS'] : ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']).map((method) => <option key={method}>{method}</option>)}
                  </select>
                </label>
                <label className="block text-xs font-semibold text-slate-300">Request URL or path
                  <input value={selectedNode.config.url || selectedNode.config.path || ''} onChange={(event) => updateNodeConfig(selectedNode.config.url ? 'url' : 'path', event.target.value)} className="input-field mt-1.5 text-xs font-mono" placeholder="/v1/health or a full URL" />
                </label>
                <label className="block text-xs font-semibold text-slate-300">Expected status
                  <input type="number" min={100} max={599} value={selectedNode.config.expected_status ?? 200} onChange={(event) => updateNodeConfig('expected_status', Number(event.target.value))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <RequestHeadersEditor value={selectedNode.config.headers || {}} onCommit={(value) => updateNodeConfig('headers', value)} />
                {selectedNode.type === 'api.request' && <ConfigJsonInput label="Request body (JSON)" value={selectedNode.config.body ?? {}} onCommit={(value) => updateNodeConfig('body', value)} />}
                <ApiAssertionsEditor value={selectedNode.config.assertions || []} onCommit={(value) => updateNodeConfig('assertions', value)} />
                {selectedNode.type === 'wait.until' && <>
                  <p className="rounded-lg border border-purple-900/60 bg-purple-950/20 p-2 text-[10px] leading-4 text-purple-200">This node repeats an idempotent API request until its status and assertions pass or the step timeout is reached.</p>
                  <label className="block text-xs font-semibold text-slate-300">Poll interval (seconds)
                    <input type="number" min={1} max={60} value={selectedNode.config.poll_interval_seconds ?? 2} onChange={(event) => updateNodeConfig('poll_interval_seconds', Math.min(60, Math.max(1, Number(event.target.value))))} className="input-field mt-1.5 text-xs font-mono" />
                  </label>
                  <label className="block text-xs font-semibold text-slate-300">Request timeout per poll (seconds)
                    <input type="number" min={1} max={60} value={selectedNode.config.request_timeout_seconds ?? 5} onChange={(event) => updateNodeConfig('request_timeout_seconds', Math.min(60, Math.max(1, Number(event.target.value))))} className="input-field mt-1.5 text-xs font-mono" />
                  </label>
                </>}
                <label className="block text-xs font-semibold text-slate-300">Maximum response time (ms)
                  <input type="number" min={1} value={selectedNode.config.max_response_time_ms ?? ''} onChange={(event) => updateNodeConfig('max_response_time_ms', event.target.value ? Number(event.target.value) : undefined)} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                {selectedNode.type === 'api.request' && <ConfigJsonInput label="Selected response fields (JSON)" value={selectedNode.config.extract || {}} onCommit={(value) => updateNodeConfig('extract', value)} />}
              </>}

              {selectedNode.type === 'wait.until' && selectedNode.config.target === 'mysql' && <>
                <label className="block text-xs font-semibold text-slate-300">Read-only SQL query
                  <textarea rows={5} value={selectedNode.config.query || ''} onChange={(event) => updateNodeConfig('query', event.target.value)} className="input-field mt-1.5 resize-y font-mono text-xs" placeholder="SELECT id FROM orders WHERE status = %s" />
                </label>
                <ConfigJsonInput label="Bound query parameters (JSON array)" value={selectedNode.config.params || []} onCommit={(value) => updateNodeConfig('params', value)} />
                <label className="block text-xs font-semibold text-slate-300">Minimum rows expected
                  <input type="number" min={0} max={500} value={selectedNode.config.expected_min_rows ?? 1} onChange={(event) => updateNodeConfig('expected_min_rows', Number(event.target.value))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <p className="text-[10px] leading-4 text-slate-500">Values stay bound parameters. Each poll reports only the row count, never row contents.</p>
                <label className="block text-xs font-semibold text-slate-300">Poll interval (seconds)
                  <input type="number" min={1} max={60} value={selectedNode.config.poll_interval_seconds ?? 2} onChange={(event) => updateNodeConfig('poll_interval_seconds', Math.min(60, Math.max(1, Number(event.target.value))))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <label className="block text-xs font-semibold text-slate-300">Query timeout per poll (seconds)
                  <input type="number" min={1} max={60} value={selectedNode.config.request_timeout_seconds ?? 5} onChange={(event) => updateNodeConfig('request_timeout_seconds', Math.min(60, Math.max(1, Number(event.target.value))))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
              </>}

              {selectedNode.type === 'wait.until' && selectedNode.config.target === 'mongodb' && <>
                <label className="block text-xs font-semibold text-slate-300">Collection
                  <input value={selectedNode.config.collection || ''} onChange={(event) => updateNodeConfig('collection', event.target.value)} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <ConfigJsonInput label="Read filter (JSON)" value={selectedNode.config.filter || {}} onCommit={(value) => updateNodeConfig('filter', value)} />
                <label className="block text-xs font-semibold text-slate-300">Minimum documents expected
                  <input type="number" min={0} max={500} value={selectedNode.config.expected_min_count ?? 1} onChange={(event) => updateNodeConfig('expected_min_count', Number(event.target.value))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <p className="text-[10px] leading-4 text-slate-500">Each poll reports only the document count, never document contents.</p>
                <label className="block text-xs font-semibold text-slate-300">Poll interval (seconds)
                  <input type="number" min={1} max={60} value={selectedNode.config.poll_interval_seconds ?? 2} onChange={(event) => updateNodeConfig('poll_interval_seconds', Math.min(60, Math.max(1, Number(event.target.value))))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <label className="block text-xs font-semibold text-slate-300">Query timeout per poll (seconds)
                  <input type="number" min={1} max={60} value={selectedNode.config.request_timeout_seconds ?? 5} onChange={(event) => updateNodeConfig('request_timeout_seconds', Math.min(60, Math.max(1, Number(event.target.value))))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
              </>}

              {selectedNode.type === 'db.mysql' && <>
                <label className="block text-xs font-semibold text-slate-300">Read-only SQL query
                  <textarea rows={5} value={selectedNode.config.query || ''} onChange={(event) => updateNodeConfig('query', event.target.value)} className="input-field mt-1.5 resize-y font-mono text-xs" placeholder="SELECT id FROM customers LIMIT 100" />
                </label>
                <ConfigJsonInput label="Bound query parameters (JSON array)" value={selectedNode.config.params || []} onCommit={(value) => updateNodeConfig('params', value)} />
                <p className="-mt-2 text-[10px] leading-4 text-slate-500">Use %s placeholders in SQL; variables resolve inside this parameter array and remain bound values.</p>
                <label className="block text-xs font-semibold text-slate-300">Minimum rows expected
                  <input type="number" min={0} max={500} value={selectedNode.config.expected_min_rows ?? 1} onChange={(event) => updateNodeConfig('expected_min_rows', Number(event.target.value))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <ConfigJsonInput label="Bounded output columns (JSON array)" value={selectedNode.config.output_columns || []} onCommit={(value) => updateNodeConfig('output_columns', value)} />
              </>}

              {selectedNode.type === 'db.mongodb' && <>
                <label className="block text-xs font-semibold text-slate-300">Collection
                  <input value={selectedNode.config.collection || ''} onChange={(event) => updateNodeConfig('collection', event.target.value)} className="input-field mt-1.5 text-xs font-mono" />
                </label>
                <ConfigJsonInput label="Read filter (JSON)" value={selectedNode.config.filter || {}} onCommit={(value) => updateNodeConfig('filter', value)} />
                <label className="block text-xs font-semibold text-slate-300">Minimum documents expected
                  <input type="number" min={0} max={500} value={selectedNode.config.expected_min_count ?? 1} onChange={(event) => updateNodeConfig('expected_min_count', Number(event.target.value))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
              </>}

              {selectedNode.type === 'data.tabular' && <>
                <label className="block text-xs font-semibold text-slate-300">Data file or table path
                  <input value={selectedNode.config.path || ''} onChange={(event) => updateNodeConfig('path', event.target.value)} className="input-field mt-1.5 text-xs font-mono" placeholder="s3://bucket/data.parquet" />
                </label>
                <label className="block text-xs font-semibold text-slate-300">Minimum rows expected
                  <input type="number" min={0} value={selectedNode.config.expected_min_rows ?? 1} onChange={(event) => updateNodeConfig('expected_min_rows', Number(event.target.value))} className="input-field mt-1.5 text-xs font-mono" />
                </label>
              </>}

              {selectedNode.type === 'sleep.wait' && <label className="block text-xs font-semibold text-slate-300">Wait duration (seconds)
                <input type="number" min={1} max={60} value={selectedNode.config.duration_seconds ?? 2} onChange={(event) => updateNodeConfig('duration_seconds', Math.min(60, Number(event.target.value)))} className="input-field mt-1.5 text-xs font-mono" />
              </label>}

              <label className="block text-xs font-semibold text-slate-300">Step timeout (seconds)
                <input type="number" min={1} max={3600} value={selectedNode.timeout_seconds || 30} onChange={(event) => {
                  const updated = { ...selectedNode, timeout_seconds: Number(event.target.value) };
                  updateSelectedNode(updated);
                }} className="input-field mt-1.5 text-xs font-mono" />
              </label>
              {selectedAsset?.kind === 'case' && <section className="border-t border-slate-800 pt-4">
                <h3 className="text-xs font-semibold text-slate-200">Data-driven iterations</h3>
                <p className="mt-1 text-[10px] leading-4 text-slate-500">Each row runs as its own reported case. Reference row fields with {'{{iteration.field}}'}. Maximum 100 rows and 1 MiB.</p>
                <label className="mt-3 block text-xs font-semibold text-slate-300">Format
                  <select value={datasetFormat} onChange={(event) => setDatasetFormat(event.target.value as 'json' | 'csv')} className="input-field mt-1.5 text-xs">
                    <option value="json">JSON rows</option>
                    <option value="csv">CSV</option>
                  </select>
                </label>
                <label className="mt-3 block text-xs font-semibold text-slate-300">{datasetFormat === 'json' ? 'Rows (JSON array of objects)' : 'CSV content with a header row'}
                  <textarea
                    rows={9}
                    value={datasetText}
                    onChange={(event) => setDatasetText(event.target.value)}
                    className="input-field mt-1.5 resize-y font-mono text-[11px]"
                    placeholder={datasetFormat === 'json' ? '[\n  { "user_id": 101 },\n  { "user_id": 102 }\n]' : 'user_id,email\n101,one@example.test\n102,two@example.test'}
                  />
                </label>
                <p className="mt-1 text-[10px] text-slate-500">Credential fields such as passwords, tokens, and secrets are rejected. Leave empty for one case run.</p>
              </section>}
              {selectedAsset?.kind === 'case' && <VariableDefinitionsEditor
                key={`${selectedAsset.id}-case-variables`}
                title="Case variables"
                value={caseVariables}
                onCommit={setCaseVariables}
                onValidationChange={setVariableEditorError}
              />}
            </div>
          </fieldset>
        )}
      </div>

      {showCreateDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6 backdrop-blur-sm">
          <section role="dialog" aria-modal="true" aria-labelledby="create-test-case-title" className="glass-panel w-full max-w-lg p-6 shadow-2xl">
            <div className="mb-5 flex items-start justify-between">
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-indigo-300">New workflow</p>
                <h2 id="create-test-case-title" className="mt-1 text-lg font-semibold text-slate-100">Create an asset</h2>
                <p className="mt-1 text-sm text-slate-400">Create a reusable test case or a suite that groups published cases.</p>
              </div>
              <button onClick={() => setShowCreateDialog(false)} className="rounded-lg p-2 text-slate-400 hover:bg-slate-800 hover:text-white" aria-label="Close dialog">
                <span aria-hidden="true">×</span>
              </button>
            </div>
            <form onSubmit={submitCreateAsset}>
              <label className="mb-4 block text-xs font-semibold text-slate-300">
                Asset type
                <select className="input-field mt-2" value={newAssetKind} onChange={(event) => setNewAssetKind(event.target.value as 'case' | 'suite')}>
                  <option value="case">Reusable test case</option>
                  <option value="suite">Suite</option>
                </select>
              </label>
              <label htmlFor="new-test-case-name" className="mb-1.5 block text-xs font-semibold text-slate-300">{newAssetKind === 'suite' ? 'Suite name' : 'Test case name'}</label>
              <input
                id="new-test-case-name"
                autoFocus
                value={newAssetName}
                onChange={(event) => setNewAssetName(event.target.value)}
                className="input-field"
                placeholder={newAssetKind === 'suite' ? 'e.g. Customer API regression' : 'e.g. Customer API health check'}
                maxLength={120}
                required
              />
              <div className="mt-5 flex justify-end gap-2">
                <button type="button" onClick={() => setShowCreateDialog(false)} className="btn btn-secondary">Cancel</button>
                <button type="submit" disabled={isCreatingAsset || !newAssetName.trim()} className="btn btn-primary">
                  {isCreatingAsset ? 'Creating…' : `Create ${newAssetKind === 'suite' ? 'suite' : 'test case'}`}
                </button>
              </div>
            </form>
          </section>
        </div>
      )}

      {bundlePreview && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6 backdrop-blur-sm">
          <section role="dialog" aria-modal="true" aria-labelledby="bundle-preview-title" className="glass-panel w-full max-w-2xl p-6 shadow-2xl">
            <div className="mb-5 flex items-start justify-between">
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-[0.18em] text-indigo-300">Project portability</p>
                <h2 id="bundle-preview-title" className="mt-1 text-lg font-semibold text-slate-100">Review bundle contents</h2>
                <p className="mt-1 text-sm text-slate-400">Import creates new copies and keeps the revisions from this archive.</p>
              </div>
              <button
                type="button"
                onClick={() => { setBundlePreview(null); setBundleFile(null); if (bundleInputRef.current) bundleInputRef.current.value = ''; }}
                className="rounded-lg p-2 text-slate-400 hover:bg-slate-800 hover:text-white"
                aria-label="Close bundle preview"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="grid grid-cols-3 gap-3">
              <div className="rounded-xl border border-slate-800 bg-slate-950/50 p-3"><p className="text-[10px] uppercase tracking-wider text-slate-500">Assets</p><p className="mt-1 text-xl font-semibold text-slate-100">{bundlePreview.asset_count}</p></div>
              <div className="rounded-xl border border-slate-800 bg-slate-950/50 p-3"><p className="text-[10px] uppercase tracking-wider text-slate-500">Published revisions</p><p className="mt-1 text-xl font-semibold text-slate-100">{bundlePreview.revision_count}</p></div>
              <div className="rounded-xl border border-slate-800 bg-slate-950/50 p-3"><p className="text-[10px] uppercase tracking-wider text-slate-500">Connections</p><p className="mt-1 text-xl font-semibold text-slate-100">{bundlePreview.connection_count}</p></div>
            </div>
            <div className="mt-4 max-h-64 space-y-2 overflow-y-auto pr-1">
              {bundlePreview.assets?.map((asset: any, index: number) => (
                <div key={`${asset.name}-${index}`} className="flex items-center justify-between rounded-lg border border-slate-800/80 bg-slate-950/40 px-3 py-2.5">
                  <div className="min-w-0"><p className="truncate text-sm font-medium text-slate-200">{asset.name}</p><p className="mt-0.5 text-[10px] uppercase tracking-wider text-slate-500">{asset.kind}</p></div>
                  <span className="ml-3 shrink-0 rounded-md bg-slate-800 px-2 py-1 text-[10px] text-slate-300">{asset.revisions} revisions</span>
                </div>
              ))}
              {bundlePreview.connections?.map((connection: any, index: number) => (
                <div key={`${connection.name}-${index}`} className="flex items-center justify-between rounded-lg border border-slate-800/80 bg-slate-950/40 px-3 py-2.5">
                  <div className="min-w-0"><p className="truncate text-sm font-medium text-slate-200">{connection.name}</p><p className="mt-0.5 text-[10px] uppercase tracking-wider text-slate-500">{connection.connector_type} connection</p></div>
                  {connection.secrets_need_reentry && <span className="ml-3 shrink-0 rounded-md bg-amber-950/60 px-2 py-1 text-[10px] text-amber-300">Credentials needed</span>}
                </div>
              ))}
              {bundlePreview.asset_count === 0 && bundlePreview.connection_count === 0 && <p className="rounded-lg border border-dashed border-slate-700 p-4 text-sm text-slate-400">This bundle contains no assets or connections.</p>}
            </div>
            {bundlePreview.conflicts?.length > 0 && <p className="mt-4 rounded-lg border border-indigo-900/70 bg-indigo-950/30 px-3 py-2 text-xs leading-5 text-indigo-200">{bundlePreview.conflicts.length} existing name{bundlePreview.conflicts.length === 1 ? '' : 's'} match this bundle. Imported copies will receive unique names.</p>}
            {(bundlePreview.connections || []).some((connection: any) => connection.secrets_need_reentry) && <p className="mt-3 rounded-lg border border-amber-900/70 bg-amber-950/30 px-3 py-2 text-xs leading-5 text-amber-200">Secret values are never included. Add them under Encrypted Secrets, then choose the new secret names in each imported profile’s Secret References before testing the connection.</p>}
            <div className="mt-6 flex justify-end gap-2">
              <button type="button" onClick={() => { setBundlePreview(null); setBundleFile(null); if (bundleInputRef.current) bundleInputRef.current.value = ''; }} className="btn btn-secondary" disabled={isHandlingBundle}>Cancel</button>
              <button type="button" onClick={handleImportBundle} disabled={isHandlingBundle || !bundleFile || (bundlePreview.asset_count === 0 && bundlePreview.connection_count === 0)} className="btn btn-primary flex items-center gap-2">
                <Upload className="h-4 w-4" />{isHandlingBundle ? 'Importing…' : 'Import as new copies'}
              </button>
            </div>
          </section>
        </div>
      )}

      {showPreviewModal && selectedNode && (
        <VariablePreviewModal
          nodeConfig={selectedNode.config}
          environmentId={selectedEnvironmentId || undefined}
          suiteVariables={suiteVariables}
          caseVariables={caseVariables}
          onClose={() => setShowPreviewModal(false)}
        />
      )}
    </div>
  );
};

function buildDraft(
  asset: Asset,
  nodes: NodeInstance[],
  suiteSetupNodes: NodeInstance[],
  suiteCleanupNodes: NodeInstance[],
  suiteCases: Array<{ case_id: string; revision_id?: string; ordinal: number }>,
  caseVariables: Record<string, any>,
  suiteVariables: Record<string, any>,
  datasetFormat: 'json' | 'csv',
  datasetText: string,
  inputSchemaText: string,
  resourceLocksText: string,
  lockWaitTimeoutText: string,
) {
  if (asset.kind === 'suite') {
    const inputSchema = parseRunInputSchema(inputSchemaText);
    return {
      id: asset.id,
      name: asset.name,
      execution_mode: 'sequential',
      variables: suiteVariables,
      input_schema: inputSchema,
      setup_nodes: suiteSetupNodes,
      cleanup_nodes: suiteCleanupNodes,
      resource_locks: parseResourceLocks(resourceLocksText),
      lock_wait_timeout_seconds: parseLockWaitTimeout(lockWaitTimeoutText),
      cases: suiteCases.map((item, ordinal) => ({ ...item, ordinal })),
    };
  }
  let data_set: { format: 'json'; rows: unknown[] } | { format: 'csv'; content: string } | undefined;
  if (datasetText.trim()) {
    if (new TextEncoder().encode(datasetText).length > 1_048_576) {
      throw new Error('The data set exceeds the 1 MiB size limit.');
    }
    if (datasetFormat === 'json') {
      let rows: unknown;
      try { rows = JSON.parse(datasetText); } catch { throw new Error('Data-set JSON is invalid.'); }
      if (!Array.isArray(rows) || rows.length < 1 || rows.length > 100 || rows.some((row) => row === null || Array.isArray(row) || typeof row !== 'object')) {
        throw new Error('JSON data sets require 1–100 object rows.');
      }
      data_set = { format: 'json', rows };
    } else {
      data_set = { format: 'csv', content: datasetText };
    }
  }
  return {
    id: asset.id,
    name: asset.name,
    variables: caseVariables,
    nodes,
    ...(data_set ? { data_set } : {}),
    edges: nodes.slice(1).map((node, index) => ({
      id: `${nodes[index].id}-${node.id}`,
      source: nodes[index].id,
      target: node.id,
    })),
  };
}

function parseResourceLocks(text: string): string[] {
  const names = text.split(/\r?\n/).map((name) => name.trim()).filter(Boolean);
  if (names.length > 32) throw new Error('A suite can declare at most 32 resource locks.');
  const seen = new Set<string>();
  for (const name of names) {
    if (name.length > 128 || !/^[A-Za-z0-9_.:-]+$/.test(name)) {
      throw new Error('Resource lock names may contain letters, digits, underscores, hyphens, periods, and colons.');
    }
    const normalized = name.toLowerCase();
    if (seen.has(normalized)) throw new Error('Resource lock names must be unique.');
    seen.add(normalized);
  }
  return names;
}

function parseLockWaitTimeout(text: string): number {
  const value = Number(text);
  if (!Number.isInteger(value) || value < 1 || value > 3600) {
    throw new Error('Lock wait limit must be a whole number from 1 to 3600 seconds.');
  }
  return value;
}

function parseRunInputs(value: string): Record<string, unknown> {
  let parsed: unknown;
  try { parsed = JSON.parse(value); } catch { throw new Error('Run inputs must be valid JSON.'); }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') throw new Error('Run inputs must be a JSON object.');
  if (new TextEncoder().encode(JSON.stringify(parsed)).length > 65_536) throw new Error('Run inputs exceed the 64 KiB limit.');
  const serialized = JSON.stringify(parsed);
  if (/"[^"\\]*(?:password|secret|token|authorization|cookie|credential)[^"\\]*"\s*:/i.test(serialized)) {
    throw new Error('Run inputs cannot contain credential fields.');
  }
  return parsed as Record<string, unknown>;
}

function parseRunInputSchema(value: string): Record<string, { type: string; required?: boolean }> {
  let parsed: unknown;
  try { parsed = JSON.parse(value); } catch { throw new Error('Run input declarations must be valid JSON.'); }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') throw new Error('Run input declarations must be a JSON object.');
  if (new TextEncoder().encode(JSON.stringify(parsed)).length > 65_536) throw new Error('Run input declarations exceed 64 KiB.');
  const fields = parsed as Record<string, unknown>;
  if (Object.keys(fields).length > 100) throw new Error('Declare no more than 100 run inputs.');
  const supportedTypes = ['string', 'integer', 'decimal', 'number', 'boolean', 'object', 'array', 'datetime'];
  for (const [name, definition] of Object.entries(fields)) {
    if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name)) throw new Error(`“${name}” is not a valid run input name.`);
    if (!definition || Array.isArray(definition) || typeof definition !== 'object') throw new Error(`Run input “${name}” needs a type definition.`);
    const candidate = definition as Record<string, unknown>;
    if (typeof candidate.type !== 'string' || !supportedTypes.includes(candidate.type)) throw new Error(`Run input “${name}” needs a supported type.`);
    if (candidate.required !== undefined && typeof candidate.required !== 'boolean') throw new Error(`Run input “${name}” required flag must be true or false.`);
  }
  return fields as Record<string, { type: string; required?: boolean }>;
}

function validateRunInputValues(schemaText: string, values: Record<string, unknown>) {
  const schema = parseRunInputSchema(schemaText);
  for (const name of Object.keys(values)) {
    if (!Object.prototype.hasOwnProperty.call(schema, name)) throw new Error(`Run input “${name}” is not declared by this suite.`);
  }
  for (const [name, definition] of Object.entries(schema)) {
    if (!Object.prototype.hasOwnProperty.call(values, name)) {
      if (definition.required) throw new Error(`Required run input “${name}” is missing.`);
      continue;
    }
    const value = values[name];
    const valid = definition.type === 'string' || definition.type === 'datetime'
      ? typeof value === 'string'
      : definition.type === 'integer'
        ? typeof value === 'number' && Number.isInteger(value)
        : definition.type === 'decimal' || definition.type === 'number'
          ? typeof value === 'number' && Number.isFinite(value)
          : definition.type === 'boolean'
            ? typeof value === 'boolean'
            : definition.type === 'array'
              ? Array.isArray(value)
              : definition.type === 'object'
                ? Boolean(value) && !Array.isArray(value) && typeof value === 'object'
                : false;
    if (!valid) throw new Error(`Run input “${name}” does not match type “${definition.type}”.`);
  }
}

interface ConfigJsonInputProps {
  label: string;
  value: unknown;
  onCommit: (value: any) => void;
}

const ConfigJsonInput: React.FC<ConfigJsonInputProps> = ({ label, value, onCommit }) => {
  const serialized = JSON.stringify(value ?? {}, null, 2);
  const [text, setText] = useState(serialized);
  const [valid, setValid] = useState(true);
  useEffect(() => {
    setText(serialized);
    setValid(true);
  }, [serialized]);
  return (
    <label className="block text-xs font-semibold text-slate-300">
      {label}
      <textarea
        rows={5}
        spellCheck={false}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onBlur={() => {
          try {
            onCommit(JSON.parse(text));
            setValid(true);
          } catch {
            setValid(false);
          }
        }}
        className="input-field mt-1.5 resize-y font-mono text-[11px]"
      />
      {!valid && <span role="alert" className="mt-1 block text-[10px] font-normal text-rose-300">Enter valid JSON before leaving this field.</span>}
    </label>
  );
};

interface RequestHeadersEditorProps {
  value: Record<string, string>;
  onCommit: (value: Record<string, string>) => void;
}

const RequestHeadersEditor: React.FC<RequestHeadersEditorProps> = ({ value, onCommit }) => {
  const [rows, setRows] = useState<Array<{ name: string; value: string }>>([]);
  useEffect(() => {
    setRows(Object.entries(value || {}).map(([name, content]) => ({ name, value: String(content) })));
  }, [JSON.stringify(value)]);
  const commit = (next: Array<{ name: string; value: string }>) => {
    setRows(next);
    onCommit(Object.fromEntries(next.filter((row) => row.name.trim()).map((row) => [row.name.trim(), row.value])));
  };
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-slate-300">Request headers</span>
        <button type="button" onClick={() => commit([...rows, { name: '', value: '' }])} className="text-[10px] font-semibold text-indigo-300 hover:text-indigo-200">Add header</button>
      </div>
      {rows.map((row, index) => (
        <div key={index} className="grid grid-cols-[1fr_1fr_auto] gap-1.5">
          <label className="sr-only" htmlFor={`header-name-${index}`}>Header name</label>
          <input id={`header-name-${index}`} value={row.name} onChange={(event) => commit(rows.map((item, i) => i === index ? { ...item, name: event.target.value } : item))} className="input-field min-w-0 text-[11px]" placeholder="Accept" />
          <label className="sr-only" htmlFor={`header-value-${index}`}>Header value</label>
          <input id={`header-value-${index}`} value={row.value} onChange={(event) => commit(rows.map((item, i) => i === index ? { ...item, value: event.target.value } : item))} className="input-field min-w-0 text-[11px]" placeholder="application/json" />
          <button type="button" aria-label={`Remove header ${row.name || index + 1}`} onClick={() => commit(rows.filter((_, i) => i !== index))} className="rounded px-2 text-slate-500 hover:bg-rose-950/40 hover:text-rose-300">×</button>
        </div>
      ))}
      <p className="text-[10px] leading-4 text-slate-500">Authorization, cookies, and API keys must use an encrypted connection secret.</p>
    </div>
  );
};

interface ApiAssertionsEditorProps {
  value: Array<Record<string, any>>;
  onCommit: (value: Array<Record<string, any>>) => void;
}

const ApiAssertionsEditor: React.FC<ApiAssertionsEditorProps> = ({ value, onCommit }) => {
  const [assertions, setAssertions] = useState<Array<Record<string, any>>>(value || []);
  useEffect(() => setAssertions(value || []), [JSON.stringify(value)]);
  const update = (index: number, key: string, next: any) => {
    const updated = assertions.map((assertion, i) => i === index ? { ...assertion, [key]: next } : assertion);
    setAssertions(updated);
    onCommit(updated);
  };
  const add = () => {
    const updated = [...assertions, { target: 'json', path: '', operator: 'exists' }];
    setAssertions(updated);
    onCommit(updated);
  };
  return (
    <div className="space-y-2 rounded-lg border border-slate-800 bg-slate-950/50 p-3">
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-slate-300">Response assertions</span>
        <button type="button" onClick={add} className="text-[10px] font-semibold text-indigo-300 hover:text-indigo-200">Add assertion</button>
      </div>
      {assertions.map((assertion, index) => (
        <div key={index} className="space-y-2 border-t border-slate-800 pt-2">
          <div className="flex gap-2">
            <label className="min-w-0 flex-1 text-[10px] text-slate-500">Response value
              <select value={assertion.target || 'json'} onChange={(event) => {
                const target = event.target.value;
                const updated = assertions.map((item, i) => i === index ? { target, [target === 'json' ? 'path' : 'name']: '', operator: 'exists' } : item);
                setAssertions(updated);
                onCommit(updated);
              }} className="input-field mt-1 text-[11px]"><option value="json">JSON path</option><option value="header">Header</option></select>
            </label>
            <button type="button" aria-label={`Remove assertion ${index + 1}`} onClick={() => {
              const updated = assertions.filter((_, i) => i !== index);
              setAssertions(updated);
              onCommit(updated);
            }} className="mt-5 rounded px-2 text-slate-500 hover:bg-rose-950/40 hover:text-rose-300">×</button>
          </div>
          <label className="block text-[10px] text-slate-500">{assertion.target === 'header' ? 'Header name' : 'JSON path'}
            <input value={assertion.target === 'header' ? assertion.name || '' : assertion.path || ''} onChange={(event) => update(index, assertion.target === 'header' ? 'name' : 'path', event.target.value)} className="input-field mt-1 text-[11px]" placeholder={assertion.target === 'header' ? 'content-type' : 'data.id'} />
          </label>
          <label className="block text-[10px] text-slate-500">Condition
            <select value={assertion.operator || 'exists'} onChange={(event) => update(index, 'operator', event.target.value)} className="input-field mt-1 text-[11px]">
              <option value="exists">is present</option><option value="equals">equals</option><option value="not_equals">does not equal</option><option value="contains">contains</option><option value="greater_than">is greater than</option><option value="less_than">is less than</option>
            </select>
          </label>
          {assertion.operator !== 'exists' && <label className="block text-[10px] text-slate-500">Expected value
            <input value={typeof assertion.expected === 'string' ? assertion.expected : JSON.stringify(assertion.expected ?? '')} onChange={(event) => {
              let expected: any = event.target.value;
              try { expected = JSON.parse(event.target.value); } catch { /* Plain text remains a string. */ }
              update(index, 'expected', expected);
            }} className="input-field mt-1 text-[11px]" />
          </label>}
        </div>
      ))}
      {assertions.length === 0 && <p className="text-[10px] text-slate-500">Add conditions for selected response fields. Status-code validation is configured above.</p>}
    </div>
  );
};

interface VariableRow {
  name: string;
  mode: 'literal' | 'function';
  type: string;
  valueText: string;
  functionId: string;
  argsText: string;
}

interface VariableDefinitionsEditorProps {
  title: string;
  value: Record<string, any>;
  onCommit: (value: Record<string, any>) => void;
  onValidationChange: (message: string | null) => void;
}

const BUILTIN_FUNCTIONS = [
  ['fn.uuid_v4', 'UUID'], ['fn.random_int', 'Random integer'], ['fn.random_string', 'Random string'],
  ['fn.synthetic_email', 'Synthetic email'], ['fn.concat', 'Join text'], ['fn.lower', 'Lowercase'],
  ['fn.upper', 'Uppercase'], ['fn.trim', 'Trim text'], ['fn.replace', 'Replace text'],
  ['fn.length', 'Length'], ['fn.add', 'Add numbers'], ['fn.subtract', 'Subtract numbers'],
  ['fn.round', 'Round number'], ['fn.date_format', 'Format UTC date'], ['fn.date_add', 'Add to date'],
  ['fn.json_extract', 'Select JSON field'], ['fn.url_encode', 'URL encode'],
];
const VARIABLE_TYPES = ['string', 'integer', 'decimal', 'boolean', 'object', 'array', 'datetime'];

function getVariableRows(value: Record<string, any>): VariableRow[] {
  return Object.entries(value || {}).map(([name, entry]) => {
    if (entry && typeof entry === 'object' && !Array.isArray(entry) && typeof entry.function === 'string') {
      return { name, mode: 'function', type: 'string', valueText: '', functionId: entry.function, argsText: JSON.stringify(entry.args || [], null, 2) };
    }
    const wrapped = entry && typeof entry === 'object' && !Array.isArray(entry) && typeof entry.type === 'string' && 'value' in entry;
    const raw = wrapped ? entry.value : entry;
    const type = wrapped ? entry.type : typeof raw === 'string' ? 'string' : typeof raw === 'boolean' ? 'boolean' : typeof raw === 'number' ? Number.isInteger(raw) ? 'integer' : 'decimal' : Array.isArray(raw) ? 'array' : raw && typeof raw === 'object' ? 'object' : 'string';
    return { name, mode: 'literal', type, valueText: type === 'string' || type === 'datetime' ? String(raw ?? '') : JSON.stringify(raw ?? (type === 'array' ? [] : {}), null, 2), functionId: 'fn.uuid_v4', argsText: '[]' };
  });
}

const VariableDefinitionsEditor: React.FC<VariableDefinitionsEditorProps> = ({ title, value, onCommit, onValidationChange }) => {
  const [rows, setRows] = useState<VariableRow[]>(() => getVariableRows(value));
  const [validationError, setValidationError] = useState<string | null>(null);
  const commit = (updated: VariableRow[]) => {
    setRows(updated);
    const definitions: Record<string, any> = {};
    const seen = new Set<string>();
    try {
      for (const row of updated) {
        const name = row.name.trim();
        if (!name) continue;
        if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name)) throw new Error(`“${name}” is not a valid variable name.`);
        if (seen.has(name)) throw new Error(`Variable “${name}” is defined more than once.`);
        seen.add(name);
        if (row.mode === 'function') {
          const args = JSON.parse(row.argsText || '[]');
          if (!Array.isArray(args)) throw new Error(`Arguments for “${name}” must be a JSON array.`);
          definitions[name] = { function: row.functionId, args };
        } else {
          let parsed: any = row.valueText;
          if (row.type !== 'string' && row.type !== 'datetime') parsed = JSON.parse(row.valueText);
          definitions[name] = { type: row.type, value: parsed };
        }
      }
      if (seen.size > 100 || new TextEncoder().encode(JSON.stringify(definitions)).length > 65_536) throw new Error('Variables exceed the 100 item or 64 KiB limit.');
      setValidationError(null);
      onValidationChange(null);
      onCommit(definitions);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Variable definitions are invalid.';
      setValidationError(message);
      onValidationChange(message);
    }
  };
  const update = (index: number, patch: Partial<VariableRow>) => commit(rows.map((row, rowIndex) => rowIndex === index ? { ...row, ...patch } : row));
  const add = () => commit([...rows, { name: '', mode: 'literal', type: 'string', valueText: '', functionId: 'fn.uuid_v4', argsText: '[]' }]);
  return (
    <section className="space-y-3 rounded-xl border border-slate-800 bg-slate-950/50 p-3">
      <div className="flex items-center justify-between">
        <div><h3 className="text-xs font-semibold text-slate-200">{title}</h3><p className="mt-1 text-[10px] text-slate-500">Use references such as {'{{suite.customer_type}}'} or {'{{case.order_id}}'}.</p></div>
        <button type="button" onClick={add} className="btn btn-secondary px-2 py-1 text-[10px]">Add variable</button>
      </div>
      {rows.map((row, index) => (
        <div key={index} className="space-y-2 border-t border-slate-800 pt-3">
          <div className="flex gap-2">
            <input aria-label={`${title} variable name`} value={row.name} onChange={(event) => update(index, { name: event.target.value })} className="input-field min-w-0 flex-1 text-[11px]" placeholder="variable_name" />
            <button type="button" aria-label={`Remove variable ${row.name || index + 1}`} onClick={() => commit(rows.filter((_, rowIndex) => rowIndex !== index))} className="rounded px-2 text-slate-500 hover:bg-rose-950/40 hover:text-rose-300">×</button>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <label className="text-[10px] text-slate-500">Value kind
              <select value={row.mode} onChange={(event) => update(index, { mode: event.target.value as VariableRow['mode'] })} className="input-field mt-1 text-[11px]"><option value="literal">Typed value</option><option value="function">Built-in function</option></select>
            </label>
            {row.mode === 'literal' ? <label className="text-[10px] text-slate-500">Type
              <select value={row.type} onChange={(event) => update(index, { type: event.target.value, valueText: event.target.value === 'array' ? '[]' : event.target.value === 'object' ? '{}' : '' })} className="input-field mt-1 text-[11px]">{VARIABLE_TYPES.map((type) => <option key={type}>{type}</option>)}</select>
            </label> : <label className="text-[10px] text-slate-500">Function
              <select value={row.functionId} onChange={(event) => update(index, { functionId: event.target.value })} className="input-field mt-1 text-[11px]">{BUILTIN_FUNCTIONS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}</select>
            </label>}
          </div>
          {row.mode === 'literal' ? <label className="block text-[10px] text-slate-500">Value
            <textarea rows={row.type === 'object' || row.type === 'array' ? 3 : 1} value={row.valueText} onChange={(event) => update(index, { valueText: event.target.value })} className="input-field mt-1 resize-y font-mono text-[11px]" placeholder={row.type === 'object' ? '{}' : row.type === 'array' ? '[]' : 'value'} />
          </label> : <label className="block text-[10px] text-slate-500">Arguments (JSON)
            <textarea rows={2} value={row.argsText} onChange={(event) => update(index, { argsText: event.target.value })} className="input-field mt-1 resize-y font-mono text-[11px]" placeholder="[]" />
          </label>}
        </div>
      ))}
      {rows.length === 0 && <p className="text-[10px] text-slate-500">No variables defined at this scope.</p>}
      {validationError && <p role="alert" className="text-[10px] text-rose-300">{validationError}</p>}
    </section>
  );
};
