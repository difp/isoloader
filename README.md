# ISOLoader

Open-source Android ISO/IMG to USB writer with a Rust USB Mass Storage core.

## Features

- Android USB Host / OTG, no root required.
- ISO/IMG selection through the system document picker.
- USB Mass Storage Bulk-Only Transport (BOT) and SCSI implemented in Rust.
- `INQUIRY`, `TEST UNIT READY`, `READ CAPACITY(10/16)`, `READ(16)`, `WRITE(16)`, and cache sync.
- Optional read-back verification with SHA-256.
- CLI companion in `cli/`.
- Android ABIs: `arm64-v8a`, `armeabi-v7a`, and `x86_64`.

## Build

### CLI

```bash
cd cli
cargo build --release
```

### Android native core

Install Android NDK and Rust Android targets, then build `android/` for the required ABI. The checked-in
`.cargo/config.toml` contains local paths from the development machine and should be adjusted for another
environment.

### APK

`android-app/` is a minimal SDK-style Android project without Gradle. It uses a small Java Activity only
for Android lifecycle, SAF file selection, USB permission, and `UsbDeviceConnection`; all storage protocol
and data writing is Rust code.

The APK is assembled with `aapt2`, `javac`, `d8`, `zipalign`, and `apksigner`. Do not use the development
keystore for a public release; configure your own signing key.

## Safety

Flashing irreversibly overwrites the selected USB device. The app checks the image size against the reported
device capacity and asks for explicit confirmation. Test against disposable hardware before distributing.

Hardware validation on real Android phones and multiple USB bridges is still required before claiming broad
device compatibility.
