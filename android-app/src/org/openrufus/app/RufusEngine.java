package org.openrufus.app;

import android.hardware.usb.UsbDeviceConnection;
import android.hardware.usb.UsbEndpoint;

public final class RufusEngine {
    static { System.loadLibrary("rufus_core"); }

    private RufusEngine() {}

    public static native long nativeOpen(UsbDeviceConnection connection,
                                         UsbEndpoint epIn,
                                         UsbEndpoint epOut,
                                         int interfaceIndex);
    public static native String nativeInfo(long handle);
    public static native int nativeWrite(long handle, long offset, byte[] data);
    public static native byte[] nativeRead(long handle, long offset, int length);
    public static native boolean nativeSync(long handle);
    public static native void nativeClose(long handle);
}
