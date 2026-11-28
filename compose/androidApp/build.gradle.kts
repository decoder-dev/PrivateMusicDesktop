plugins {
    id("com.android.application")
    id("org.jetbrains.compose")
    kotlin("plugin.compose")
}

val releaseKeystore = System.getenv("ANDROID_KEYSTORE_PATH").orEmpty()
val hasReleaseSigning = releaseKeystore.isNotBlank()
val rustAbi = System.getProperty("pm.rust.abi") ?: "arm64-v8a"
val rustTarget = when (rustAbi) {
    "arm64-v8a" -> "aarch64-linux-android"
    "x86_64" -> "x86_64-linux-android"
    else -> error("Unsupported Rust ABI: $rustAbi")
}

android {
    namespace = "com.dec.privatemusic.compose.app"
    compileSdk = 36
    buildToolsVersion = "36.0.0"
    defaultConfig {
        applicationId = "com.dec.privatemusic2.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 10002
        versionName = "1.0.2"
        ndk { abiFilters.add(rustAbi) }
    }
    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            if (hasReleaseSigning) signingConfig = signingConfigs.getByName("release")
        }
        getByName("debug") {
            isDebuggable = true
            isJniDebuggable = true
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
    }
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
}

dependencies {
    implementation(project(":composeApp"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.media:media:1.7.1")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
}

val rustRoot = rootDir.resolve("rust-ffi")
val rustOutput = System.getenv("CARGO_TARGET_DIR")?.let { file(it) } ?: rustRoot.resolve("target")
val nativeLibrary = rustOutput.resolve("$rustTarget/release/libprivate_music_engine.so")

tasks.register<Exec>("cargoBuildPrivateMusic") {
    workingDir = rustRoot
    commandLine("cargo", "build", "--release", "--target", rustTarget)
}

tasks.register<Copy>("packagePrivateMusicJni") {
    dependsOn("cargoBuildPrivateMusic")
    from(nativeLibrary)
    into("src/main/jniLibs/$rustAbi")
}

tasks.named("preBuild") {
    dependsOn("packagePrivateMusicJni")
}
