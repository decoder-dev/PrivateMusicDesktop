import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    kotlin("multiplatform")
    kotlin("plugin.serialization")
    id("org.jetbrains.compose")
    id("com.android.application")
}

kotlin {
    androidTarget {
        compilerOptions {
            jvmTarget = JvmTarget.JVM_17
        }
    }
    jvm("desktop")

    sourceSets {
        commonMain.dependencies {
            implementation(compose.runtime)
            implementation(compose.foundation)
            implementation(compose.material3)
            implementation(compose.ui)
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
            implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
        }
        androidMain.dependencies {
            implementation("androidx.activity:activity-compose:1.10.1")
            implementation("androidx.core:core-ktx:1.16.0")
        }
        val desktopMain by getting
        desktopMain.dependencies {
            implementation(compose.desktop.currentOs)
            implementation("net.java.dev.jna:jna:5.17.0")
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
        }
        val desktopTest by getting
        desktopTest.dependencies {
            implementation(compose.desktop.currentOs)
        }
    }
}

android {
    namespace = "com.dec.privatemusic.compose"
    compileSdk = 36
    buildToolsVersion = "36.0.0"
    defaultConfig {
        applicationId = "com.dec.privatemusic.compose"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }
    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
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

compose.desktop {
    application {
        mainClass = "com.dec.privatemusic.desktop.MainKt"
        nativeDistributions {
            targetFormats(TargetFormat.Dmg, TargetFormat.Msi, TargetFormat.Deb)
            packageName = "PrivateMusicCompose"
            packageVersion = "0.1.0"
        }
    }
}

val rustAbi = System.getProperty("pm.rust.abi") ?: "arm64-v8a"
val rustTarget = when (rustAbi) {
    "arm64-v8a" -> "aarch64-linux-android"
    "x86_64" -> "x86_64-linux-android"
    else -> error("Unsupported Rust ABI: $rustAbi")
}
val rustRoot = rootDir.resolve("rust-ffi")

tasks.register<Exec>("cargoBuildPrivateMusic") {
    workingDir = rustRoot
    commandLine("cargo", "build", "--release", "--target", rustTarget)
}

tasks.register<Copy>("packagePrivateMusicJni") {
    dependsOn("cargoBuildPrivateMusic")
    from(rustRoot.resolve("target/$rustTarget/release/libprivate_music_engine.so"))
    into("src/main/jniLibs/$rustAbi")
}

tasks.named("preBuild") {
    dependsOn("packagePrivateMusicJni")
}

tasks.named<JavaExec>("run") {
    environment("JNA_LIBRARY_PATH", rustRoot.resolve("target/debug").absolutePath)
}
