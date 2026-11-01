package org.orqwa.android;

import android.Manifest;
import android.app.Activity;
import android.app.PendingIntent;
import android.app.PictureInPictureParams;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageInstaller;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.UserManager;
import android.provider.Settings;
import android.util.Rational;
import android.view.View;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Arrays;
import java.util.HashSet;
import java.util.Set;

import org.godotengine.godot.Godot;
import org.godotengine.godot.plugin.GodotPlugin;
import org.godotengine.godot.plugin.SignalInfo;
import org.godotengine.godot.plugin.UsedByGodot;

/**
 * Readies ORQWA on Android (ADR 0024). Godot creates its plugins before it opens its
 * extensions, on the app's main thread: loading ORQWA's library here runs its JNI_OnLoad with
 * the app's class loader, the one moment btleplug can find its Java classes, and names the
 * folder for ORQWA's data, which sync apps reach (R148). Then it hands the
 * app's context to HTTPS's certificate checks and asks for Bluetooth, which Android grants only
 * at run time. It also installs ORQWA's updates (R99): the APK the core downloaded, handed to
 * Android's installer.
 */
public class OrqwaAndroid extends GodotPlugin {
    /** Android tells how an update's installation went with this broadcast, to ORQWA only. */
    private static final String UPDATE_STATUS = "org.orqwa.android.UPDATE_STATUS";

    /** The request for "Photos and videos", told apart from Bluetooth's in the answer. */
    private static final int VIDEOS = 2;

    /** An update waiting for the rider to allow ORQWA to install apps. */
    private String pendingUpdate;

    public OrqwaAndroid(Godot godot) {
        super(godot);
        System.loadLibrary("orqwa_gd");
        initStorage(sharedFolder(godot.getContext()));
        initVoice(godot.getContext().getApplicationContext());
    }

    /** Readies HTTPS's certificate checks through Android (rustls-platform-verifier). */
    private static native void initTls(Context context);

    /** Names ORQWA's folder in shared storage to the core, before Godot opens it (R148). */
    private static native void initStorage(String folder);

    /** Readies CPAL's embedded AAudio host without opening a microphone or speaker stream. */
    private static native void initVoice(Context context);

    /**
     * ORQWA's own folder in shared storage, `Android/media/org.orqwa.bike`, which Android
     * creates here; null where shared storage is not ready. It is the one folder of ORQWA's that
     * sync apps allowed all files reach (ORQWA needs no permission for it); the newer MediaStore
     * holds media only, not rides and courses.
     */
    @SuppressWarnings("deprecation")
    private static String sharedFolder(Context context) {
        File[] folders = context.getExternalMediaDirs();
        return folders.length > 0 && folders[0] != null ? folders[0].getAbsolutePath() : null;
    }

    @Override
    public String getPluginName() {
        return "OrqwaAndroid";
    }

    @Override
    public Set<SignalInfo> getPluginSignals() {
        return new HashSet<>(Arrays.asList(
                new SignalInfo("update_failed", String.class),
                new SignalInfo("videos_allowed", Boolean.class)));
    }

    @Override
    public View onMainCreate(Activity activity) {
        initTls(activity.getApplicationContext());
        IntentFilter status = new IntentFilter(UPDATE_STATUS);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            activity.getApplicationContext()
                    .registerReceiver(updateStatus, status, Context.RECEIVER_NOT_EXPORTED);
        } else {
            activity.getApplicationContext().registerReceiver(updateStatus, status);
        }
        String[] bluetooth = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
                ? new String[] {Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT}
                : new String[] {Manifest.permission.ACCESS_FINE_LOCATION};
        activity.requestPermissions(bluetooth, 1);
        return null;
    }

    /**
     * Whether ORQWA may read course videos a sync app put in its folder (R149). From Android 16
     * an app targeting it needs "Photos and videos" for videos in its own `Android/media` folder
     * that it did not write itself.
     */
    @UsedByGodot
    public boolean canReadVideos() {
        Activity activity = getActivity();
        return Build.VERSION.SDK_INT < Build.VERSION_CODES.BAKLAVA
                || (activity != null
                        && activity.checkSelfPermission(Manifest.permission.READ_MEDIA_VIDEO)
                                == PackageManager.PERMISSION_GRANTED);
    }

    /** Asks Android for "Photos and videos"; `videos_allowed` tells the answer. */
    @UsedByGodot
    public void askToReadVideos() {
        Activity activity = getActivity();
        if (activity == null) {
            emitSignal("videos_allowed", false);
            return;
        }
        activity.requestPermissions(new String[] {Manifest.permission.READ_MEDIA_VIDEO}, VIDEOS);
    }

    @Override
    public void onMainRequestPermissionsResult(int requestCode, String[] permissions, int[] grantResults) {
        if (requestCode == VIDEOS) {
            emitSignal("videos_allowed", canReadVideos());
        }
    }

    /**
     * Shows ORQWA in a small window on top of other apps, shaped like its content of `width` by
     * `height` (the ride's HUD, as the overlay on a computer, R55). Android allows shapes between
     * 1:2.39 and 2.39:1 only.
     */
    @UsedByGodot
    public void enterPictureInPicture(int width, int height) {
        Activity activity = getActivity();
        if (activity == null || Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return;
        }
        float limit = 2.39f;
        float shape = Math.max(1f / limit, Math.min(limit, (float) width / Math.max(height, 1)));
        Rational ratio = new Rational(Math.round(shape * 1000), 1000);
        PictureInPictureParams params =
                new PictureInPictureParams.Builder().setAspectRatio(ratio).build();
        activity.runOnUiThread(() -> activity.enterPictureInPictureMode(params));
    }

    /** Whether ORQWA shows in the small window now; the rider may have opened it again. */
    @UsedByGodot
    public boolean isInPictureInPicture() {
        Activity activity = getActivity();
        return activity != null
                && Build.VERSION.SDK_INT >= Build.VERSION_CODES.N
                && activity.isInPictureInPictureMode();
    }

    /**
     * Whether Android lets ORQWA install its updates. Advanced Protection (Android 16) or a
     * device's administrator may forbid every app to install apps; asked before the download,
     * so the rider is not sent to a setting they cannot change.
     */
    @UsedByGodot
    public boolean canInstallUpdates() {
        Activity activity = getActivity();
        if (activity == null) {
            return false;
        }
        UserManager users = (UserManager) activity.getSystemService(Context.USER_SERVICE);
        return !users.hasUserRestriction(UserManager.DISALLOW_INSTALL_UNKNOWN_SOURCES)
                && (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q
                        || !users.hasUserRestriction(UserManager.DISALLOW_INSTALL_UNKNOWN_SOURCES_GLOBALLY));
    }

    /**
     * Installs the update at `path` over ORQWA, which Android then closes. From Android 12 an app
     * updating itself may do so without asking; where Android asks anyway, it shows its prompt.
     * The first time, the rider allows ORQWA to install apps; the update goes on when they come
     * back from that setting.
     */
    @UsedByGodot
    public void installUpdate(String path) {
        Activity activity = getActivity();
        if (activity == null) {
            emitSignal("update_failed", "ORQWA is not open");
            return;
        }
        if (!activity.getPackageManager().canRequestPackageInstalls()) {
            pendingUpdate = path;
            Uri orqwa = Uri.parse("package:" + activity.getPackageName());
            activity.startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, orqwa));
            return;
        }
        // A whole APK is copied: not on the main thread.
        new Thread(() -> install(activity, path)).start();
    }

    @Override
    public void onMainResume() {
        String path = pendingUpdate;
        Activity activity = getActivity();
        if (path == null || activity == null) {
            return;
        }
        pendingUpdate = null;
        if (activity.getPackageManager().canRequestPackageInstalls()) {
            new Thread(() -> install(activity, path)).start();
        } else {
            emitSignal("update_failed", "ORQWA may not install apps");
        }
    }

    private void install(Activity activity, String path) {
        PackageInstaller installer = activity.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params =
                new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(activity.getPackageName());
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
        }
        File apk = new File(path);
        try {
            int id = installer.createSession(params);
            try (PackageInstaller.Session session = installer.openSession(id)) {
                try (InputStream in = new FileInputStream(apk);
                        OutputStream out = session.openWrite("orqwa.apk", 0, apk.length())) {
                    byte[] buffer = new byte[1 << 16];
                    int read;
                    while ((read = in.read(buffer)) > 0) {
                        out.write(buffer, 0, read);
                    }
                    session.fsync(out);
                }
                Intent status = new Intent(UPDATE_STATUS).setPackage(activity.getPackageName());
                // Mutable: Android fills in the result.
                int flags = PendingIntent.FLAG_UPDATE_CURRENT
                        | (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ? PendingIntent.FLAG_MUTABLE : 0);
                session.commit(PendingIntent.getBroadcast(activity, id, status, flags).getIntentSender());
            }
        } catch (IOException | RuntimeException e) {
            emitSignal("update_failed", e.toString());
        }
    }

    private final BroadcastReceiver updateStatus = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
            if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                Intent confirm = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                        ? intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent.class)
                        : legacyIntent(intent);
                if (confirm != null) {
                    context.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                }
            } else if (status != PackageInstaller.STATUS_SUCCESS) {
                String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
                emitSignal("update_failed", message == null ? "installation failed" : message);
            }
        }
    };

    @SuppressWarnings("deprecation")
    private static Intent legacyIntent(Intent intent) {
        return intent.getParcelableExtra(Intent.EXTRA_INTENT);
    }
}
