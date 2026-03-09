# mesh-audio

## English
Lightweight Windows app for output mirroring and sync across multiple speakers (including Bluetooth).

### What it does
- Mirrors audio from the current default output to selected secondary outputs.
- Tunes sync with global delay, signed per-device offsets, and auto drift adjustment.
- Supports an optional VB-Cable workflow in the UI.

### Tabs
- `Devices`: targets, mirror start/stop, per-device volume.
- `VB-Cable`: install/check VB-Cable and route validation.
- `Sync`: global delay, warmup/ring, offsets, re-sync.

### Quick start
1. Select mirror targets in `Devices`.
2. (Optional) Prepare virtual route in `VB-Cable`.
3. Apply profile and offsets in `Sync`.
4. Enable `Auto adjust` if Bluetooth drift appears.

### Build
```bash
npm run build
cargo build --release --manifest-path src-tauri/Cargo.toml
```

Release binary: `src-tauri/target/release/mesh-audio.exe`

## Русский
Легкое Windows-приложение для зеркалирования звука и синхронизации нескольких колонок (включая Bluetooth).

### Что умеет
- Зеркалирует звук с текущего устройства по умолчанию на выбранные дополнительные устройства.
- Настраивает синхронизацию через global delay, signed offset на устройство и автоподстройку дрейфа.
- Поддерживает сценарий VB-Cable прямо в интерфейсе.

### Вкладки
- `Устройства`: выбор целей, старт/стоп зеркалирования, громкость устройств.
- `VB-Cable`: установка/проверка VB-Cable и проверка маршрута.
- `Синхронизация`: global delay, warmup/ring, offsets, ресинхронизация.

### Быстрый старт
1. Выбери цели зеркалирования во вкладке `Устройства`.
2. (Опционально) Подготовь виртуальный маршрут во вкладке `VB-Cable`.
3. Примени профиль и offsets во вкладке `Синхронизация`.
4. Включи `Автоподстройку`, если есть BT-дрейф.

### Сборка
```bash
npm run build
cargo build --release --manifest-path src-tauri/Cargo.toml
```

Release-файл: `src-tauri/target/release/mesh-audio.exe`
