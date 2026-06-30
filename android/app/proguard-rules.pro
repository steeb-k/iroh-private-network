# JNA (used by the UniFFI-generated bindings) relies on reflection / native
# method names; keep it intact even if minification is enabled.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { *; }

# UniFFI-generated bindings and callback interfaces are referenced via JNI.
-keep class uniffi.seed_mobile.** { *; }
