use anyhow::{bail, Result};
use jni::objects::{GlobalRef, JObject, JValue};
use jni::JNIEnv;

// Bulk-Only Transport (BOT) + SCSI driver for USB Mass Storage Class devices.
// Uses Android UsbDeviceConnection.bulkTransfer through JNI.

const TIMEOUT_MS: i32 = 30_000;
const CBW_SIG: u32 = 0x4342_5355; // "USBC" on the wire
const CSW_SIG: u32 = 0x5342_5355; // "USBS" on the wire

const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_INQUIRY: u8 = 0x12;
const SCSI_READ_CAPACITY10: u8 = 0x25;
const SCSI_READ_CAPACITY16: u8 = 0x9E;
const SCSI_READ16: u8 = 0x88;
const SCSI_WRITE16: u8 = 0x8A;
const SCSI_SYNC_CACHE10: u8 = 0x35;

pub struct BotDevice {
    conn: GlobalRef,
    ep_in: GlobalRef,
    ep_out: GlobalRef,
    ep_in_addr: i32,
    ep_out_addr: i32,
    iface_index: i32,
    tag: u32,
    lun: u8,
    pub block_size: u32,
    pub block_count: u64,
    pub vendor: String,
    pub product: String,
    chunk: usize,
}

impl BotDevice {
    pub fn open(
        env: &mut JNIEnv,
        conn: JObject,
        ep_in: JObject,
        ep_out: JObject,
        iface_index: i32,
    ) -> Result<Self> {
        let conn = env.new_global_ref(conn)?;
        let ep_in = env.new_global_ref(ep_in)?;
        let ep_out = env.new_global_ref(ep_out)?;
        let ep_in_addr = endpoint_address(env, &ep_in)?;
        let ep_out_addr = endpoint_address(env, &ep_out)?;

        let mut dev = Self {
            conn,
            ep_in,
            ep_out,
            ep_in_addr,
            ep_out_addr,
            iface_index,
            tag: 1,
            lun: 0,
            block_size: 512,
            block_count: 0,
            vendor: String::new(),
            product: String::new(),
            // Android's UsbDeviceConnection.bulkTransfer(byte[]) limits each
            // transfer to 16 KiB on modern API levels.
            chunk: 16 * 1024,
        };

        dev.reset(env)?;
        dev.wait_until_ready(env)?;
        dev.identify(env)?;
        dev.read_capacity(env)?;
        Ok(dev)
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.block_count * self.block_size as u64
    }

    /// Bulk-Only Mass Storage Reset + ClearFeature(HALT) on both endpoints.
    fn reset(&mut self, env: &mut JNIEnv) -> Result<()> {
        // BOT reset and endpoint halt clear are recovery operations. A device
        // is allowed to reject a reset when it is already in the default state.
        self.control(env, 0x21, 0xFF, 0, self.iface_index)?;
        self.control(env, 0x02, 0x01, 0, self.ep_in_addr)?;
        self.control(env, 0x02, 0x01, 0, self.ep_out_addr)?;
        self.tag = 1;
        Ok(())
    }

    fn control(
        &mut self,
        env: &mut JNIEnv,
        request_type: i32,
        request: i32,
        value: i32,
        index: i32,
    ) -> Result<i32> {
        let null = JObject::null();
        let res = env.call_method(
            &self.conn,
            "controlTransfer",
            "(IIII[BIII)I",
            &[
                JValue::Int(request_type),
                JValue::Int(request),
                JValue::Int(value),
                JValue::Int(index),
                JValue::Object(&null),
                JValue::Int(0),
                JValue::Int(0),
                JValue::Int(TIMEOUT_MS),
            ],
        )?;
        let transferred = res.i()?;
        if transferred < 0 {
            bail!("USB control transfer failed: {transferred}");
        }
        Ok(transferred)
    }

    fn bulk_out(&mut self, env: &mut JNIEnv, data: &[u8]) -> Result<usize> {
        let arr = env.byte_array_from_slice(data)?;
        let res = env.call_method(
            &self.conn,
            "bulkTransfer",
            "(Landroid/hardware/usb/UsbEndpoint;[BII)I",
            &[
                JValue::Object(self.ep_out.as_obj()),
                JValue::Object(&arr.into()),
                JValue::Int(data.len() as i32),
                JValue::Int(TIMEOUT_MS),
            ],
        )?;
        let n = res.i()?;
        if n < 0 {
            bail!("bulkTransfer(OUT) failed: {}", n);
        }
        Ok(n as usize)
    }

    fn bulk_in(&mut self, env: &mut JNIEnv, out: &mut [u8]) -> Result<usize> {
        let arr = env.new_byte_array(out.len() as i32)?;
        let res = env.call_method(
            &self.conn,
            "bulkTransfer",
            "(Landroid/hardware/usb/UsbEndpoint;[BII)I",
            &[
                JValue::Object(self.ep_in.as_obj()),
                JValue::Object(arr.as_ref()),
                JValue::Int(out.len() as i32),
                JValue::Int(TIMEOUT_MS),
            ],
        )?;
        let n = res.i()?;
        if n < 0 {
            bail!("bulkTransfer(IN) failed: {}", n);
        }
        let n = n as usize;
        if n > 0 {
            let mut signed = vec![0i8; n];
            env.get_byte_array_region(&arr, 0, &mut signed)?;
            for (dst, src) in out[..n].iter_mut().zip(signed.iter()) {
                *dst = *src as u8;
            }
        }
        Ok(n)
    }

    fn write_all(&mut self, env: &mut JNIEnv, data: &[u8]) -> Result<()> {
        let mut off = 0;
        while off < data.len() {
            let end = std::cmp::min(off + self.chunk, data.len());
            let sent = self.bulk_out(env, &data[off..end])?;
            if sent == 0 {
                bail!("bulk OUT stalled at offset {off}");
            }
            off += sent;
        }
        Ok(())
    }

    fn read_exact(&mut self, env: &mut JNIEnv, out: &mut [u8]) -> Result<()> {
        let mut off = 0;
        while off < out.len() {
            let end = std::cmp::min(off + self.chunk, out.len());
            let got = self.bulk_in(env, &mut out[off..end])?;
            if got == 0 {
                bail!("bulk IN stalled at offset {off}");
            }
            off += got;
        }
        Ok(())
    }

    /// Execute one SCSI command over BOT.
    fn exec(
        &mut self,
        env: &mut JNIEnv,
        cb: [u8; 16],
        data_len: usize,
        dir_in: bool,
        payload: &[u8],
    ) -> Result<Vec<u8>> {
        if data_len > u32::MAX as usize {
            bail!("BOT transfer exceeds 4 GiB");
        }
        let tag = self.tag;
        self.tag = self.tag.wrapping_add(1);

        let mut cbw = [0u8; 31];
        cbw[0..4].copy_from_slice(&CBW_SIG.to_le_bytes());
        cbw[4..8].copy_from_slice(&tag.to_le_bytes());
        cbw[8..12].copy_from_slice(&(data_len as u32).to_le_bytes());
        cbw[12] = if dir_in { 0x80 } else { 0x00 };
        cbw[13] = self.lun;
        cbw[14] = cdb_length(cb[0]);
        cbw[15..31].copy_from_slice(&cb);

        self.write_all(env, &cbw)?;

        let mut data = vec![0u8; data_len];
        if data_len > 0 {
            if dir_in {
                self.read_exact(env, &mut data)?;
            } else {
                if payload.len() != data_len {
                    bail!("payload length {} != data length {}", payload.len(), data_len);
                }
                self.write_all(env, payload)?;
            }
        }

        let mut csw = [0u8; 13];
        self.read_exact(env, &mut csw)?;

        let sig = u32::from_le_bytes([csw[0], csw[1], csw[2], csw[3]]);
        let csw_tag = u32::from_le_bytes([csw[4], csw[5], csw[6], csw[7]]);
        let residue = u32::from_le_bytes([csw[8], csw[9], csw[10], csw[11]]) as usize;
        let status = csw[12];

        if sig != CSW_SIG {
            bail!("bad CSW signature 0x{sig:08x}");
        }
        if csw_tag != tag {
            bail!("CSW tag mismatch ({} != {})", csw_tag, tag);
        }
        if residue > data_len {
            bail!("invalid CSW residue {residue} for {data_len}-byte transfer");
        }
        if status != 0 {
            bail!("SCSI command failed with status {}", status);
        }
        if residue != 0 {
            bail!("short SCSI transfer: {residue} bytes remain");
        }

        Ok(data)
    }

    fn identify(&mut self, env: &mut JNIEnv) -> Result<()> {
        let mut cb = [0u8; 16];
        cb[0] = SCSI_INQUIRY;
        cb[4] = 36;
        let d = self.exec(env, cb, 36, true, &[])?;
        if d.len() >= 36 {
            self.vendor = String::from_utf8_lossy(&d[8..16]).trim().to_string();
            self.product = String::from_utf8_lossy(&d[16..32]).trim().to_string();
        }
        Ok(())
    }

    fn wait_until_ready(&mut self, env: &mut JNIEnv) -> Result<()> {
        let mut cb = [0u8; 16];
        cb[0] = SCSI_TEST_UNIT_READY;
        let mut last_error = None;
        for attempt in 0..20 {
            match self.exec(env, cb, 0, false, &[]) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_error = Some(e.to_string());
                    if attempt != 19 {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            }
        }
        bail!("USB storage did not become ready: {}", last_error.unwrap_or_default())
    }

    fn read_capacity(&mut self, env: &mut JNIEnv) -> Result<()> {
        let mut cb = [0u8; 16];
        cb[0] = SCSI_READ_CAPACITY10;
        let d = self.exec(env, cb, 8, true, &[])?;
        if d.len() < 8 {
            bail!("short READ CAPACITY response");
        }
        let bs = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
        if bs == 0 {
            bail!("device reported zero logical block size");
        }
        self.block_size = bs;

        let last_lba = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
        if last_lba != u32::MAX {
            self.block_count = last_lba as u64 + 1;
        } else {
            // READ CAPACITY(10) returns 0xffffffff when the device is larger
            // than 2 TiB. Fall back to READ CAPACITY(16).
            let mut cap16 = [0u8; 16];
            cap16[0] = SCSI_READ_CAPACITY16;
            cap16[1] = 0x10; // service action
            cap16[13] = 32;
            let d16 = self.exec(env, cap16, 32, true, &[])?;
            if d16.len() < 12 {
                bail!("short READ CAPACITY(16) response");
            }
            let last = u64::from_be_bytes([
                d16[0], d16[1], d16[2], d16[3], d16[4], d16[5], d16[6], d16[7],
            ]);
            let bs16 = u32::from_be_bytes([d16[8], d16[9], d16[10], d16[11]]);
            if bs16 == 0 {
                bail!("device reported zero logical block size");
            }
            self.block_size = bs16;
            self.block_count = last.checked_add(1).ok_or_else(|| anyhow::anyhow!("capacity overflow"))?;
        }

        Ok(())
    }

    pub fn write_blocks(&mut self, env: &mut JNIEnv, lba: u64, data: &[u8]) -> Result<()> {
        let block = self.block_size as usize;
        if data.is_empty() || !data.len().is_multiple_of(block) {
            bail!("unaligned or empty WRITE(10) request");
        }
        let blocks = data.len() / block;
        if blocks > u32::MAX as usize {
            bail!("WRITE(16) request exceeds 2^32-1 blocks");
        }
        let mut cb = [0u8; 16];
        cb[0] = SCSI_WRITE16;
        cb[2..10].copy_from_slice(&lba.to_be_bytes());
        cb[10..14].copy_from_slice(&(blocks as u32).to_be_bytes());
        self.exec(env, cb, data.len(), false, data)?;
        Ok(())
    }

    pub fn read_blocks(&mut self, env: &mut JNIEnv, lba: u64, len: usize) -> Result<Vec<u8>> {
        let block = self.block_size as usize;
        if len == 0 || !len.is_multiple_of(block) {
            bail!("unaligned or empty READ(10) request");
        }
        let blocks = len / block;
        if blocks > u32::MAX as usize {
            bail!("READ(16) request exceeds 2^32-1 blocks");
        }
        let mut cb = [0u8; 16];
        cb[0] = SCSI_READ16;
        cb[2..10].copy_from_slice(&lba.to_be_bytes());
        cb[10..14].copy_from_slice(&(blocks as u32).to_be_bytes());
        self.exec(env, cb, len, true, &[])
    }

    pub fn sync(&mut self, env: &mut JNIEnv) -> Result<()> {
        let mut cb = [0u8; 16];
        cb[0] = SCSI_SYNC_CACHE10;
        self.exec(env, cb, 0, false, &[])?;
        Ok(())
    }
}

fn cdb_length(opcode: u8) -> u8 {
    match opcode {
        SCSI_TEST_UNIT_READY | SCSI_INQUIRY => 6,
        SCSI_READ_CAPACITY10 | SCSI_SYNC_CACHE10 => 10,
        SCSI_READ_CAPACITY16 | SCSI_READ16 | SCSI_WRITE16 => 16,
        _ => 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_scsi_commands_to_cdb_lengths() {
        assert_eq!(cdb_length(SCSI_TEST_UNIT_READY), 6);
        assert_eq!(cdb_length(SCSI_INQUIRY), 6);
        assert_eq!(cdb_length(SCSI_READ_CAPACITY10), 10);
        assert_eq!(cdb_length(SCSI_SYNC_CACHE10), 10);
        assert_eq!(cdb_length(SCSI_READ_CAPACITY16), 16);
        assert_eq!(cdb_length(SCSI_READ16), 16);
        assert_eq!(cdb_length(SCSI_WRITE16), 16);
    }
}

fn endpoint_address(env: &mut JNIEnv, ep: &GlobalRef) -> Result<i32> {
    let res = env.call_method(ep.as_obj(), "getAddress", "()I", &[])?;
    Ok(res.i()?)
}
