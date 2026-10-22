// Standalone build of the plugin's Android library, for `gradle test` and CI.
// Inside an app, Tauri's generated project includes this module instead and this
// file is not used. `.tauri/tauri-api` (Tauri's Android API) is copied here by the
// plugin's build script: run `cargo check -p tauri-plugin-vk-login --target
// aarch64-linux-android` once to create it.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        id("com.android.library") version "8.9.3"
        id("org.jetbrains.kotlin.android") version "2.0.21"
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.PREFER_SETTINGS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "tauri-plugin-vk-login"

include(":tauri-android")
project(":tauri-android").projectDir = File("./.tauri/tauri-api")
