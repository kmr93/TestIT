import React, { useState } from 'react';
import {
  Globe,
  Database,
  FileSpreadsheet,
  Clock,
  GitBranch,
  Plus,
  Trash2,
  Settings2,
  CheckCircle2,
  Smartphone,
  Monitor,
} from 'lucide-react';
import { NodeInstance } from '../types';

interface WorkflowCanvasProps {
  nodes: NodeInstance[];
  onChange: (nodes: NodeInstance[]) => void;
  onSelectNode: (node: NodeInstance) => void;
  selectedNodeId: string | null;
}

export const WorkflowCanvas: React.FC<WorkflowCanvasProps> = ({
  nodes,
  onChange,
  onSelectNode,
  selectedNodeId,
}) => {
  const getNodeIcon = (type: string) => {
    switch (type) {
      case 'api.request':
        return <Globe className="w-4 h-4 text-sky-400" />;
      case 'db.mysql':
      case 'db.mongodb':
        return <Database className="w-4 h-4 text-emerald-400" />;
      case 'data.tabular':
        return <FileSpreadsheet className="w-4 h-4 text-amber-400" />;
      case 'sleep.wait':
        return <Clock className="w-4 h-4 text-purple-400" />;
      case 'condition.branch':
        return <GitBranch className="w-4 h-4 text-rose-400" />;
      default:
        return <Settings2 className="w-4 h-4 text-slate-400" />;
    }
  };

  const addNode = (type: string, name: string) => {
    const newNode: NodeInstance = {
      id: crypto.randomUUID(),
      type,
      type_version: 1,
      name,
      timeout_seconds: 30,
      config:
        type === 'api.request'
          ? { method: 'GET', path: 'https://httpbin.org/get', expected_status: 200 }
          : type === 'sleep.wait'
          ? { duration_seconds: 2 }
          : {},
      position: { x: 50, y: nodes.length * 100 + 50 },
    };
    onChange([...nodes, newNode]);
    onSelectNode(newNode);
  };

  const removeNode = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    onChange(nodes.filter((n) => n.id !== id));
  };

  return (
    <div className="flex-1 bg-slate-950/60 rounded-xl border border-slate-800 p-6 flex flex-col justify-between overflow-y-auto">
      <div>
        {/* Canvas Toolbar */}
        <div className="flex items-center justify-between mb-6 pb-4 border-b border-slate-800">
          <div className="flex items-center gap-2">
            <span className="text-xs font-semibold text-slate-400 uppercase tracking-wider">
              Nodes in Case ({nodes.length})
            </span>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={() => addNode('api.request', 'HTTP API Request')}
              className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5 hover:border-sky-500/50"
            >
              <Plus className="w-3.5 h-3.5 text-sky-400" />
              API Request
            </button>
            <button
              onClick={() => addNode('db.mysql', 'MySQL Read Check')}
              className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5 hover:border-emerald-500/50"
            >
              <Plus className="w-3.5 h-3.5 text-emerald-400" />
              DB Check
            </button>
            <button
              onClick={() => addNode('data.tabular', 'Parquet Table Check')}
              className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5 hover:border-amber-500/50"
            >
              <Plus className="w-3.5 h-3.5 text-amber-400" />
              Data File
            </button>
            <button
              onClick={() => addNode('sleep.wait', 'Wait Condition')}
              className="btn btn-secondary text-xs py-1.5 px-3 flex items-center gap-1.5 hover:border-purple-500/50"
            >
              <Plus className="w-3.5 h-3.5 text-purple-400" />
              Wait Condition
            </button>
          </div>
        </div>

        {/* Nodes Sequence Canvas */}
        {nodes.length === 0 ? (
          <div className="h-64 border-2 border-dashed border-slate-800 rounded-xl flex flex-col items-center justify-center text-slate-500">
            <Globe className="w-10 h-10 mb-2 stroke-1 text-slate-600" />
            <p className="text-sm font-medium">No nodes added to this workflow yet</p>
            <p className="text-xs text-slate-600 mt-1">
              Select a node type from the toolbar above to start assembling.
            </p>
          </div>
        ) : (
          <div className="space-y-4 max-w-xl mx-auto">
            {nodes.map((node, index) => {
              const isSelected = selectedNodeId === node.id;
              return (
                <div key={node.id} className="relative">
                  <div
                    onClick={() => onSelectNode(node)}
                    className={`glass-panel p-4 cursor-pointer transition-all flex items-center justify-between ${
                      isSelected
                        ? 'border-indigo-500 ring-2 ring-indigo-500/20 bg-slate-800/80'
                        : 'hover:border-slate-700'
                    }`}
                  >
                    <div className="flex items-center gap-3">
                      <div className="w-7 h-7 rounded-lg bg-slate-900 border border-slate-800 flex items-center justify-center font-mono text-xs text-slate-400 font-semibold">
                        {index + 1}
                      </div>
                      <div className="w-8 h-8 rounded-lg bg-slate-900/80 flex items-center justify-center">
                        {getNodeIcon(node.type)}
                      </div>
                      <div>
                        <h4 className="text-sm font-semibold text-slate-200">{node.name}</h4>
                        <div className="flex items-center gap-2 mt-0.5">
                          <span className="text-[11px] font-mono text-indigo-400">{node.type}</span>
                          <span className="text-[10px] text-slate-500">•</span>
                          <span className="text-[11px] text-slate-400">Timeout: {node.timeout_seconds}s</span>
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center gap-2">
                      <button
                        onClick={(e) => removeNode(node.id, e)}
                        className="p-1.5 rounded-lg text-slate-500 hover:text-red-400 hover:bg-red-500/10 transition-colors"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </div>
                  </div>

                  {/* Connecting edge to next node */}
                  {index < nodes.length - 1 && (
                    <div className="flex justify-center my-1.5">
                      <div className="w-0.5 h-5 bg-gradient-to-b from-indigo-500/50 to-slate-700"></div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
};
