import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    buildToolsVersion = "36.0.0"
    namespace = "com.dec.privatemusic2.android"
    defaultConfig {
        applicationId = "com.dec.privatemusic2.android"
        minSdk = 26
        targetSdk = 35
        // versionName is the app version (written to tauri.properties from the
        // Tauri config); versionCode = major*10000 + minor*100 + patch (0.2.7 -> 207).
        val name = tauriProperties.getProperty("tauri.android.versionName", "0.0.1")
        val (major, minor, patch) = name.split("-")[0].split(".").map { it.toIntOrNull() ?: 0 }.plus(listOf(0, 0, 0))
        // CI only: ANDROID_VERSION_CODE_OVERRIDE builds a "newer" APK of the same version to test
        // installing an update over the previous one (scripts/android-upgrade-check.sh).
        val codeOverride = System.getenv("ANDROID_VERSION_CODE_OVERRIDE")?.toIntOrNull()
        versionCode = codeOverride ?: (major * 10000 + minor * 100 + patch)
        versionName = name
    }

    // Release signing from the environment. Without all four variables the
    // release APK is left UNSIGNED (zipaligned) to be signed afterwards with
    // apksigner; there is no fallback key and no keystore in this repository.
    val envStore = System.getenv("ANDROID_KEYSTORE_PATH").orEmpty()
    val envStorePass = System.getenv("ANDROID_KEYSTORE_PASSWORD").orEmpty()
    val envAlias = System.getenv("ANDROID_KEY_ALIAS").orEmpty()
    val envKeyPass = System.getenv("ANDROID_KEY_PASSWORD").orEmpty()
    val canSign = listOf(envStore, envStorePass, envAlias, envKeyPass).all { it.isNotEmpty() }
    if (canSign) {
        signingConfigs {
            create("release") {
                storeFile = file(envStore)
                storePassword = envStorePass
                keyAlias = envAlias
                keyPassword = envKeyPass
            }
        }
    }
    buildTypes {
        getByName("debug") {
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (canSign) {
                signingConfig = signingConfigs.getByName("release")
            } else {
                logger.warn(
                    "WARNING: ANDROID_KEYSTORE_PATH / ANDROID_KEYSTORE_PASSWORD / ANDROID_KEY_ALIAS / " +
                        "ANDROID_KEY_PASSWORD are not all set: the release APK will be UNSIGNED. " +
                        "Sign it with apksigner before installing (see docs/android.md)."
                )
            }
            // R8 stays off: the release build is never run by the emulator tests, and the plugins are found by
            // name and by reflection (see the keep rules). A few MB of APK size is cheaper than a crash on a phone.
            optimization {
               enable = false
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
