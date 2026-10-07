import React, { useEffect, useState } from 'react';
import { Navigation } from './components/Navigation';
import { SuitesView } from './components/SuitesView';
import { RunsView } from './components/RunsView';
import { EnvironmentsView } from './components/EnvironmentsView';
import { LiveRunMonitor } from './components/LiveRunMonitor';
import { OpenApiImportModal } from './components/OpenApiImportModal';
import { VariablePreviewModal } from './components/VariablePreviewModal';
import { LoginView } from './components/LoginView';
import { UsersView } from './components/UsersView';
import { getCurrentUser, logout } from './api/client';

export const App: React.FC = () => {
  const [activeTab, setActiveTab] = useState<string>('suites');
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [assetRefreshKey, setAssetRefreshKey] = useState(0);
  const [showOpenApiModal, setShowOpenApiModal] = useState<boolean>(false);
  const [showPreviewModal, setShowPreviewModal] = useState<boolean>(false);
  const [user, setUser] = useState<any | null>(null);
  const [sessionChecked, setSessionChecked] = useState(false);

  const refreshUser = async () => {
    const currentUser = await getCurrentUser();
    setUser(currentUser);
  };

  useEffect(() => {
    void refreshUser().catch(() => setUser(null)).finally(() => setSessionChecked(true));
  }, []);

  const signOut = async () => {
    try {
      await logout();
    } finally {
      setUser(null);
    }
  };

  if (!sessionChecked) {
    return <main className="flex min-h-screen min-w-[1280px] items-center justify-center text-sm text-slate-400">Checking your session…</main>;
  }

  if (!user) return <LoginView onSignedIn={refreshUser} />;

  const handleTabChange = (tab: string) => {
    if (tab === 'openapi') {
      setShowOpenApiModal(true);
    } else if (tab === 'preview') {
      setShowPreviewModal(true);
    } else {
      setActiveTab(tab);
    }
  };

  return (
    <div className="app-shell flex min-h-screen bg-slate-950 text-slate-100">
      {/* Sidebar Navigation */}
      <Navigation activeTab={activeTab} setActiveTab={handleTabChange} user={user} onLogout={signOut} />

      {/* Main Content Workspace */}
      <main className="flex-1 flex flex-col h-screen overflow-hidden">
        {activeTab === 'suites' && (
          <SuitesView userRole={user.role} assetRefreshKey={assetRefreshKey} onRunStarted={(runId) => setActiveRunId(runId)} />
        )}

        {activeTab === 'runs' && (
            <RunsView userRole={user.role} onSelectRun={(runId) => setActiveRunId(runId)} />
        )}

        {activeTab === 'environments' && <EnvironmentsView />}
        {activeTab === 'users' && <UsersView />}
      </main>

      {/* Live Run Monitor Drawer / Modal */}
      {activeRunId && (
        <div role="dialog" aria-modal="true" aria-label="Run details" className="fixed inset-0 z-50 bg-slate-950/95 p-3 backdrop-blur-sm">
          <div className="h-full w-full">
            <LiveRunMonitor userRole={user.role} runId={activeRunId} onClose={() => setActiveRunId(null)} />
          </div>
        </div>
      )}

      {/* OpenAPI Import Wizard Modal */}
      {showOpenApiModal && (
        <OpenApiImportModal
          onClose={() => setShowOpenApiModal(false)}
          onImportSuccess={() => {
            setActiveTab('suites');
            setAssetRefreshKey((current) => current + 1);
          }}
        />
      )}

      {/* Zero-network Variable Preview Modal */}
      {showPreviewModal && (
        <VariablePreviewModal
          nodeConfig={null}
          onClose={() => setShowPreviewModal(false)}
        />
      )}
    </div>
  );
};

export default App;
