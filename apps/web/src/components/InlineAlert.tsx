import React from 'react';
import { AlertCircle, X } from 'lucide-react';

interface InlineAlertProps {
  message: string;
  onDismiss?: () => void;
  className?: string;
}

export const InlineAlert: React.FC<InlineAlertProps> = ({ message, onDismiss, className = '' }) => (
  <div
    role="alert"
    className={`flex items-center justify-between gap-4 rounded-lg border border-rose-500/25 bg-rose-500/10 px-4 py-2.5 text-xs text-rose-200 ${className}`}
  >
    <span className="flex items-center gap-2">
      <AlertCircle className="h-4 w-4 shrink-0" />
      {message}
    </span>
    {onDismiss && (
      <button
        onClick={onDismiss}
        className="rounded p-1 text-rose-200/70 hover:bg-rose-500/10 hover:text-rose-100"
        aria-label="Dismiss error"
      >
        <X className="h-4 w-4" />
      </button>
    )}
  </div>
);
