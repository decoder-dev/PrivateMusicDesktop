// Standalone build of this library, for unit tests outside an app:
//   ./scripts/android-test.sh   (see ../README.md)
// Inside an app, Tauri's generated settings include this module instead and this
// file is not used.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    // Versions live here, not in build.gradle.kts: inside an app Tauri's project already has
    // the Android and Kotlin plugins on its classpath and refuses a second, versioned request.
    plugins {
        id("com.android.library") version "9.3.3"
        id("org.jetbrains.kotlin.android") version "2.4.21"
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "tauri-plugin-media-session"

include(":tauri-android")
// Tauri's Android API (app.tauri.*). `scripts/android-test.sh` finds it in the cargo registry
// and passes it as -PtauriAndroidDir; Tauri's own generated build uses ".tauri/tauri-api".
project(":tauri-android").projectDir = file(
    (settings.providers.gradleProperty("tauriAndroidDir").orNull ?: System.getenv("TAURI_ANDROID_DIR"))
        ?: ".tauri/tauri-api"
)
