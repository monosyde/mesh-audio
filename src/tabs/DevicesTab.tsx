import { invoke } from "@tauri-apps/api/tauri";
import { t } from "../i18n";

export interface AudioDevice { id:string; displayName:string; volume:number; isDefault:boolean }
export interface MirrorStatus { enabled:boolean; targets: AudioDevice[] }

interface Props {
  audioDevices: AudioDevice[];
  mirrorSelection: string[];
  setMirrorSelection: (ids: string[]) => void;
  setDeviceVolume: (id: string, v: number) => Promise<void> | void;
  loadAudioContext: () => Promise<void>;
  audioLoading: boolean;
  audioError: string | null;
  mirrorStatus: MirrorStatus | null;
  setAudioError: (s: string | null) => void;
}

export default function DevicesTab(props: Props) {
  const { audioDevices, mirrorSelection, setMirrorSelection, setDeviceVolume, loadAudioContext, audioLoading, audioError, mirrorStatus, setAudioError } = props;

  const handleMirrorToggle = (id: string) => {
    setMirrorSelection(
      mirrorSelection.includes(id)
        ? mirrorSelection.filter((x) => x !== id)
        : [...mirrorSelection, id]
    );
  };

  const applyMirrorSelection = async () => {
    try {
      if (mirrorSelection.length === 0) {
        await invoke<void>("disable_audio_mirror");
        return;
      }
      await invoke("enable_audio_mirror", { deviceIds: mirrorSelection });
      setAudioError(null);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setAudioError(message);
    }
  };

  const disableMirror = async () => {
    try {
      await invoke<void>("disable_audio_mirror");
      setMirrorSelection([]);
      setAudioError(null);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setAudioError(message);
    }
  };

  return (
    <section className="card">
      <div className="section-heading">
        <h2>{t('devices_title')}</h2>
        <button className="secondary" onClick={loadAudioContext} disabled={audioLoading}>
          {audioLoading ? `${t('loading_label')}…` : t('devices_refresh')}
        </button>
      </div>
      {audioError && <div style={{ color: '#dc2626', marginBottom: 12 }}>{audioError}</div>}
      {audioDevices.length === 0 ? (
        <div className="empty">{t('devices_empty')}</div>
      ) : (
        <ul className="config-list">
          {audioDevices.map((device) => (
            <li key={device.id} className={`config-item ${mirrorSelection.includes(device.id) ? 'active' : ''}`} onClick={() => { if (!device.isDefault) handleMirrorToggle(device.id); }}>
              <div className="name">
                <span style={{ whiteSpace: 'pre-line' }}>{device.displayName}</span>
                <small>{device.isDefault ? t('devices_default') : t('devices_secondary')}</small>
              </div>
              <div className="controls" style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center' }}>
                <input type="checkbox" checked={mirrorSelection.includes(device.id)} disabled={device.isDefault}
                  onChange={(e) => { e.stopPropagation(); handleMirrorToggle(device.id); }} />
                <input type="range" min={0} max={100} value={Math.round((device.volume ?? 1) * 100)}
                  onMouseDown={(e) => e.stopPropagation()} onPointerDown={(e) => e.stopPropagation()} onClick={(e) => e.stopPropagation()}
                  onChange={(e) => { e.stopPropagation(); const val = Number(e.currentTarget.value) / 100; setDeviceVolume(device.id, val); }}
                  style={{ marginLeft: 12, width: 140 }} title={`Volume: ${Math.round((device.volume ?? 1) * 100)}%`} />
              </div>
            </li>
          ))}
        </ul>
      )}
      <div style={{ display: 'flex', gap: 12, marginTop: 24 }}>
        <button onClick={applyMirrorSelection} disabled={audioDevices.length === 0}>
          {mirrorSelection.length === 0 ? t('disable_mirroring') : t('apply_selection')}
        </button>
        <button className="secondary" onClick={disableMirror}>{t('stop_mirror')}</button>
      </div>
      <div style={{ marginTop: 16 }}>
        <strong>{t('mirrored_targets')}</strong>{' '}
        {mirrorStatus && mirrorStatus.enabled && mirrorStatus.targets.length > 0
          ? mirrorStatus.targets.map((tgt) => { const [primary] = tgt.displayName.split('\n'); return primary || tgt.displayName; }).join(', ')
          : t('none')}
      </div>
    </section>
  );
}

