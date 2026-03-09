import { invoke } from "@tauri-apps/api/tauri";
import { t } from "../i18n";
import type { AudioDevice } from "./DevicesTab";

interface Props {
  warmupMs: number;
  setWarmupMs: (n: number) => void;
  ringMs: number;
  setRingMs: (n: number) => void;
  globalDelayMs: number;
  setGlobalDelayMs: (n: number) => void;
  autoAdjust: boolean;
  setAutoAdjust: (v: boolean) => void;
  mirrorSelection: string[];
  audioDevices: AudioDevice[];
  delays: Record<string, number>;
  setDelays: (updater: (prev: Record<string, number>) => Record<string, number>) => void;
}

const NUDGE_STEPS = [-20, -5, -1, 1, 5, 20];

export default function SyncTab(props: Props) {
  const {
    warmupMs,
    setWarmupMs,
    ringMs,
    setRingMs,
    globalDelayMs,
    setGlobalDelayMs,
    autoAdjust,
    setAutoAdjust,
    mirrorSelection,
    audioDevices,
    delays,
    setDelays,
  } = props;

  const selectedDevices = mirrorSelection
    .map((id) => ({ id, device: audioDevices.find((d) => d.id === id) }))
    .filter((x) => Boolean(x.device));

  const maxAbsOffset = Math.max(1, ...selectedDevices.map((x) => Math.abs(delays[x.id] ?? 0)));

  const applySettings = async () => {
    try {
      await invoke("set_audio_settings", { warmupMs, ringMs, globalDelayMs });
    } catch (e) {
      console.error(e);
    }
  };

  const setDeviceOffset = async (id: string, offsetMs: number) => {
    const clamped = Math.max(-800, Math.min(800, offsetMs));
    setDelays((prev) => ({ ...prev, [id]: clamped }));
    try {
      await invoke("set_device_delay", { deviceId: id, delayMs: clamped });
    } catch (e) {
      console.error(e);
    }
  };

  const applyAllOffsets = async () => {
    try {
      await Promise.all(
        selectedDevices.map(({ id }) =>
          invoke("set_device_delay", {
            deviceId: id,
            delayMs: Number.isFinite(delays[id]) ? delays[id] : 0,
          })
        )
      );
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <section className="card">
      <div className="section-heading"><h2>{t("sync_title")}</h2></div>

      <div className="card" style={{ marginTop: 12, padding: 14 }}>
        <strong>Как настроить</strong>
        <div style={{ marginTop: 8, opacity: 0.9 }}>1. На вкладке устройств включи зеркалирование на нужные колонки.</div>
        <div style={{ opacity: 0.9 }}>2. Поставь Global Delay так, чтобы всем колонкам хватало буфера (обычно 60-140ms).</div>
        <div style={{ opacity: 0.9 }}>3. У ранних колонок ставь Offset в плюс, у поздних в минус.</div>
        <div style={{ opacity: 0.9 }}>4. Нажми Apply all offsets и Re-sync, затем включи Auto adjust.</div>
      </div>

      <div style={{ marginTop: 14, display: "grid", gap: 12 }}>
        <label>
          Global mirror delay: <strong>{globalDelayMs} ms</strong>
          <input
            type="range"
            min={20}
            max={400}
            step={1}
            value={globalDelayMs}
            onChange={(e) => setGlobalDelayMs(Number(e.currentTarget.value) || 20)}
            style={{ width: "100%" }}
          />
        </label>

        <label>
          {t("warmup_ms")}: <strong>{warmupMs}</strong>
          <input
            type="range"
            min={0}
            max={200}
            step={1}
            value={warmupMs}
            onChange={(e) => setWarmupMs(Number(e.currentTarget.value) || 0)}
            style={{ width: "100%" }}
          />
        </label>

        <label>
          {t("ring_ms")}: <strong>{ringMs}</strong>
          <input
            type="range"
            min={120}
            max={1200}
            step={10}
            value={ringMs}
            onChange={(e) => setRingMs(Number(e.currentTarget.value) || 120)}
            style={{ width: "100%" }}
          />
        </label>

        <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          <button onClick={applySettings}>{t("apply_settings")}</button>
          <button className="secondary" onClick={async () => { try { await invoke("audio_resync"); } catch (e) { console.error(e); } }}>
            {t("resync_now")}
          </button>
          <label className={`toggle ${autoAdjust ? "active" : ""}`}>
            <input
              type="checkbox"
              checked={autoAdjust}
              onChange={async (e) => {
                const enabled = e.currentTarget.checked;
                setAutoAdjust(enabled);
                try {
                  await invoke("set_auto_adjust", { enabled });
                } catch (err) {
                  console.error(err);
                }
              }}
            />
            {t("auto_adjust")}
          </label>
        </div>
      </div>

      <div style={{ marginTop: 18 }}>
        <h3>Device offset (ms)</h3>
        {selectedDevices.length === 0 ? (
          <div className="empty">{t("select_on_devices")}</div>
        ) : (
          <>
            <ul className="config-list">
              {selectedDevices.map(({ id, device }) => {
                const name = device?.displayName || id;
                const offset = Number.isFinite(delays[id]) ? delays[id] : 0;
                const ratio = Math.max(-1, Math.min(1, offset / maxAbsOffset));
                const left = ratio < 0 ? `${50 + ratio * 50}%` : "50%";
                const width = `${Math.abs(ratio) * 50}%`;
                return (
                  <li key={id} className="config-item" style={{ display: "block" }}>
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 12 }}>
                      <div className="name" style={{ minWidth: 0 }}>
                        <span>{name}</span>
                        <small>{id}</small>
                      </div>
                      <strong>{offset > 0 ? `+${offset}` : offset} ms</strong>
                    </div>

                    <div style={{ marginTop: 10 }}>
                      <div style={{ position: "relative", height: 10, borderRadius: 999, background: "rgba(15,23,42,.1)" }}>
                        <div style={{ position: "absolute", left: "50%", top: 0, bottom: 0, width: 1, background: "rgba(15,23,42,.35)" }} />
                        <div
                          style={{
                            position: "absolute",
                            left,
                            top: 0,
                            height: "100%",
                            width,
                            borderRadius: 999,
                            background: ratio >= 0 ? "linear-gradient(90deg,#22c55e,#3b82f6)" : "linear-gradient(90deg,#f59e0b,#ef4444)",
                            transition: "all 120ms ease",
                          }}
                        />
                      </div>
                    </div>

                    <div style={{ marginTop: 10, display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                      {NUDGE_STEPS.map((step) => (
                        <button
                          key={`${id}-${step}`}
                          className="secondary"
                          onClick={() => void setDeviceOffset(id, offset + step)}
                        >
                          {step > 0 ? `+${step}` : step}
                        </button>
                      ))}
                      <input
                        type="number"
                        min={-800}
                        max={800}
                        value={offset}
                        onChange={(e) => {
                          const val = Math.max(-800, Math.min(800, Number(e.currentTarget.value) || 0));
                          setDelays((prev) => ({ ...prev, [id]: val }));
                        }}
                        style={{ width: 90, marginLeft: 4 }}
                      />
                      <button className="secondary" onClick={() => void setDeviceOffset(id, offset)}>Apply</button>
                    </div>
                  </li>
                );
              })}
            </ul>

            <div style={{ marginTop: 12, display: "flex", gap: 10 }}>
              <button onClick={applyAllOffsets}>Apply all offsets</button>
            </div>
          </>
        )}
      </div>
    </section>
  );
}
