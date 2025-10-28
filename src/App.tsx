import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
import DevicesTab, { type AudioDevice, type MirrorStatus } from './tabs/DevicesTab';
import SyncTab from './tabs/SyncTab';
import { t } from './i18n';

interface AppStatus { allowMultiple: boolean; configs: any[] }

export default function App(){
  const [activeTab, setActiveTab] = useState<'devices'|'sync'>('devices');
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string|null>(null);
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [mirrorSelection, setMirrorSelection] = useState<string[]>([]);
  const [mirrorStatus, setMirrorStatus] = useState<MirrorStatus|null>(null);
  const [audioLoading, setAudioLoading] = useState(false);
  const [audioError, setAudioError] = useState<string|null>(null);
  const [warmupMs, setWarmupMs] = useState(40);
  const [ringMs, setRingMs] = useState(200);
  const [delays, setDelays] = useState<Record<string, number>>({});

  const refresh = async () => {
    setLoading(true);
    try {
      const s = await invoke<AppStatus>('get_status');
      setStatus(s);
      setError(null);
    } catch(e:any){ setError(e?.message ?? String(e)); }
    finally { setLoading(false); }
  };

  const loadAudioContext = async () => {
    setAudioLoading(true);
    try {
      const [devices, m] = await Promise.all([
        invoke<AudioDevice[]>('list_audio_devices'),
        invoke<MirrorStatus>('audio_mirror_status')
      ]);
      setAudioDevices(devices);
      setMirrorStatus(m);
      setMirrorSelection(m.targets.map(d=>d.id));
      setAudioError(null);
    } catch(e:any){ setAudioError(e?.message ?? String(e)); }
    finally { setAudioLoading(false); }
  };

  useEffect(()=>{
    refresh();
    loadAudioContext();
    let unlistenState: UnlistenFn|undefined;
    (async()=>{
      unlistenState = await listen<AppStatus>('state-updated', (ev)=> setStatus(ev.payload));
    })();
    return ()=>{ unlistenState?.(); };
  },[]);

  useEffect(()=>{
    if (activeTab !== 'devices') return;
    let stop=false;
    const id = setInterval(async ()=>{
      try{
        const [devices, m] = await Promise.all([
          invoke<AudioDevice[]>('list_audio_devices'),
          invoke<MirrorStatus>('audio_mirror_status')
        ]);
        if(!stop){ setAudioDevices(devices); setMirrorStatus(m); }
      }catch{}
    },150);
    return ()=>{ stop=true; clearInterval(id); };
  },[activeTab]);

  useEffect(()=>{
    (async()=>{
      try{ const [wu, rm] = await invoke<[number,number]>('get_audio_settings'); setWarmupMs(wu); setRingMs(rm);}catch{}
    })();
  },[]);

  useEffect(()=>{
    if(activeTab!=='sync') return;
    setDelays(prev=>{ const next={...prev}; mirrorSelection.forEach(id=>{ const v=prev[id]; next[id]=Number.isFinite(v as number)?(v as number):0; }); return next; });
  },[activeTab, mirrorSelection, audioDevices]);

  const setDeviceVolume = async (id:string, v:number)=>{
    const volume = Math.max(0,Math.min(1,v));
    try{ await invoke('set_render_volume', { deviceId:id, volume }); setAudioDevices(prev=>prev.map(d=>d.id===id?{...d, volume}:d)); }
    catch(e:any){ setAudioError(e?.message ?? String(e)); }
  };

  if(loading || !status){
    return (
      <main>
        <header className="section-heading"><h1>{t('app_title')}</h1><span className="status-pill">{t('loading_label')}:</span></header>
        <div className="card empty">{t('loading_label')}:</div>
      </main>
    );
  }

  return (
    <main>
      <header className="section-heading">
        <div><h1>{t('app_title')}</h1></div>
      </header>
      {error && <div className="card" style={{ color:'#dc2626' }}>{error}</div>}
      <div style={{ display:'flex', gap:8, marginBottom:12 }}>
        <button className={activeTab==='devices'?'':'secondary'} onClick={()=>setActiveTab('devices')}>{t('tab_devices')}</button>
        <button className={activeTab==='sync'?'':'secondary'} onClick={()=>setActiveTab('sync')}>{t('tab_sync')}</button>
      </div>

      {activeTab==='devices' && (
        <DevicesTab
          audioDevices={audioDevices}
          mirrorSelection={mirrorSelection}
          setMirrorSelection={(ids)=> setMirrorSelection(ids)}
          setDeviceVolume={setDeviceVolume}
          loadAudioContext={loadAudioContext}
          audioLoading={audioLoading}
          audioError={audioError}
          mirrorStatus={mirrorStatus}
          setAudioError={(s)=> setAudioError(s)}
        />
      )}

      {activeTab==='sync' && (
        <SyncTab
          warmupMs={warmupMs}
          setWarmupMs={setWarmupMs}
          ringMs={ringMs}
          setRingMs={setRingMs}
          mirrorSelection={mirrorSelection}
          audioDevices={audioDevices}
          delays={delays}
          setDelays={(updater)=> setDelays(prev=>updater(prev))}
        />
      )}
      <footer></footer>
    </main>
  );
}

