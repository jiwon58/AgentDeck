import { useState } from 'react';
import { api, browse } from './api';
import type { Candidate } from './types';

type Publisher = 'claude' | 'hermes';
type Download = Awaited<ReturnType<typeof api.downloadAgent>>;
export interface InstallReview { agent: Publisher; directory: string; candidate: Candidate }

export function AgentInstallSettings({ task, onReview }: {
  task: (work: () => Promise<void>) => Promise<void>;
  onReview: (value: InstallReview) => void;
}) {
  const [agent, setAgent] = useState<Publisher>('claude');
  const [directory, setDirectory] = useState('');
  const [download, setDownload] = useState<Download>();
  const [approved, setApproved] = useState(false);
  const [scan, setScan] = useState<Awaited<ReturnType<typeof api.installHermes>>>();
  const reset = () => { setDownload(undefined); setApproved(false); setScan(undefined); };
  const hermes = agent === 'hermes';
  return <section className="stack">
    <h3>Install Agents</h3>
    <p className="muted">Publisher downloads require internet access. Choose a folder for a fresh installation.</p>
    <label>Agent<select value={agent} onChange={event => { reset(); setAgent(event.target.value as Publisher); }}>
      <option value="claude">Claude Code</option><option value="hermes">Hermes Agent</option>
    </select></label>
    <div className="row end"><label className="grow">Destination directory<input value={directory} onChange={event => {
      reset(); setDirectory(event.target.value);
    }} /></label><button onClick={() => void task(async () => {
      const selected = await browse(true);
      if (selected) { reset(); setDirectory(selected); }
    })}>Browse</button></div>
    <p className="notice">{hermes
      ? 'The Nous Research bootstrap installs into hermes-home and hermes-agent under this folder. It downloads dependencies and can add the launcher to the Windows user PATH.'
      : 'Deck places the Windows x64 claude.exe here and verifies it against Anthropic’s release manifest.'}</p>
    <div><button className="accent" disabled={!directory.trim() || !!download}
      onClick={() => void task(async () => setDownload(await api.downloadAgent(agent, directory)))}>
      {hermes ? 'Download official installer' : 'Download and install Claude Code'}
    </button></div>
    {download && <section className="review stack">
      <strong>{download.version ? 'Claude Code ' + download.version : 'Publisher installer downloaded'}</strong>
      <div className="path">{download.path}<small>SHA-256 {download.sha256}</small></div>
      {!hermes && <button onClick={() => void task(async () => {
        const candidate = await api.inspect(download.path);
        if (candidate.sha256 !== download.sha256) throw new Error('The downloaded executable changed.');
        onReview({ agent, directory, candidate });
      })}>Review runtime</button>}
      {hermes && !scan && <>
        <label className="check"><input type="checkbox" checked={approved} onChange={event => setApproved(event.target.checked)} />
          Run the reviewed Nous Research installer with these folder settings</label>
        <button className="accent" disabled={!approved} onClick={() => void task(async () => {
          setApproved(false);
          setScan(await api.installHermes(download.sha256));
        })}>Install Hermes</button>
      </>}
      {scan && <>
        <strong>Hermes installer completed</strong>
        {scan.candidates.filter(candidate => candidate.launch_type === 'native_exe').map(candidate =>
          <div key={candidate.path} className="row"><span className="grow path">{candidate.path}</span>
            <button onClick={() => onReview({ agent, directory, candidate })}>Review runtime</button></div>)}
        {!scan.candidates.some(candidate => candidate.launch_type === 'native_exe') &&
          <p>Use Agent Runtimes to inspect the publisher launcher or register its interpreter.</p>}
        {scan.warnings.map((warning, index) => <small key={index}>{warning}</small>)}
      </>}
    </section>}
  </section>;
}
