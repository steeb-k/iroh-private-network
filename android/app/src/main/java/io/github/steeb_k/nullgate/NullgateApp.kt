package io.github.steeb_k.nullgate

import android.app.Application

class NullgateApp : Application() {
    override fun onCreate() {
        super.onCreate()
        // Wire up the Android trust store + DNS context for iroh before any engine
        // component (service / activity) can start a connection.
        RustlsSetup.ensureInitialized(this)
    }
}
