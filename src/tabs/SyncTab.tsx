import { invoke } from "@tauri-apps/api/tauri";
import { t } from "../i18n";
import type { AudioDevice } from "./DevicesTab";

interface Props {
  warmupMs: number;
  setWarmupMs: (n: number) => void;
  ringMs: number;
  setRingMs: (n: number) => void;
  mirrorSelection: string[];
  audioDevices: AudioDevice[];
  delays: Record<string, number>;
  setDelays: (updater: (prev: Record<string, number>) => Record<string, number>) => void;
}

export default function SyncTab(props: Props) {
  const { warmupMs, setWarmupMs, ringMs, setRingMs, mirrorSelection, audioDevices, delays, setDelays } = props;

  return (
    <section className="card">
      <div className="section-heading"><h2>{t('sync_title')}</h2></div>
      <div style={{ display: 'flex', gap: 24, flexWrap: 'wrap' }}>
        <label>{t('warmup_ms')}
          <input type="number" min={0} max={200} value={warmupMs}
            onChange={(e)=> setWarmupMs(Number(e.currentTarget.value)||0)}
            style={{ marginLeft: 8, width: 90 }} />
        </label>
        <label>{t('ring_ms')}
          <input type="number" min={100} max={2000} value={ringMs}
            onChange={(e)=> setRingMs(Number(e.currentTarget.value)||0)}
            style={{ marginLeft: 8, width: 100 }} />
        </label>
        <button onClick={async ()=>{ try { await invoke('set_audio_settings', { warmupMs, ringMs }); } catch(e){ console.error(e);} }}>{t('apply_settings')}</button>
      </div>
      <div style={{ display:'flex', gap:12, alignItems:'center', marginTop:8 }}>
        <label className={`toggle`}>
          <input type="checkbox" onChange={async (e)=>{ try { await invoke('set_auto_adjust', { enabled: e.currentTarget.checked }); } catch(err){ console.error(err);} }} /> {t('auto_adjust')}
        </label>
        <button className="secondary" onClick={async ()=>{ try { await invoke('audio_resync'); } catch(e){ console.error(e);} }}>{t('resync_now')}</button>
      </div>
      <div style={{ marginTop: 16 }}>
        <h3>{t('per_device_delay')}</h3>
        {mirrorSelection.length === 0 ? (
          <div className="empty">{t('select_on_devices')}</div>
        ) : (
          <ul className="config-list">
            {mirrorSelection.map(id => {
              const dev = audioDevices.find(d=>d.id===id);
              const name = dev?.displayName || id;
              const val = delays[id] ?? 0;
              return (
                <li key={id} className="config-item">
                  <div className="name"><span>{name}</span><small>{id}</small></div>
                  <div className="controls">
                    <input type="number" min={0} max={2000} value={Number.isFinite(val) ? val : 0}
                      onChange={(e)=> {
                        const raw = e.currentTarget.value; let n = parseInt(raw,10);
                        if(!Number.isFinite(n)) n=0; if(n<0) n=0; if(n>2000) n=2000;
                        setDelays(prev=>({ ...prev, [id]: n }));
                      }}
                      style={{ width: 100, marginRight: 8 }} />
                    <button onClick={async ()=>{ try { await invoke('set_device_delay', { deviceId: id, delayMs: (Number.isFinite(val)?val:0) }); } catch(e){ console.error(e);} }}>Apply</button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </section>
  );
}

