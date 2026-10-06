import React, { FormEvent, useState } from 'react';
import { ArrowRight, LockKeyhole, Sparkles } from 'lucide-react';
import { login } from '../api/client';
import { InlineAlert } from './InlineAlert';

interface LoginViewProps {
  onSignedIn: () => Promise<void>;
}

export const LoginView: React.FC<LoginViewProps> = ({ onSignedIn }) => {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await login(email, password);
      await onSignedIn();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Sign in failed. Check your details and try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="flex min-h-screen min-w-[1280px] items-center justify-center px-12 py-10">
      <section className="glass-panel grid w-full max-w-5xl grid-cols-[1.1fr_0.9fr] overflow-hidden">
        <div className="flex flex-col justify-between border-r border-slate-700/60 bg-indigo-500/[0.04] p-12">
          <div className="flex items-center gap-3">
            <span className="flex h-11 w-11 items-center justify-center rounded-2xl bg-indigo-500 shadow-glow">
              <Sparkles className="h-5 w-5 text-white" />
            </span>
            <div>
              <div className="text-lg font-bold tracking-tight text-slate-100">TestIT</div>
              <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-indigo-300">Automation workspace</div>
            </div>
          </div>
          <div className="max-w-md py-16">
            <div className="mb-4 text-xs font-bold uppercase tracking-[0.18em] text-indigo-300">Welcome back</div>
            <h1 className="text-4xl font-semibold leading-tight tracking-tight text-slate-50">Make every release easier to trust.</h1>
            <p className="mt-5 max-w-sm text-sm leading-6 text-slate-400">Sign in to author backend checks, run suites, and review results from one desktop workspace.</p>
          </div>
          <p className="text-xs text-slate-500">Secure sign in · Desktop workspace</p>
        </div>

        <div className="p-12">
          <div className="mb-8 flex h-10 w-10 items-center justify-center rounded-xl border border-slate-700 bg-slate-900/70 text-indigo-300">
            <LockKeyhole className="h-4 w-4" />
          </div>
          <h2 className="text-2xl font-semibold text-slate-100">Sign in</h2>
          <p className="mt-2 text-sm text-slate-400">Use the account configured by your workspace administrator.</p>
          {error && <div className="mt-5"><InlineAlert message={error} /></div>}
          <form className="mt-8 space-y-5" onSubmit={submit}>
            <label className="block text-xs font-semibold text-slate-300">
              Email address
              <input
                autoComplete="username"
                autoFocus
                className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950/70 px-3.5 py-3 text-sm text-slate-100 placeholder:text-slate-600"
                onChange={(event) => setEmail(event.target.value)}
                required
                type="email"
                value={email}
              />
            </label>
            <label className="block text-xs font-semibold text-slate-300">
              Password
              <input
                autoComplete="current-password"
                className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950/70 px-3.5 py-3 text-sm text-slate-100 placeholder:text-slate-600"
                onChange={(event) => setPassword(event.target.value)}
                required
                type="password"
                value={password}
              />
            </label>
            <button className="btn-primary w-full justify-center py-3" disabled={busy} type="submit">
              {busy ? 'Signing in…' : 'Continue'}
              {!busy && <ArrowRight className="h-4 w-4" />}
            </button>
          </form>
          <p className="mt-6 text-center text-[11px] leading-5 text-slate-500">Your session expires automatically after a period of inactivity.</p>
        </div>
      </section>
    </main>
  );
};
