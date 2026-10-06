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
  testConnection,
  getSecrets,
  createSecret,
} from '../api/client';
import { InlineAlert } from './InlineAlert';

export const EnvironmentsView: React.FC = () => {
  const [tab, setTab] = useState<'env' | 'conn' | 'secrets'>('env');
  const [environments, setEnvironments] = useState<any[]>([]);
  const [connections, setConnections] = useState<any[]>([]);
  const [secrets, setSecrets] = useState<any[]>([]);

  // Create form states
  const [newEnvName, setNewEnvName] = useState('');
  const [newEnvVars, setNewEnvVars] = useState('{\n  "api_base_url": "https://httpbin.org",\n  "timeout_ms": 5000\n}');

  const [newConnName, setNewConnName] = useState('');
  const [newConnType, setNewConnType] = useState('mysql');
  const [newConnSettings, setNewConnSettings] = useState('{\n  "host": "localhost",\n  "port": 3306,\n  "database": "test_db"\n}');
  const [newConnSecrets, setNewConnSecrets] = useState('{\n  "password_secret": "mysql_password"\n}');

  const [newSecretName, setNewSecretName] = useState('');
  const [newSecretValue, setNewSecretValue] = useState('');

  const [testResult, setTestResult] = useState<any>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    loadAll();
  }, []);

  const loadAll = async () => {
    try {
      const [e, c, s] = await Promise.all([getEnvironments(), getConnections(), getSecrets()]);
      setEnvironments(e);
      setConnections(c);
      setSecrets(s);
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to load environments and credentials.');
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
      await createConnection({
        name: newConnName,
        connector_type: newConnType,
        settings: JSON.parse(newConnSettings),
        secret_refs: JSON.parse(newConnSecrets),
      });
      setNewConnName('');
      loadAll();
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : 'Unable to create the connection profile.');
    }
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
                      </div>
                      <pre className="font-mono text-[11px] text-slate-400 mt-1">
                        {c.settings_json}
                      </pre>
                    </div>
                    <button
                      onClick={() => handleTestConn(c.id)}
                      className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5"
                    >
                      <Wifi className="w-3.5 h-3.5 text-indigo-400" />
                      Test Connectivity
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>

          <div className="glass-panel p-5 space-y-4 h-fit">
            <h3 className="text-sm font-bold text-slate-200">New Connection Profile</h3>
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
                  onChange={(e) => setNewConnType(e.target.value)}
                  className="input-field text-xs font-mono"
                >
                  <option value="mysql">MySQL / MariaDB</option>
                  <option value="mongodb">MongoDB</option>
                  <option value="cassandra">Cassandra CQL</option>
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
                Save Profile
              </button>
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
    </div>
  );
};
