import React from 'react';
import { ArrowUp, ArrowDown, Trash2, Edit3, Plus } from 'lucide-react';
import { NodeInstance } from '../types';

interface MobileListEditorProps {
  nodes: NodeInstance[];
  onChange: (nodes: NodeInstance[]) => void;
  onEditNode: (node: NodeInstance) => void;
}

export const MobileListEditor: React.FC<MobileListEditorProps> = ({
  nodes,
  onChange,
  onEditNode,
}) => {
  const moveUp = (index: number) => {
    if (index === 0) return;
    const updated = [...nodes];
    const temp = updated[index - 1];
    updated[index - 1] = updated[index];
    updated[index] = temp;
    onChange(updated);
  };

  const moveDown = (index: number) => {
    if (index === nodes.length - 1) return;
    const updated = [...nodes];
    const temp = updated[index + 1];
    updated[index + 1] = updated[index];
    updated[index] = temp;
    onChange(updated);
  };

  const deleteNode = (id: string) => {
    onChange(nodes.filter((n) => n.id !== id));
  };

  return (
    <div className="bg-slate-900 border border-slate-800 rounded-xl p-4">
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-sm font-bold text-slate-200">
          Touch-Friendly Step List ({nodes.length})
        </h3>
        <span className="text-xs text-indigo-400 font-medium">Reorder & Configure</span>
      </div>

      <div className="space-y-2">
        {nodes.map((node, idx) => (
          <div
            key={node.id}
            className="flex items-center justify-between p-3 bg-slate-950 border border-slate-800/80 rounded-lg"
          >
            <div className="flex items-center gap-3">
              <span className="w-6 h-6 rounded bg-slate-800 flex items-center justify-center text-xs font-mono text-slate-300">
                {idx + 1}
              </span>
              <div>
                <p className="text-xs font-semibold text-slate-100">{node.name}</p>
                <p className="text-[11px] font-mono text-slate-500">{node.type}</p>
              </div>
            </div>

            <div className="flex items-center gap-1">
              <button
                disabled={idx === 0}
                onClick={() => moveUp(idx)}
                className="p-1.5 rounded bg-slate-800 text-slate-400 disabled:opacity-30 hover:text-white"
              >
                <ArrowUp className="w-3.5 h-3.5" />
              </button>
              <button
                disabled={idx === nodes.length - 1}
                onClick={() => moveDown(idx)}
                className="p-1.5 rounded bg-slate-800 text-slate-400 disabled:opacity-30 hover:text-white"
              >
                <ArrowDown className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => onEditNode(node)}
                className="p-1.5 rounded bg-indigo-900/40 text-indigo-400 hover:text-white ml-1"
              >
                <Edit3 className="w-3.5 h-3.5" />
              </button>
              <button
                onClick={() => deleteNode(node.id)}
                className="p-1.5 rounded bg-red-900/30 text-red-400 hover:text-white"
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};
