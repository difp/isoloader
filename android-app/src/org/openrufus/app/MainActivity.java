package org.openrufus.app;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.PendingIntent;
import android.content.*;
import android.hardware.usb.*;
import android.net.Uri;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.os.Build;
import android.view.View;
import android.widget.*;

import java.io.InputStream;
import java.security.MessageDigest;
import java.util.HashMap;

public final class MainActivity extends Activity {
    private static final int PICK_IMAGE = 42;
    private static final String USB_PERMISSION = "org.openrufus.app.USB_PERMISSION";
    private UsbManager usb;
    private UsbDevice device;
    private UsbInterface massStorage;
    private UsbEndpoint epIn, epOut;
    private Uri image;
    private long imageSize;
    private TextView status;
    private ProgressBar progress;
    private Button burn;

    private final BroadcastReceiver receiver = new BroadcastReceiver() {
        @Override public void onReceive(Context context, Intent intent) {
            if (USB_PERMISSION.equals(intent.getAction())) {
                UsbDevice d;
                if (Build.VERSION.SDK_INT >= 33) {
                    d = intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice.class);
                } else {
                    @SuppressWarnings("deprecation") UsbDevice legacy = intent.getParcelableExtra(UsbManager.EXTRA_DEVICE);
                    d = legacy;
                }
                if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false) && d != null) {
                    selectMassStorage(d);
                } else if (status != null) status.setText("USB permission denied");
            } else if (UsbManager.ACTION_USB_DEVICE_ATTACHED.equals(intent.getAction())) {
                findUsb();
            } else if (UsbManager.ACTION_USB_DEVICE_DETACHED.equals(intent.getAction())) {
                device = null; massStorage = null; epIn = null; epOut = null;
                if (status != null) status.setText("USB device detached"); updateButton();
            }
        }
    };

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        usb = (UsbManager) getSystemService(USB_SERVICE);
        IntentFilter f = new IntentFilter(USB_PERMISSION);
        f.addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED);
        f.addAction(UsbManager.ACTION_USB_DEVICE_DETACHED);
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(receiver, f, Context.RECEIVER_NOT_EXPORTED);
        else registerReceiver(receiver, f);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL); root.setPadding(32, 40, 32, 32);
        TextView title = new TextView(this); title.setText("ISO Loader — Rust USB Flasher"); title.setTextSize(22); root.addView(title);
        Button pick = new Button(this); pick.setText("1. Select ISO / IMG"); pick.setOnClickListener(v -> pickImage()); root.addView(pick);
        TextView selected = new TextView(this); selected.setText("No image selected"); root.addView(selected);
        Button scan = new Button(this); scan.setText("2. Detect USB drive"); scan.setOnClickListener(v -> findUsb()); root.addView(scan);
        TextView target = new TextView(this); target.setText("No USB drive selected"); root.addView(target);
        CheckBox verify = new CheckBox(this); verify.setText("Verify after writing"); verify.setChecked(true); root.addView(verify);
        burn = new Button(this); burn.setText("3. FLASH USB"); burn.setEnabled(false); root.addView(burn);
        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal); progress.setMax(100); progress.setVisibility(View.GONE); root.addView(progress);
        status = new TextView(this); status.setText("Ready"); root.addView(status);
        setContentView(root);

        pick.setOnClickListener(v -> { pickImage(); });
        scan.setOnClickListener(v -> { findUsb(); });
        burn.setOnClickListener(v -> confirmBurn(verify.isChecked()));
        this.selected = selected; this.target = target;
        findUsb();
    }

    private TextView selected, target;

    private void pickImage() {
        Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*");
        startActivityForResult(i, PICK_IMAGE);
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == PICK_IMAGE && result == RESULT_OK && data != null) {
            image = data.getData();
            try (ParcelFileDescriptor p = getContentResolver().openFileDescriptor(image, "r")) { imageSize = p == null ? 0 : p.getStatSize(); }
            catch (Exception e) { imageSize = 0; }
            selected.setText("Image: " + image.getLastPathSegment() + " (" + imageSize / 1048576 + " MiB)"); updateButton();
        }
    }

    private void findUsb() {
        for (UsbDevice d : usb.getDeviceList().values()) {
            for (int i = 0; i < d.getInterfaceCount(); i++) {
                UsbInterface iface = d.getInterface(i);
                if (iface.getInterfaceClass() == UsbConstants.USB_CLASS_MASS_STORAGE
                        && iface.getInterfaceSubclass() == 0x06
                        && iface.getInterfaceProtocol() == 0x50) {
                    device = d;
                    if (!usb.hasPermission(d)) {
                        PendingIntent p = PendingIntent.getBroadcast(this, 0, new Intent(USB_PERMISSION), PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE);
                        usb.requestPermission(d, p);
                    } else selectMassStorage(d);
                    return;
                }
            }
        }
        target.setText("No USB Mass Storage device"); updateButton();
    }

    private void selectMassStorage(UsbDevice d) {
        for (int i = 0; i < d.getInterfaceCount(); i++) {
            UsbInterface iface = d.getInterface(i); UsbEndpoint in = null, out = null;
            for (int j = 0; j < iface.getEndpointCount(); j++) {
                UsbEndpoint e = iface.getEndpoint(j);
                if (e.getType() == UsbConstants.USB_ENDPOINT_XFER_BULK) {
                    if (e.getDirection() == UsbConstants.USB_DIR_IN) in = e; else out = e;
                }
            }
            if (in != null && out != null) { massStorage = iface; epIn = in; epOut = out; break; }
        }
        target.setText("USB: " + (d.getProductName() == null ? d.getDeviceName() : d.getProductName())); updateButton();
    }

    private void updateButton() { if (burn != null) burn.setEnabled(image != null && device != null && massStorage != null && epIn != null && epOut != null); }

    private void confirmBurn(boolean verify) {
        String name = device == null || device.getProductName() == null ? "USB drive" : device.getProductName();
        new AlertDialog.Builder(this).setTitle("Erase USB drive?").setMessage("All data on " + name + " will be overwritten.")
            .setNegativeButton("Cancel", null).setPositiveButton("Flash", (d, w) -> flash(verify)).show();
    }

    private void flash(boolean verify) {
        final UsbDevice targetDevice = device;
        final UsbInterface targetInterface = massStorage;
        final UsbEndpoint targetIn = epIn;
        final UsbEndpoint targetOut = epOut;
        final Uri sourceImage = image;
        final long sourceSize = imageSize;
        burn.setEnabled(false); progress.setVisibility(View.VISIBLE); progress.setProgress(0);
        new Thread(() -> {
            UsbDeviceConnection connection = null; long h = 0; String error = null;
            try {
                connection = usb.openDevice(targetDevice); if (connection == null || !connection.claimInterface(targetInterface, true)) throw new Exception("USB open/claim failed");
                h = RufusEngine.nativeOpen(connection, targetIn, targetOut, targetInterface.getId()); if (h == 0) throw new Exception("USB Mass Storage initialization failed");
                String[] info = RufusEngine.nativeInfo(h).split("\\|", -1);
                if (info.length != 4) throw new Exception("Could not read USB capacity");
                long capacity = Long.parseLong(info[2]);
                int blockSize = Integer.parseInt(info[3]);
                if (blockSize <= 0 || blockSize > 65536 || (blockSize & (blockSize - 1)) != 0) {
                    throw new Exception("Unsupported USB logical block size: " + blockSize);
                }
                if (sourceSize <= 0) throw new Exception("Image size is unknown; copy it to local storage first");
                long required = Math.multiplyExact(Math.addExact(sourceSize, blockSize - 1) / blockSize, blockSize);
                if (required > capacity) throw new Exception("Image is larger than the USB drive");
                int chunkSize = (64 * 1024 / blockSize) * blockSize;
                if (chunkSize == 0) chunkSize = blockSize;
                byte[] buf = new byte[chunkSize]; long off = 0; MessageDigest expected = verify ? MessageDigest.getInstance("SHA-256") : null, actual = verify ? MessageDigest.getInstance("SHA-256") : null;
                try (InputStream in = getContentResolver().openInputStream(sourceImage)) {
                    if (in == null) throw new Exception("Cannot open image");
                    long remaining = sourceSize;
                    while (remaining > 0) {
                        int wanted = (int)Math.min(buf.length, remaining);
                        int n = 0;
                        while (n < wanted) {
                            int got = in.read(buf, n, wanted - n);
                            if (got < 0) throw new Exception("Image ended before its reported size");
                            if (got > 0) n += got;
                        }
                        int len = (n + blockSize - 1) / blockSize * blockSize; byte[] block = new byte[len]; System.arraycopy(buf, 0, block, 0, n);
                        if (RufusEngine.nativeWrite(h, off, block) != len) throw new Exception("USB write failed at " + off);
                        if (expected != null) expected.update(block); off += len; remaining -= n; final int pct = (int)Math.min(100, off * 100 / Math.max(1, required)); runOnUiThread(() -> progress.setProgress(pct));
                    }
                }
                if (!RufusEngine.nativeSync(h)) throw new Exception("USB cache flush failed");
                if (verify) { for (long p = 0; p < off; ) { int n = (int)Math.min(chunkSize, off-p); byte[] b = RufusEngine.nativeRead(h, p, n); if (b == null || b.length != n) throw new Exception("USB verify read failed"); actual.update(b); p += n; } if (!MessageDigest.isEqual(expected.digest(), actual.digest())) throw new Exception("Verification failed"); }
            } catch (Exception e) { error = e.getMessage(); }
            finally { if (h != 0) RufusEngine.nativeClose(h); if (connection != null) { try { connection.releaseInterface(targetInterface); } catch (Exception ignored) {} connection.close(); } }
            final String result = error; runOnUiThread(() -> { progress.setVisibility(View.GONE); updateButton(); status.setText(result == null ? "Flash complete" : "Error: " + result); });
        }).start();
    }

    @Override protected void onDestroy() { try { unregisterReceiver(receiver); } catch (Exception ignored) {} super.onDestroy(); }
}
