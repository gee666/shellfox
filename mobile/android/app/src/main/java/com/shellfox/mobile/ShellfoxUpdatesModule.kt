package com.shellfox.mobile

import android.app.Activity
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import com.facebook.react.bridge.BaseActivityEventListener
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.UiThreadUtil
import java.io.File
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.net.ssl.HttpsURLConnection

/** Downloads stay private until verified. Only Android's interactive installer may install them. */
class ShellfoxUpdatesModule(private val context: ReactApplicationContext) : ReactContextBaseJavaModule(context) {
    private val busy = AtomicBoolean(false)
    private val stopped = AtomicBoolean(false)
    private val executor = Executors.newSingleThreadExecutor()
    private var pending: Pending? = null
    private data class Pending(val file: File, val sha256: String, val version: String, val promise: Promise)
    private val listener = object : BaseActivityEventListener() {
        override fun onActivityResult(activity: Activity, requestCode: Int, resultCode: Int, data: Intent?) {
            if (requestCode != PERMISSION_REQUEST) return
            val update = pending ?: return
            pending = null
            if (Build.VERSION.SDK_INT >= 26 && !context.packageManager.canRequestPackageInstalls()) {
                fail(update, "INSTALL_PERMISSION", "Allow installation from Shellfox, then retry.")
            } else verifyAndLaunch(update)
        }
    }

    init { context.addActivityEventListener(listener) }
    override fun getName() = "ShellfoxUpdates"

    @ReactMethod
    fun getVersion(promise: Promise) {
        try { promise.resolve(installed().versionName ?: throw IllegalStateException("Missing app version.")) }
        catch (error: Exception) { promise.reject("VERSION", "Could not read the installed version.", error) }
    }

    @ReactMethod
    fun install(url: String, sha256: String, promise: Promise) {
        val match = ASSET_URL.matchEntire(url)
        if (match == null || !sha256.matches(Regex("[a-fA-F0-9]{64}"))) {
            promise.reject("VALIDATION", "Invalid Shellfox release asset.")
            return
        }
        if (stopped.get()) {
            promise.reject("CANCELLED", "Updater stopped.")
            return
        }
        if (!busy.compareAndSet(false, true)) {
            promise.reject("BUSY", "An update is already in progress.")
            return
        }
        executor.execute {
            var file: File? = null
            try {
                val directory = File(context.cacheDir, "updates")
                if (!directory.exists() && !directory.mkdirs()) error("Cannot create update directory.")
                // An earlier system installer may still be reading its APK.
                val expired = System.currentTimeMillis() - 24L * 60 * 60 * 1000
                directory.listFiles()?.filter { it.isFile && it.lastModified() < expired }?.forEach { it.delete() }
                file = File.createTempFile("shellfox-", ".apk", directory)
                download(url, file, sha256.lowercase())
                val update = Pending(file, sha256.lowercase(), match.groupValues[1], promise)
                verify(update)
                UiThreadUtil.runOnUiThread {
                    try {
                        val activity = context.currentActivity ?: error("Open Shellfox before installing an update.")
                        if (Build.VERSION.SDK_INT >= 26 && !context.packageManager.canRequestPackageInstalls()) {
                            pending = update
                            activity.startActivityForResult(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                                Uri.parse("package:${context.packageName}")), PERMISSION_REQUEST)
                        } else launch(update)
                    } catch (error: Exception) { fail(update, "INSTALL", error.message ?: "Cannot open installer.") }
                }
            } catch (error: Exception) {
                file?.delete()
                busy.set(false)
                promise.reject("UPDATE", error.message ?: "Update verification failed.", error)
            }
        }
    }

    private fun verifyAndLaunch(update: Pending) {
        executor.execute {
            try {
                verify(update)
                UiThreadUtil.runOnUiThread {
                    try { launch(update) }
                    catch (error: Exception) { fail(update, "INSTALL", error.message ?: "Cannot open installer.") }
                }
            } catch (error: Exception) { fail(update, "VERIFICATION", error.message ?: "Update verification failed.") }
        }
    }

    private fun launch(update: Pending) {
        check(!stopped.get()) { "Updater stopped." }
        val activity = context.currentActivity ?: error("Open Shellfox before installing an update.")
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", update.file)
        val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        activity.startActivity(intent)
        pending = null
        busy.set(false)
        // Keep the file for the installer. The next attempt removes old downloads.
        update.promise.resolve(null)
    }

    private fun fail(update: Pending, code: String, message: String) {
        pending = null
        update.file.delete()
        busy.set(false)
        update.promise.reject(code, message)
    }

    private fun download(source: String, file: File, expected: String) {
        var url = URL(source)
        val deadline = System.nanoTime() + 30L * 60 * 1_000_000_000
        for (redirect in 0..5) {
            require(url.protocol == "https" && url.port == -1 && url.userInfo == null && url.ref == null &&
                url.host in DOWNLOAD_HOSTS) { "Untrusted download origin." }
            val connection = url.openConnection() as HttpsURLConnection
            try {
                connection.instanceFollowRedirects = false
                connection.connectTimeout = 15_000
                connection.readTimeout = 30_000
                connection.setRequestProperty("Accept-Encoding", "identity")
                val status = connection.responseCode
                if (status in listOf(301, 302, 303, 307, 308)) {
                    require(redirect < 5) { "Too many download redirects." }
                    val location = connection.getHeaderField("Location") ?: error("Missing redirect location.")
                    url = URL(url, location)
                    continue
                }
                require(status == 200) { "Release download failed." }
                val length = connection.contentLengthLong
                require(length == -1L || length in 1..MAX_BYTES) { "Invalid download size." }
                val digest = MessageDigest.getInstance("SHA-256")
                var received = 0L
                connection.inputStream.use { input ->
                    file.outputStream().use { output ->
                        val buffer = ByteArray(64 * 1024)
                        while (true) {
                            check(!stopped.get() && !Thread.currentThread().isInterrupted) { "Updater stopped." }
                            require(System.nanoTime() < deadline) { "Update download timed out." }
                            val count = input.read(buffer)
                            if (count < 0) break
                            received += count
                            require(received <= MAX_BYTES) { "Update is too large." }
                            digest.update(buffer, 0, count)
                            output.write(buffer, 0, count)
                        }
                        output.fd.sync()
                    }
                }
                require(received > 0 && (length == -1L || received == length)) { "Incomplete update download." }
                require(hex(digest.digest()) == expected) { "Update checksum mismatch." }
                return
            } finally { connection.disconnect() }
        }
        error("Release download failed.")
    }

    @Suppress("DEPRECATION")
    private fun installed(): PackageInfo = context.packageManager.getPackageInfo(context.packageName, signatureFlags())

    @Suppress("DEPRECATION")
    private fun signatureFlags(): Int = if (Build.VERSION.SDK_INT >= 28)
        PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES

    @Suppress("DEPRECATION")
    private fun signers(info: PackageInfo): Set<String> {
        val signatures = if (Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners else info.signatures
        return signatures?.map { hex(MessageDigest.getInstance("SHA-256").digest(it.toByteArray())) }?.toSet() ?: emptySet()
    }

    @Suppress("DEPRECATION")
    private fun versionCode(info: PackageInfo): Long = if (Build.VERSION.SDK_INT >= 28) info.longVersionCode else info.versionCode.toLong()

    private fun verify(update: Pending) {
        val digest = MessageDigest.getInstance("SHA-256")
        update.file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) { val count = input.read(buffer); if (count < 0) break; digest.update(buffer, 0, count) }
        }
        require(hex(digest.digest()) == update.sha256) { "Update checksum mismatch." }
        val current = installed()
        val candidate = context.packageManager.getPackageArchiveInfo(update.file.absolutePath, signatureFlags())
            ?: error("Invalid APK.")
        require(candidate.packageName == context.packageName && candidate.packageName == "com.shellfox.mobile") { "Wrong APK package." }
        require(candidate.versionName == update.version && versionCode(candidate) > versionCode(current)) { "APK is not a newer release." }
        val trusted = signers(current)
        require(trusted.isNotEmpty() && signers(candidate) == trusted) { "APK signing certificate mismatch." }
        val currentDebug = (current.applicationInfo?.flags ?: 0) and ApplicationInfo.FLAG_DEBUGGABLE != 0
        val candidateDebug = (candidate.applicationInfo?.flags ?: 0) and ApplicationInfo.FLAG_DEBUGGABLE != 0
        require(currentDebug || !candidateDebug) { "Debug APK cannot replace a release build." }
    }

    override fun invalidate() {
        stopped.set(true)
        context.removeActivityEventListener(listener)
        pending?.let { fail(it, "CANCELLED", "Updater stopped. Retry from Shellfox.") }
        executor.shutdownNow()
        super.invalidate()
    }

    companion object {
        private const val PERMISSION_REQUEST = 52744
        private const val MAX_BYTES = 512L * 1024 * 1024
        private val DOWNLOAD_HOSTS = setOf("github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com")
        private val ASSET_URL = Regex("https://github\\.com/gee666/shellfox/releases/download/v((?:0|[1-9][0-9]*)\\.(?:0|[1-9][0-9]*)\\.(?:0|[1-9][0-9]*))/shellfox-mobile-\\1-android\\.apk")
        private fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it.toInt() and 255) }
    }
}
