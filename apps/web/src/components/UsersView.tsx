import React, { useEffect, useState } from 'react';
import { KeyRound, ShieldCheck, UserPlus, Users } from 'lucide-react';
import { createUser, getUsers, updateUser } from '../api/client';
import { InlineAlert } from './InlineAlert';

type WorkspaceUser = {
  id: string;
  email: string;
  display_name: string;
  role: 'ADMIN' | 'AUTHOR' | 'RUNNER' | 'VIEWER';
  active: boolean;
};

const roles: WorkspaceUser['role'][] = ['ADMIN', 'AUTHOR', 'RUNNER', 'VIEWER'];

export const UsersView: React.FC = () => {
  const [users, setUsers] = useState<WorkspaceUser[]>([]);
  const [email, setEmail] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [role, setRole] = useState<WorkspaceUser['role']>('AUTHOR');
  const [password, setPassword] = useState('');
  const [resetPasswords, setResetPasswords] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadUsers = async () => setUsers(await getUsers());
  useEffect(() => {
    void loadUsers().catch((reason) => setError(reason instanceof Error ? reason.message : 'Could not load workspace users.'));
  }, []);

  const create = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await createUser({ email, display_name: displayName, role, password });
      setEmail('');
      setDisplayName('');
      setPassword('');
      setNotice('Account created. Share the initial password with the new user through your secure channel.');
      await loadUsers();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not create the account.');
    } finally {
      setBusy(false);
    }
  };

  const saveRole = async (user: WorkspaceUser, nextRole: WorkspaceUser['role']) => {
    try {
      await updateUser(user.id, { role: nextRole });
      setNotice(`Updated ${user.email}'s role.`);
      await loadUsers();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not update the role.');
    }
  };

  const toggleActive = async (user: WorkspaceUser) => {
    try {
      await updateUser(user.id, { active: !user.active });
      setNotice(user.active ? `Disabled ${user.email} and signed out existing sessions.` : `Enabled ${user.email}.`);
      await loadUsers();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not update account access.');
    }
  };

  const resetPassword = async (user: WorkspaceUser) => {
    const nextPassword = resetPasswords[user.id] || '';
    try {
      await updateUser(user.id, { password: nextPassword });
      setResetPasswords((current) => ({ ...current, [user.id]: '' }));
      setNotice(`Reset ${user.email}'s password.`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not reset the password.');
    }
  };

  return (
    <section className="flex-1 overflow-y-auto bg-slate-950 p-8">
      <div className="mb-7 flex items-start justify-between">
        <div>
          <div className="mb-2 flex items-center gap-2 text-indigo-300">
            <ShieldCheck className="h-4 w-4" />
            <span className="text-[10px] font-bold uppercase tracking-[0.18em]">Workspace administration</span>
          </div>
          <h2 className="text-2xl font-bold tracking-tight text-slate-100">People & access</h2>
          <p className="mt-1 text-sm text-slate-400">Manage workspace roles, account access, and password resets.</p>
        </div>
        <div className="flex items-center gap-2 rounded-lg border border-slate-800 bg-slate-900 px-3 py-2 text-xs text-slate-300">
          <Users className="h-4 w-4 text-indigo-300" /> {users.length} accounts
        </div>
      </div>

      {error && <InlineAlert message={error} onDismiss={() => setError(null)} className="mb-4" />}
      {notice && <div role="status" className="mb-4 rounded-lg border border-emerald-800 bg-emerald-950/40 px-4 py-3 text-sm text-emerald-200">{notice}<button className="ml-4 text-emerald-400 underline" onClick={() => setNotice(null)}>Dismiss</button></div>}

      <div className="grid grid-cols-[minmax(0,1fr)_360px] gap-6">
        <div className="glass-panel overflow-hidden">
          <div className="border-b border-slate-800 px-5 py-4">
            <h3 className="text-sm font-semibold text-slate-200">Workspace accounts</h3>
          </div>
          <div className="divide-y divide-slate-800/80">
            {users.map((user) => (
              <article key={user.id} className="grid grid-cols-[minmax(190px,1fr)_150px_100px_minmax(210px,260px)] items-center gap-4 px-5 py-4">
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-slate-100">{user.display_name}</div>
                  <div className="mt-1 truncate text-xs text-slate-500">{user.email}</div>
                </div>
                <label className="sr-only" htmlFor={`role-${user.id}`}>Role for {user.email}</label>
                <select id={`role-${user.id}`} value={user.role} onChange={(event) => void saveRole(user, event.target.value as WorkspaceUser['role'])} className="input-field text-xs">
                  {roles.map((item) => <option key={item} value={item}>{item}</option>)}
                </select>
                <button type="button" onClick={() => void toggleActive(user)} className={`rounded-full px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider ${user.active ? 'bg-emerald-500/10 text-emerald-300' : 'bg-slate-800 text-slate-400'}`}>
                  {user.active ? 'Active' : 'Disabled'}
                </button>
                <div className="flex gap-2">
                  <label className="sr-only" htmlFor={`password-${user.id}`}>New password for {user.email}</label>
                  <input id={`password-${user.id}`} type="password" minLength={12} autoComplete="new-password" placeholder="Reset password" value={resetPasswords[user.id] || ''} onChange={(event) => setResetPasswords((current) => ({ ...current, [user.id]: event.target.value }))} className="input-field min-w-0 text-xs" />
                  <button type="button" title="Reset password" disabled={(resetPasswords[user.id] || '').length < 12} onClick={() => void resetPassword(user)} className="btn btn-secondary px-2 disabled:cursor-not-allowed disabled:opacity-40"><KeyRound className="h-4 w-4" /></button>
                </div>
              </article>
            ))}
            {users.length === 0 && <div className="px-5 py-12 text-center text-sm text-slate-500">No workspace accounts found.</div>}
          </div>
        </div>

        <form onSubmit={create} className="glass-panel h-fit space-y-4 p-5">
          <div className="flex items-center gap-2">
            <UserPlus className="h-4 w-4 text-indigo-300" />
            <h3 className="text-sm font-semibold text-slate-200">Add account</h3>
          </div>
          <label className="block text-xs text-slate-400">Display name<input required maxLength={128} value={displayName} onChange={(event) => setDisplayName(event.target.value)} className="input-field mt-1.5 text-sm" /></label>
          <label className="block text-xs text-slate-400">Email<input required type="email" maxLength={320} autoComplete="off" value={email} onChange={(event) => setEmail(event.target.value)} className="input-field mt-1.5 text-sm" /></label>
          <label className="block text-xs text-slate-400">Role<select value={role} onChange={(event) => setRole(event.target.value as WorkspaceUser['role'])} className="input-field mt-1.5 text-sm">{roles.map((item) => <option key={item} value={item}>{item}</option>)}</select></label>
          <label className="block text-xs text-slate-400">Initial password<input required type="password" minLength={12} autoComplete="new-password" value={password} onChange={(event) => setPassword(event.target.value)} className="input-field mt-1.5 text-sm" /><span className="mt-1 block text-[11px] text-slate-500">At least 12 characters. The password is never shown again.</span></label>
          <button disabled={busy} type="submit" className="btn btn-primary w-full">{busy ? 'Creating account…' : 'Create account'}</button>
        </form>
      </div>
    </section>
  );
};
