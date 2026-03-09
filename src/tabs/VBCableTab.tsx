import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import type { AudioDevice } from "./DevicesTab";
import { t } from "../i18n";

interface Props {
  audioDevices: AudioDevice[];
  mirrorSelection: string[];
  setMirrorSelection: (ids: string[]) => void;
  loadAudioContext: () => Promise<void>;
}

interface VbProfile {
  id: string;
  name: string;
  warmupMs: number;
  ringMs: number;
  globalDelayMs: number;
  autoAdjust: boolean;
}

const VB_PROFILES: VbProfile[] = [
  { id: "music-room", name: "Music Room", warmupMs: 50, ringMs: 300, globalDelayMs: 90, autoAdjust: true },
  { id: "movie-tv", name: "Movie / TV", warmupMs: 80, ringMs: 500, globalDelayMs: 140, autoAdjust: true },
  { id: "low-latency", name: "Low Latency", warmupMs: 20, ringMs: 180, globalDelayMs: 45, autoAdjust: false },
  { id: "bt-stable", name: "BT Stable", warmupMs: 70, ringMs: 650, globalDelayMs: 120, autoAdjust: true },
  { id: "front-near-rear-far", name: "Front Near / Rear Far", warmupMs: 70, ringMs: 550, globalDelayMs: 120, autoAdjust: true },
  { id: "party-max-stability", name: "Party (Max Stability)", warmupMs: 90, ringMs: 850, globalDelayMs: 170, autoAdjust: true },
  { id: "speech-podcast", name: "Speech / Podcast", warmupMs: 35, ringMs: 240, globalDelayMs: 80, autoAdjust: false },
  { id: "gaming-balanced", name: "Gaming (Balanced)", warmupMs: 25, ringMs: 200, globalDelayMs: 60, autoAdjust: false },
];

export default function VBCableTab(props: Props) {
  const { audioDevices, mirrorSelection, setMirrorSelection, loadAudioContext } = props;
  const [vbInstalled, setVbInstalled] = useState<boolean | null>(null);
  const [installing, setInstalling] = useState(false);
  const [applyingProfile, setApplyingProfile] = useState<string | null>(null);
  const [delays, setDelays] = useState<Record<string, number>>({});
  const [showStepsHint, setShowStepsHint] = useState(false);
  const [selectedProfileId, setSelectedProfileId] = useState(VB_PROFILES[0].id);

  const defaultDevice = audioDevices.find((d) => d.isDefault);
  const hasVbAsDefault = Boolean(defaultDevice && /cable/i.test(defaultDevice.displayName));
  const secondaryOutputs = audioDevices.filter((d) => !d.isDefault);
  const activeMirrorOutputs = secondaryOutputs.filter((d) => mirrorSelection.includes(d.id));

  const checkVbCable = async () => {
    try {
      const installed = await invoke<boolean>("check_vbcable_installed");
      setVbInstalled(installed);
    } catch {
      setVbInstalled(false);
    }
  };

  useEffect(() => {
    void checkVbCable();
    void refreshDelays();
  }, [audioDevices.length]);

  const refreshDelays = async () => {
    try {
      const next = await invoke<Record<string, number>>("get_device_delays");
      setDelays(next);
    } catch {
      setDelays({});
    }
  };

  const installVbCable = async () => {
    setInstalling(true);
    try {
      const installed = await invoke<boolean>("install_vbcable");
      setVbInstalled(installed);
      await loadAudioContext();
    } catch (e) {
      console.error(e);
    } finally {
      setInstalling(false);
    }
  };

  const applySyncProfile = async (profile: VbProfile) => {
    const { id, warmupMs, ringMs, globalDelayMs, autoAdjust } = profile;
    setApplyingProfile(id);
    try {
      await invoke("set_audio_settings", { warmupMs, ringMs, globalDelayMs });
      await invoke("set_auto_adjust", { enabled: autoAdjust });
      await invoke("audio_resync");
    } catch (e) {
      console.error(e);
    } finally {
      setApplyingProfile(null);
    }
  };

  const handleMirrorToggle = (id: string) => {
    setMirrorSelection(
      mirrorSelection.includes(id)
        ? mirrorSelection.filter((x) => x !== id)
        : [...mirrorSelection, id]
    );
  };

  const applySelectedProfile = async () => {
    const profile = VB_PROFILES.find((p) => p.id === selectedProfileId);
    if (!profile) return;
    await applySyncProfile(profile);
  };

  const resetAllDelays = async () => {
    setApplyingProfile("reset-delays");
    try {
      const currentDelays = await invoke<Record<string, number>>("get_device_delays");
      await Promise.all(
        Object.keys(currentDelays).map((deviceId) =>
          invoke("set_device_delay", { deviceId, delayMs: 0 })
        )
      );
      await invoke("audio_resync");
      await refreshDelays();
    } catch (e) {
      console.error(e);
    } finally {
      setApplyingProfile(null);
    }
  };

  const setDeviceDelay = async (deviceId: string, delayMs: number) => {
    const clamped = Math.max(-800, Math.min(800, delayMs));
    setDelays((prev) => ({ ...prev, [deviceId]: clamped }));
    try {
      await invoke("set_device_delay", { deviceId, delayMs: clamped });
    } catch (e) {
      console.error(e);
    }
  };

  const applyAllDelays = async () => {
    setApplyingProfile("apply-all-delays");
    try {
      const entries = Object.entries(delays);
      await Promise.all(
        entries.map(([deviceId, delayMs]) =>
          invoke("set_device_delay", {
            deviceId,
            delayMs: Number.isFinite(delayMs) ? delayMs : 0,
          })
        )
      );
      await invoke("audio_resync");
      await refreshDelays();
    } catch (e) {
      console.error(e);
    } finally {
      setApplyingProfile(null);
    }
  };

  return (
    <section className="card">
      <div className="section-heading">
        <h2>{t("vb_title")}</h2>
      </div>
      <p style={{ marginTop: 0, opacity: 0.9 }}>{t("vb_intro")}</p>

      <div className="card" style={{ marginTop: 12, padding: 14 }}>
        <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 10 }}>
          <strong>{t("vb_status_title")}</strong>
          <button
            className="secondary"
            onClick={() => setShowStepsHint((prev) => !prev)}
            title={t("vb_steps_hint_title")}
            style={{ minWidth: 0, padding: "4px 8px", lineHeight: 1.1 }}
          >
            ?
          </button>
        </div>
        <div style={{ marginTop: 8 }}>
          VB-Cable: <strong style={{ color: vbInstalled ? "#15803d" : "#b45309" }}>
            {vbInstalled === null ? t("vb_status_checking") : vbInstalled ? t("vb_status_ok") : t("vb_status_missing")}
          </strong>
        </div>
        <div style={{ marginTop: 8 }}>
          Default output: <strong>{defaultDevice?.displayName ?? t("vb_default_unknown")}</strong>
        </div>
        <div style={{ marginTop: 6, color: hasVbAsDefault ? "#15803d" : "#b45309" }}>
          {hasVbAsDefault ? t("vb_default_ok") : t("vb_default_warn")}
        </div>
        <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button onClick={installVbCable} disabled={installing}>
            {installing ? t("vb_installing") : t("vb_install")}
          </button>
          <button className="secondary" onClick={() => void checkVbCable()}>{t("vb_recheck")}</button>
          <button className="secondary" onClick={async () => { await loadAudioContext(); await refreshDelays(); }}>{t("devices_refresh")}</button>
        </div>
        {showStepsHint && (
          <div className="card" style={{ marginTop: 10, padding: 10 }}>
            <strong>{t("vb_steps_hint_title")}</strong>
            <ol style={{ marginTop: 8, marginBottom: 0, paddingLeft: 20, display: "grid", gap: 6 }}>
              <li>{t("vb_step_1")}</li>
              <li>{t("vb_step_2")}</li>
              <li>{t("vb_step_3")}</li>
              <li>{t("vb_step_4")}</li>
              <li>{t("vb_step_5")}</li>
              <li>{t("vb_step_6")}</li>
            </ol>
          </div>
        )}
        <small style={{ display: "block", marginTop: 10, opacity: 0.8 }}>{t("vb_reboot_hint")}</small>
      </div>

      <div className="card" style={{ marginTop: 12, padding: 14 }}>
        <strong>{t("vb_active_outputs")}</strong>
        {audioDevices.length === 0 ? (
          <div style={{ marginTop: 10, opacity: 0.85 }}>
            {t("devices_empty")}
          </div>
        ) : (
          <ul className="config-list" style={{ marginTop: 10 }}>
            {defaultDevice && (
              <li key={defaultDevice.id} className="config-item">
                <div className="name">
                  <span style={{ whiteSpace: "pre-line" }}>{defaultDevice.displayName}</span>
                  <small>{t("devices_default")}</small>
                </div>
                <div className="controls" style={{ marginLeft: "auto", display: "flex", alignItems: "center" }}>
                  <input type="checkbox" checked disabled />
                </div>
              </li>
            )}
            {secondaryOutputs.map((d) => (
              <li
                key={d.id}
                className={`config-item ${mirrorSelection.includes(d.id) ? "active" : ""}`}
                onClick={() => handleMirrorToggle(d.id)}
              >
                <div className="name">
                  <span style={{ whiteSpace: "pre-line" }}>{d.displayName}</span>
                  <small>{t("devices_secondary")}</small>
                </div>
                <div className="controls" style={{ marginLeft: "auto", display: "flex", alignItems: "center" }}>
                  <input
                    type="checkbox"
                    checked={mirrorSelection.includes(d.id)}
                    onChange={(e) => {
                      e.stopPropagation();
                      handleMirrorToggle(d.id);
                    }}
                  />
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="card" style={{ marginTop: 12, padding: 14 }}>
        <strong>{t("vb_profiles_title")}</strong>
        <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
          <select
            value={selectedProfileId}
            onChange={(e) => setSelectedProfileId(e.currentTarget.value)}
            style={{ minWidth: 260 }}
          >
            {VB_PROFILES.map((profile) => (
              <option key={profile.id} value={profile.id}>
                {profile.name}
              </option>
            ))}
          </select>
          <button className="secondary" onClick={() => void applySelectedProfile()} disabled={applyingProfile !== null}>
            {t("vb_apply_profile")}
          </button>
          <button className="secondary" onClick={() => void invoke("audio_resync")} disabled={applyingProfile !== null}>
            {t("resync_now")}
          </button>
        </div>
        {applyingProfile && (
          <small style={{ display: "block", marginTop: 8, opacity: 0.8 }}>
            Applying: {applyingProfile}
          </small>
        )}
      </div>

      <div className="card" style={{ marginTop: 12, padding: 14 }}>
        <strong>{t("vb_device_delays_title")}</strong>
        {activeMirrorOutputs.length === 0 ? (
          <div style={{ marginTop: 10, opacity: 0.85 }}>
            {t("vb_no_active_mirrors")}
          </div>
        ) : (
          <ul className="config-list" style={{ marginTop: 10 }}>
            {activeMirrorOutputs.map((d) => {
              const val = Number.isFinite(delays[d.id]) ? delays[d.id] : 0;
              return (
                <li key={d.id} className="config-item" style={{ display: "block" }}>
                  <div style={{ display: "flex", justifyContent: "space-between", gap: 8 }}>
                    <span>{d.displayName}</span>
                    <strong>{val > 0 ? `+${val}` : val} ms</strong>
                  </div>
                  <div style={{ marginTop: 8, display: "flex", gap: 6, flexWrap: "wrap" }}>
                    {[-20, -5, -1, 1, 5, 20].map((step) => (
                      <button
                        key={`${d.id}-${step}`}
                        className="secondary"
                        onClick={() => void setDeviceDelay(d.id, val + step)}
                      >
                        {step > 0 ? `+${step}` : step}
                      </button>
                    ))}
                    <input
                      type="number"
                      min={-800}
                      max={800}
                      value={val}
                      onChange={(e) => {
                        const raw = Number(e.currentTarget.value);
                        const next = Number.isFinite(raw) ? Math.max(-800, Math.min(800, raw)) : 0;
                        setDelays((prev) => ({ ...prev, [d.id]: next }));
                      }}
                      style={{ width: 90 }}
                    />
                    <button className="secondary" onClick={() => void setDeviceDelay(d.id, val)}>Apply</button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        <div style={{ marginTop: 12, display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button onClick={() => void applyAllDelays()} disabled={applyingProfile !== null}>{t("vb_apply_all_delays")}</button>
          <button className="secondary" onClick={() => void resetAllDelays()} disabled={applyingProfile !== null}>{t("vb_reset_all_delays")}</button>
        </div>
      </div>
    </section>
  );
}
