# Android release status

This directory contains the Android release candidate with a minimal Java Activity adapter. The Android
application layer is Java, while USB transport and all data movement are implemented in Rust.

## Implemented

- Small Java Activity adapter for Android lifecycle, SAF image selection, and USB permission.
- Release native libraries for `arm64-v8a`, `armeabi-v7a`, and `x86_64`.
- Userspace USB Mass Storage Bulk-Only Transport implementation.
- SCSI inquiry, capacity discovery, read/write, cache synchronization, and SHA-256 support.
- No root and no Java/Kotlin application classes.

## Validation requirement

The APK can now select an image, request USB permission, discover BOT endpoints, write through Rust, flush,
and optionally read back for verification. The BOT implementation must still be tested against real devices
before a public production release. Building and signing an APK is not a substitute for hardware validation.
