package io.github.steeb_k.nullgate

import com.journeyapps.barcodescanner.CaptureActivity

/**
 * ZXing's default `CaptureActivity` is declared `sensorLandscape`, which forces
 * the scanner into landscape. This subclass is declared `portrait` in the
 * manifest so the QR scanner stays upright like the rest of the app. Wired via
 * `ScanOptions.setCaptureActivity(...)`.
 */
class PortraitCaptureActivity : CaptureActivity()
