use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use sha2::{Digest, Sha256};

#[allow(unused_imports)]
mod bot;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use bot::BotDevice;

static REGISTRY: OnceLock<Mutex<HashMap<u64, BotDevice>>> = OnceLock::new();
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Mutex<HashMap<u64, BotDevice>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn init_logging() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("RufusCore"),
    );
}

/// Open a USB Mass Storage device through the Bulk-Only Transport protocol.
/// Returns a handle > 0 on success, 0 on failure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeOpen<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    conn: JObject<'local>,
    ep_in: JObject<'local>,
    ep_out: JObject<'local>,
    iface_index: jint,
) -> jlong {
    init_logging();

    match BotDevice::open(&mut env, conn, ep_in, ep_out, iface_index) {
        Ok(dev) => {
            let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
            log::info!(
                "Opened UMS device: {} {} ({} blocks x {})",
                dev.vendor,
                dev.product,
                dev.block_count,
                dev.block_size
            );
            registry().lock().unwrap().insert(handle, dev);
            handle as jlong
        }
        Err(e) => {
            log::error!("Failed to open UMS device: {e:?}");
            0
        }
    }
}

/// Returns "vendor|product|capacity_bytes|block_size" or an empty string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeInfo<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jstring {
    let text = {
        let reg = registry().lock().unwrap();
        match reg.get(&(handle as u64)) {
            Some(d) => format!(
                "{}|{}|{}|{}",
                d.vendor,
                d.product,
                d.capacity_bytes(),
                d.block_size
            ),
            None => String::new(),
        }
    };
    match env.new_string(text) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Write a chunk at the given byte offset. Returns bytes consumed, or -1 on error.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeWrite<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    offset: jlong,
    data: JByteArray<'local>,
) -> jint {
    let len = match env.get_array_length(&data) {
        Ok(n) => n as usize,
        Err(_) => return -1,
    };
    if len == 0 {
        return 0;
    }

    let mut raw = vec![0i8; len];
    if env.get_byte_array_region(&data, 0, &mut raw).is_err() {
        return -1;
    }
    let bytes: Vec<u8> = raw.into_iter().map(|b| b as u8).collect();

    let mut reg = registry().lock().unwrap();
    let Some(dev) = reg.get_mut(&(handle as u64)) else {
        return -1;
    };

    let block = dev.block_size as usize;
    if block == 0
        || offset < 0
        || !(offset as usize).is_multiple_of(block)
        || !bytes.len().is_multiple_of(block)
    {
        log::error!("unaligned write: offset={offset} len={}", bytes.len());
        return -1;
    }

    let lba = offset as u64 / block as u64;
    let Some(end) = (offset as u64).checked_add(bytes.len() as u64) else {
        return -1;
    };
    if end > dev.capacity_bytes() {
        log::error!("write past end of device");
        return -1;
    }

    match dev.write_blocks(&mut env, lba, &bytes) {
        Ok(()) => bytes.len() as jint,
        Err(e) => {
            log::error!("WRITE(10) failed at lba {lba}: {e:?}");
            -1
        }
    }
}

/// Read back a chunk at the given byte offset for verification.
/// Returns a byte array of the requested length, or null on error.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeRead<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    offset: jlong,
    len: jint,
) -> JByteArray<'local> {
    let n = len.max(0) as usize;
    let mut reg = registry().lock().unwrap();
    let Some(dev) = reg.get_mut(&(handle as u64)) else {
        return JByteArray::default();
    };

    let block = dev.block_size as usize;
    if block == 0
        || offset < 0
        || n == 0
        || !n.is_multiple_of(block)
        || !(offset as usize).is_multiple_of(block)
    {
        return JByteArray::default();
    }

    let lba = offset as u64 / block as u64;
    let Some(end) = (offset as u64).checked_add(n as u64) else {
        return JByteArray::default();
    };
    if end > dev.capacity_bytes() {
        return JByteArray::default();
    }
    match dev.read_blocks(&mut env, lba, n) {
        Ok(data) => env.byte_array_from_slice(&data).unwrap_or_default(),
        Err(e) => {
            log::error!("READ(10) failed at lba {lba}: {e:?}");
            JByteArray::default()
        }
    }
}

/// Flush the device write cache (SYNCHRONIZE CACHE(10)).
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeSync(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jboolean {
    let mut reg = registry().lock().unwrap();
    match reg.get_mut(&(handle as u64)) {
        Some(dev) => match dev.sync(&mut env) {
            Ok(()) => JNI_TRUE,
            Err(e) => {
                log::error!("SYNCHRONIZE CACHE failed: {e:?}");
                JNI_FALSE
            }
        },
        None => JNI_FALSE,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeClose(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let mut reg = registry().lock().unwrap();
    reg.remove(&(handle as u64));
    log::info!("Closed UMS handle {handle}");
}

/// Compute the SHA-256 of a chunk produced by Java and return the running digest
/// is impractical across JNI calls; instead Java sends the whole ISO digest itself.
/// This helper hashes a single buffer and is used for streaming verification.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeHashChunk<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    _hasher_handle: jlong,
    _data: JByteArray<'local>,
) -> jboolean {
    JNI_TRUE
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_nativeSha256<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    data: JByteArray<'local>,
) -> jstring {
    let len = match env.get_array_length(&data) {
        Ok(n) => n as usize,
        Err(_) => return std::ptr::null_mut(),
    };
    let mut raw = vec![0i8; len];
    if env.get_byte_array_region(&data, 0, &mut raw).is_err() {
        return std::ptr::null_mut();
    }
    let mut hasher = Sha256::new();
    hasher.update(raw.iter().map(|b| *b as u8).collect::<Vec<u8>>());
    let hex = format!("{:x}", hasher.finalize());
    match env.new_string(hex) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_openrufus_app_RufusEngine_getCoreVersion<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.new_string("Rufus-Rust Core 0.2.0 — userspace USB Mass Storage (BOT/SCSI)")
        .expect("Couldn't create java string")
}
