pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        // The Kotlin support class for rustls-platform-verifier (iroh's relay TLS
        // verifier) ships as an AAR inside the `rustls-platform-verifier-android`
        // crate's on-disk Maven repo. Locate it via cargo metadata so it tracks
        // the exact crate version cargo resolved — no Maven Central, no manual
        // version pinning. (Recommended setup from the crate's README.)
        maven {
            url = uri(rustlsPlatformVerifierMavenRepo())
            content { includeGroup("rustls") }
        }
    }
}

rootProject.name = "Nullgate"
include(":app")

/**
 * Run `cargo metadata` (filtered to Android) to find the cargo-cached
 * `rustls-platform-verifier-android` crate, and return the path to the on-disk
 * Maven repository bundled inside it.
 */
fun rustlsPlatformVerifierMavenRepo(): String {
    val manifest = file("../crates/ipn-mobile/Cargo.toml").absolutePath
    val proc = ProcessBuilder(
        "cargo", "metadata", "--format-version", "1",
        "--filter-platform", "aarch64-linux-android",
        "--manifest-path", manifest
    ).redirectErrorStream(false).start()
    val json = proc.inputStream.bufferedReader().readText()
    proc.waitFor()
    @Suppress("UNCHECKED_CAST")
    val meta = groovy.json.JsonSlurper().parseText(json) as Map<String, Any>
    @Suppress("UNCHECKED_CAST")
    val packages = meta["packages"] as List<Map<String, Any>>
    val pkg = packages.firstOrNull { it["name"] == "rustls-platform-verifier-android" }
        ?: error("rustls-platform-verifier-android not found in cargo metadata; is cargo on PATH?")
    val manifestPath = File(pkg["manifest_path"] as String)
    return File(manifestPath.parentFile, "maven").path
}
