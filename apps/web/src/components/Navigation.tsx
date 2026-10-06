import React from 'react';
import {
  Layers,
  FileCode,
  PlayCircle,
  Database,
  Sliders,
  Sparkles,
  LogOut,
  Users,
} from 'lucide-react';

interface NavigationProps {
  activeTab: string;
  setActiveTab: (tab: string) => void;
  user: { display_name: string; role: 'ADMIN' | 'AUTHOR' | 'RUNNER' | 'VIEWER' | string };
  onLogout: () => void;
}

export const Navigation: React.FC<NavigationProps> = ({ activeTab, setActiveTab, user, onLogout }) => {
  const navItems = [
    { id: 'suites', label: 'Suites & Cases', icon: Layers },
    { id: 'runs', label: 'Runs & Reports', icon: PlayCircle },
    { id: 'environments', label: 'Environments & Secrets', icon: Database },
    { id: 'openapi', label: 'OpenAPI Import', icon: FileCode },
    { id: 'preview', label: 'Variable Preview', icon: Sliders },
    { id: 'users', label: 'People & Access', icon: Users },
  ];
  const visibleItems = navItems.filter((item) => {
    if (item.id === 'environments') return user.role === 'ADMIN';
    if (item.id === 'openapi') return user.role === 'ADMIN' || user.role === 'AUTHOR';
    if (item.id === 'users') return user.role === 'ADMIN';
    return true;
  });

  return (
    <nav className="w-64 bg-slate-900 border-r border-slate-800 flex flex-col justify-between p-4 min-h-screen">
      <div>
        <div className="flex items-center gap-3 px-2 py-4 mb-6">
          <div className="w-9 h-9 rounded-xl bg-gradient-to-tr from-indigo-600 to-violet-500 flex items-center justify-center shadow-lg shadow-indigo-500/20">
            <Sparkles className="w-5 h-5 text-white" />
          </div>
          <div>
            <h1 className="font-bold text-lg text-slate-100 tracking-tight">TestIT</h1>
            <p className="text-[10px] text-indigo-300/80 font-semibold uppercase tracking-[0.16em]">Automation workspace</p>
          </div>
        </div>

        <div className="space-y-1">
          {visibleItems.map((item) => {
            const Icon = item.icon;
            const isActive = activeTab === item.id;
            return (
              <button
                key={item.id}
                onClick={() => setActiveTab(item.id)}
                className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all ${
                  isActive
                    ? 'bg-indigo-600/15 text-indigo-400 border border-indigo-500/30 shadow-sm'
                    : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/60'
                }`}
              >
                <Icon className={`w-4 h-4 ${isActive ? 'text-indigo-400' : 'text-slate-400'}`} />
                {item.label}
              </button>
            );
          })}
        </div>
      </div>

      <div className="space-y-3">
        <div className="rounded-xl border border-slate-800/80 bg-slate-950/60 p-3">
          <div className="truncate text-xs font-semibold text-slate-200">{user.display_name}</div>
          <div className="mt-1 text-[10px] font-bold uppercase tracking-wider text-indigo-300">{user.role}</div>
        </div>
        <button onClick={onLogout} className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-xs font-medium text-slate-400 transition-colors hover:bg-slate-800 hover:text-slate-100">
          <LogOut className="h-3.5 w-3.5" />
          Sign out
        </button>
        <div className="rounded-xl border border-slate-800/80 bg-slate-950/60 p-3">
        <div className="flex items-center gap-2 mb-1.5">
          <span className="w-2 h-2 rounded-full bg-indigo-400"></span>
          <span className="text-xs font-semibold text-slate-300">Desktop workspace</span>
        </div>
        <p className="text-[11px] text-slate-500">Backend test suites and run reports</p>
        </div>
      </div>
    </nav>
  );
};
