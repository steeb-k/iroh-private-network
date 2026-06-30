package io.github.steeb_k.nullgate

import android.content.Context
import android.util.Log

/**
 * Initializes the two Android-aware pieces of the iroh stack with the app's JVM
 * Context, via a native method implemented in `libipn_mobile.so`:
 *
 *  * `rustls-platform-verifier` — iroh's relay TLS validates certificates against
 *    Android's trust store through it.
 *  * `ndk-context` — iroh's DNS discovery (hickory-resolver) reads Android's DNS
 *    config through it.
 *
 * Must run once, before the engine opens any TLS/DNS connection — otherwise the
 * first handshake fails with "android context was not initialized".
 *
 * The Kotlin `CertificateVerifier` support class the native side calls back into
 * is provided by the `rustls:rustls-platform-verifier` AAR (sourced from the
 * rustls-platform-verifier-android crate's local Maven repo; see settings.gradle.kts).
 */
object RustlsSetup {
    @Volatile
    private var initialized = false

    private external fun initRustlsPlatformVerifier(context: Context)

    @Synchronized
    fun ensureInitialized(context: Context) {
        if (initialized) return
        // libipn_mobile is also loaded lazily by the UniFFI bindings; loading it
        // here first guarantees the native symbol is resolvable before we call it.
        System.loadLibrary("ipn_mobile")
        initRustlsPlatformVerifier(context.applicationContext)
        initialized = true
        Log.i("RustlsSetup", "android context registered (ndk-context + rustls verifier)")
    }
}
