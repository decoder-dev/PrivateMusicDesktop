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
