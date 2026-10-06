import React, { useState } from 'react';
import { Navigation } from './components/Navigation';
import { SuitesView } from './components/SuitesView';
import { RunsView } from './components/RunsView';
import { EnvironmentsView } from './components/EnvironmentsView';
import { LiveRunMonitor } from './components/LiveRunMonitor';
import { OpenApiImportModal } from './components/OpenApiImportModal';
import { VariablePreviewModal } from './components/VariablePreviewModal';

export const App: React.FC = () => {
  const [activeTab, setActiveTab] = useState<string>('suites');
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [showOpenApiModal, setShowOpenApiModal] = useState<boolean>(false);
  const [showPreviewModal, setShowPreviewModal] = useState<boolean>(false);

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
    <div className="flex min-h-screen bg-slate-950 text-slate-100">
      {/* Sidebar Navigation */}
      <Navigation activeTab={activeTab} setActiveTab={handleTabChange} />

      {/* Main Content Workspace */}
      <main className="flex-1 flex flex-col h-screen overflow-hidden">
        {activeTab === 'suites' && (
          <SuitesView onRunStarted={(runId) => setActiveRunId(runId)} />
        )}

        {activeTab === 'runs' && (
          <RunsView onSelectRun={(runId) => setActiveRunId(runId)} />
        )}

        {activeTab === 'environments' && <EnvironmentsView />}
      </main>

      {/* Live Run Monitor Drawer / Modal */}
      {activeRunId && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-6">
          <div className="w-full max-w-4xl max-h-[90vh] overflow-y-auto">
            <LiveRunMonitor runId={activeRunId} onClose={() => setActiveRunId(null)} />
          </div>
        </div>
      )}

      {/* OpenAPI Import Wizard Modal */}
      {showOpenApiModal && (
        <OpenApiImportModal
          onClose={() => setShowOpenApiModal(false)}
          onImportSuccess={() => {
            setActiveTab('suites');
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
