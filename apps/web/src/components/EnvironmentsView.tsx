import React, { useEffect, useState } from 'react';
import {
  Database,
  KeyRound,
  Shield,
  Plus,
  CheckCircle2,
  AlertCircle,
  Wifi,
} from 'lucide-react';
import {
  getEnvironments,
  createEnvironment,
  getConnections,
  createConnection,
  updateConnection,
  testConnection,
  getSecrets,
  createSecret,
  getResourceLocks,
  releaseResourceLock,
} from '../api/client';
import { InlineAlert } from './InlineAlert';

export const EnvironmentsView: React.FC = () => {
  const [tab, setTab] = useState<'env' | 'conn' | 'secrets' | 'locks'>('env');
  const [environments, setEnvironments] = useState<any[]>([]);
  const [connections, setConnections] = useState<any[]>([]);
  const [secrets, setSecrets] = useState<any[]>([]);
  const [resourceLocks, setResourceLocks] = useState<any[]>([]);

  // Create form states
  const [newEnvName, setNewEnvName] = useState('');
  const [newEnvVars, setNewEnvVars] = useState('{\n  "api_base_url": "https://httpbin.org",\n  "timeout_ms": 5000\n}');

  const [newConnName, setNewConnName] = useState('');
  const [newConnType, setNewConnType] = useState('mysql');
  const [newConnSettings, setNewConnSettings] = useState('{\n  "host": "localhost",\n  "port": 3306,\n  "database": "test_db"\n}');
  const [newConnSecrets, setNewConnSecrets] = useState('{\n  "password_secret": "mysql_password"\n}');
  const [editingConnectionId, setEditingConnectionId] = useState<string | null>(null);

  const [newSecretName, setNewSecretName] = useState('');
  const [newSecretValue, setNewSecretValue] = useState('');

  const [testResult, setTestResult] = useState<any>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    loadAll();
  }, []);

  const loadAll = async () => {
    try {
      const [e, c, s, locks] = await Promise.all([getEnvironments(), getConnections(), getSecrets(), getResourceLocks()]);
      setEnvironments(e);
      setConnections(c);
      setSecrets(s);
      setResourceLocks(locks);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to load environments and credentials.');
    }
  };

  useEffect(() => {
    if (tab !== 'locks') return;
    const refreshLocks = async () => {
      try {
        setResourceLocks(await getResourceLocks());
      } catch (err) {
        setErrorMessage(err instanceof Error ? err.message : 'Unable to refresh resource locks.');
      }
    };
    void refreshLocks();
    const timer = window.setInterval(() => void refreshLocks(), 5000);
    return () => window.clearInterval(timer);
  }, [tab]);

  const handleReleaseResourceLock = async (lock: any) => {
    const confirmed = window.confirm(`Review lock ${lock.resource_key} held by run ${lock.run_id}. Release it only if the previous run can no longer affect this resource.`);
    if (!confirmed) return;
    const reason = window.prompt('Enter an audit reason for releasing this resource lock:');
    if (!reason) return;
    try {
      await releaseResourceLock({ resource_key: lock.resource_key, reason });
      await loadAll();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to release this resource lock.');
      void loadAll();
    }
  };

  const handleCreateEnv = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newEnvName) return;
    try {
      await createEnvironment({
        name: newEnvName,
        variables: JSON.parse(newEnvVars),
      });
      setNewEnvName('');
      loadAll();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to create the environment.');
    }
  };

  const handleCreateConn = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newConnName) return;
    try {
      const payload = {
        name: newConnName,
        connector_type: newConnType,
        settings: JSON.parse(newConnSettings),
        secret_refs: JSON.parse(newConnSecrets),
      };
      if (editingConnectionId) await updateConnection(editingConnectionId, payload);
      else await createConnection(payload);
      setEditingConnectionId(null);
      setNewConnName('');
      setNewConnSettings('{}');
      setNewConnSecrets('{}');
      loadAll();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to create the connection profile.');
    }
  };

  const editConnection = (connection: any) => {
    setEditingConnectionId(connection.id);
    setNewConnName(connection.name);
    setNewConnType(connection.connector_type);
    try { setNewConnSettings(JSON.stringify(JSON.parse(connection.settings_json), null, 2)); }
    catch { setNewConnSettings('{}'); }
    try { setNewConnSecrets(JSON.stringify(JSON.parse(connection.secret_refs_json || '{}'), null, 2)); }
    catch { setNewConnSecrets('{}'); }
    setTab('conn');
    setErrorMessage(null);
  };

  const cancelEditConnection = () => {
    setEditingConnectionId(null);
    setNewConnName('');
    setNewConnSettings('{}');
    setNewConnSecrets('{}');
  };

  const handleTestConn = async (id: string) => {
    try {
      const res = await testConnection(id);
      setTestResult(res);
      setTimeout(() => setTestResult(null), 4000);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to test this connection.');
    }
  };

  const handleCreateSecret = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newSecretName || !newSecretValue) return;
    try {
      await createSecret({
        name: newSecretName,
        plaintext: newSecretValue,
      });
      setNewSecretName('');
      setNewSecretValue('');
      loadAll();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to save the secret.');
    }
  };

  return (
    <div className="flex-1 p-8 overflow-y-auto bg-slate-950">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-xl font-bold text-slate-100 tracking-tight">
            Environments, Connectors & Secrets
          </h2>
          <p className="text-xs text-slate-400 mt-1">
            Scoped variable sets, database connection profiles, and encrypted credential storage
          </p>
        </div>

        {/* Tab switch */}
        <div className="flex bg-slate-900 border border-slate-800 rounded-lg p-1 text-xs">
          <button
            onClick={() => setTab('env')}
            className={`px-3 py-1.5 rounded-md font-medium transition-colors ${
              tab === 'env' ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-white'
            }`}
          >
            Environments
          </button>
          <button
            onClick={() => setTab('conn')}
            className={`px-3 py-1.5 rounded-md font-medium transition-colors ${
              tab === 'conn' ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-white'
            }`}
          >
            Connections
          </button>
          <button
            onClick={() => setTab('secrets')}
            className={`px-3 py-1.5 rounded-md font-medium transition-colors ${
              tab === 'secrets' ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-white'
            }`}
          >
            Encrypted Secrets
          </button>
          <button
            onClick={() => setTab('locks')}
            className={`px-3 py-1.5 rounded-md font-medium transition-colors ${tab === 'locks' ? 'bg-indigo-600 text-white' : 'text-slate-400 hover:text-white'}`}
          >
            Resource Locks
          </button>
        </div>
      </div>

      {errorMessage && <InlineAlert message={errorMessage} onDismiss={() => setErrorMessage(null)} className="mb-6" />}

      {testResult && (
        <div className="p-3 mb-6 bg-emerald-950/40 border border-emerald-800 rounded-lg flex items-center justify-between text-xs text-emerald-300">
          <span className="flex items-center gap-2">
            <CheckCircle2 className="w-4 h-4 text-emerald-400" />
            {testResult.message} (Latency: {testResult.latency_ms}ms)
          </span>
        </div>
      )}

      {/* Environments View */}
      {tab === 'env' && (
        <div className="grid grid-cols-3 gap-6">
          <div className="col-span-2 glass-panel p-5 space-y-4">
            <h3 className="text-sm font-bold text-slate-200">Registered Environments</h3>
            <div className="space-y-3">
              {environments.map((env) => (
                <div key={env.id} className="p-4 bg-slate-950 border border-slate-800 rounded-lg space-y-2">
                  <div className="flex items-center justify-between">
                    <h4 className="text-sm font-semibold text-slate-200">{env.name}</h4>
                    <span className="text-[10px] font-mono text-slate-500">ID: {env.id}</span>
                  </div>
                  <pre className="p-3 bg-slate-900 rounded font-mono text-xs text-indigo-300 overflow-x-auto">
                    {env.variables_json}
                  </pre>
                </div>
              ))}
            </div>
          </div>

          <div className="glass-panel p-5 space-y-4 h-fit">
            <h3 className="text-sm font-bold text-slate-200">Create Environment</h3>
            <form onSubmit={handleCreateEnv} className="space-y-3">
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">Name</label>
                <input
                  type="text"
                  value={newEnvName}
                  onChange={(e) => setNewEnvName(e.target.value)}
                  className="input-field text-xs"
                  placeholder="e.g. Staging-US"
                  required
                />
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">
                  Variables (JSON)
                </label>
                <textarea
                  value={newEnvVars}
                  onChange={(e) => setNewEnvVars(e.target.value)}
                  className="input-field font-mono text-xs h-32 resize-none"
                  required
                />
              </div>
              <button type="submit" className="btn btn-primary w-full text-xs py-2">
                Save Environment
              </button>
            </form>
          </div>
        </div>
      )}

      {/* Connections View */}
      {tab === 'conn' && (
        <div className="grid grid-cols-3 gap-6">
          <div className="col-span-2 glass-panel p-5 space-y-4">
            <h3 className="text-sm font-bold text-slate-200">Configured Connection Profiles</h3>
            <div className="space-y-3">
              {connections.length === 0 ? (
                <p className="text-xs text-slate-500">No external connection profiles registered.</p>
              ) : (
                connections.map((c) => (
                  <div key={c.id} className="p-4 bg-slate-950 border border-slate-800 rounded-lg flex items-center justify-between">
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="font-bold text-slate-200 text-xs">{c.name}</span>
                        <span className="badge badge-queued text-[10px]">{c.connector_type}</span>
                        {(c.settings_json?.includes('[REENTER_') || c.secret_refs_json?.includes('[REENTER_')) && <span className="rounded-md border border-amber-800 bg-amber-950/40 px-2 py-0.5 text-[10px] text-amber-300">Credentials needed</span>}
                      </div>
                      <pre className="font-mono text-[11px] text-slate-400 mt-1">
                        {c.settings_json}
                      </pre>
                    </div>
                    <div className="flex shrink-0 gap-2">
                      <button
                        onClick={() => editConnection(c)}
                        className="btn btn-secondary text-xs py-1.5 px-3"
                      >Edit / Rebind</button>
                      <button
                        onClick={() => handleTestConn(c.id)}
                        className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
                      >
                        <Wifi className="w-3.5 h-3.5 text-indigo-400" />
                        Test Connectivity
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>

          <div className="glass-panel p-5 space-y-4 h-fit">
            <h3 className="text-sm font-bold text-slate-200">{editingConnectionId ? 'Edit Connection Profile' : 'New Connection Profile'}</h3>
            {editingConnectionId && <p className="text-[11px] leading-5 text-amber-300">Choose encrypted secret names or IDs in Secret References. Secret plaintext is never shown here.</p>}
            <form onSubmit={handleCreateConn} className="space-y-3">
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">Profile Name</label>
                <input
                  type="text"
                  value={newConnName}
                  onChange={(e) => setNewConnName(e.target.value)}
                  className="input-field text-xs"
                  placeholder="e.g. Analytics Read DB"
                  required
                />
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">Connector Type</label>
                <select
                  value={newConnType}
                  onChange={(e) => {
                    const nextType = e.target.value;
                    setNewConnType(nextType);
                    if (nextType === 'cassandra') {
                      setNewConnSettings(JSON.stringify({ host: 'localhost', port: 9042, keyspace: 'test_keyspace', tls: true }, null, 2));
                      setNewConnSecrets(JSON.stringify({ password_secret: 'cassandra_password' }, null, 2));
                    } else if (nextType === 'mysql') {
                      setNewConnSettings(JSON.stringify({ host: 'localhost', port: 3306, database: 'test_db', tls: true }, null, 2));
                      setNewConnSecrets(JSON.stringify({ password_secret: 'mysql_password' }, null, 2));
                    }
                  }}
                  className="input-field text-xs font-mono"
                >
                  <option value="mysql">MySQL / MariaDB</option>
                  <option value="cassandra">Cassandra</option>
                  <option value="mongodb">MongoDB</option>
                  <option value="api">HTTP API</option>
                  <option value="http">HTTP endpoint</option>
                  <option value="parquet">Parquet Tabular</option>
                  <option value="delta">Delta Lake</option>
                </select>
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">
                  Non-Secret Settings (JSON)
                </label>
                <textarea
                  value={newConnSettings}
                  onChange={(e) => setNewConnSettings(e.target.value)}
                  className="input-field font-mono text-xs h-28 resize-none"
                  required
                />
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">Encrypted Secret References (JSON)</label>
                <textarea
                  value={newConnSecrets}
                  onChange={(e) => setNewConnSecrets(e.target.value)}
                  className="input-field font-mono text-xs h-24 resize-none"
                  required
                />
                <p className="mt-1 text-[10px] text-slate-500">Use adapter fields such as <code>password_secret</code> and reference a secret name or ID stored above.</p>
              </div>
              <button type="submit" className="btn btn-primary w-full text-xs py-2">
                {editingConnectionId ? 'Save Profile & Secret Bindings' : 'Save Profile'}
              </button>
              {editingConnectionId && <button type="button" onClick={cancelEditConnection} className="btn btn-secondary w-full text-xs py-2">Cancel Edit</button>}
            </form>
          </div>
        </div>
      )}

      {/* Secrets View */}
      {tab === 'secrets' && (
        <div className="grid grid-cols-3 gap-6">
          <div className="col-span-2 glass-panel p-5 space-y-4">
            <div className="flex items-center gap-2">
              <Shield className="w-4 h-4 text-emerald-400" />
              <h3 className="text-sm font-bold text-slate-200">Secrets (Encrypted at Rest with AES-256-GCM)</h3>
            </div>
            <p className="text-xs text-slate-400">
              Secret plaintext is write-only and encrypted immediately. Stored secrets are never transmitted to browser local storage or SSE streams.
            </p>

            <div className="space-y-2 pt-2">
              {secrets.length === 0 ? (
                <p className="text-xs text-slate-500">No secrets registered.</p>
              ) : (
                secrets.map((sec) => (
                  <div key={sec.id} className="flex items-center justify-between p-3 bg-slate-950 border border-slate-800 rounded-lg text-xs font-mono">
                    <span className="text-slate-200 font-semibold">{sec.name}</span>
                    <span className="text-emerald-400 font-semibold">v{sec.secret_version} • ENCRYPTED</span>
                  </div>
                ))
              )}
            </div>
          </div>

          <div className="glass-panel p-5 space-y-4 h-fit">
            <h3 className="text-sm font-bold text-slate-200">Register Secret Key</h3>
            <form onSubmit={handleCreateSecret} className="space-y-3">
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">Secret Key Name</label>
                <input
                  type="text"
                  value={newSecretName}
                  onChange={(e) => setNewSecretName(e.target.value)}
                  className="input-field text-xs font-mono"
                  placeholder="e.g. stripe_api_key"
                  required
                />
              </div>
              <div>
                <label className="block text-xs font-semibold text-slate-300 mb-1">
                  Plaintext Value (Will be encrypted)
                </label>
                <input
                  type="password"
                  value={newSecretValue}
                  onChange={(e) => setNewSecretValue(e.target.value)}
                  className="input-field text-xs font-mono"
                  placeholder="sk_test_..."
                  required
                />
              </div>
              <button type="submit" className="btn btn-primary w-full text-xs py-2">
                Encrypt & Store
              </button>
            </form>
          </div>
        </div>
      )}

      {tab === 'locks' && (
        <section className="glass-panel p-5 space-y-4">
          <div>
            <h3 className="text-sm font-bold text-slate-200">Held and uncertain resource locks</h3>
            <p className="mt-1 text-xs leading-5 text-slate-400">Runs hold declared resources through case cleanup. Locks left uncertain after a service restart require an administrator to review the run and release them with an audit reason.</p>
          </div>
          {resourceLocks.length === 0 ? (
            <p className="rounded-lg border border-dashed border-slate-700 p-4 text-xs text-slate-500">No resource locks are currently held or awaiting review.</p>
          ) : (
            <div className="space-y-2">
              {resourceLocks.map((lock) => (
                <div key={lock.resource_key} className="grid grid-cols-[1fr_auto_auto] items-center gap-4 rounded-lg border border-slate-800 bg-slate-950/60 p-3">
                  <div className="min-w-0">
                    <p className="truncate font-mono text-xs font-semibold text-slate-200">{lock.resource_key.split('|').slice(1).join('|')}</p>
                    <p className="mt-1 truncate text-[10px] font-mono text-slate-500">Run {lock.run_id} · owner {lock.owner_id}</p>
                    <p className="mt-1 text-[10px] text-slate-500">Lease until {lock.lease_expires_at} · run status {lock.run_status}</p>
                  </div>
                  <span className={`rounded-md px-2 py-1 text-[10px] font-semibold ${lock.status === 'HELD' ? 'bg-emerald-950/50 text-emerald-300' : 'bg-amber-950/60 text-amber-300'}`}>{lock.status}</span>
                  <button
                    type="button"
                    onClick={() => handleReleaseResourceLock(lock)}
                    disabled={['QUEUED', 'RUNNING'].includes(lock.run_status)}
                    className="btn btn-secondary text-xs py-1.5 px-3 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {['QUEUED', 'RUNNING'].includes(lock.run_status) ? 'Run is active' : 'Review & release'}
                  </button>
                </div>
              ))}
            </div>
          )}
        </section>
      )}
    </div>
  );
};
