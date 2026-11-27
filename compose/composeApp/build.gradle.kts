import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

val hostOs = System.getProperty("os.name").lowercase()
val nativeLibraryName = when {
    hostOs.contains("win") -> "private_music_engine.dll"
    hostOs.contains("mac") -> "libprivate_music_engine.dylib"
    else -> "libprivate_music_engine.so"
}
val jnaResourcePrefix = when {
    hostOs.contains("win") -> "win32-x86-64"
    hostOs.contains("mac") -> "darwin"
    else -> "linux-x86-64"
}
val rustRoot = rootDir.resolve("rust-ffi")
val rustOutput = System.getenv("CARGO_TARGET_DIR")?.let { file(it) } ?: rustRoot.resolve("target")
val generatedNativeResources = layout.buildDirectory.dir("nativeResources")

plugins {
    kotlin("multiplatform")
    kotlin("plugin.serialization")
    kotlin("plugin.compose")
    id("org.jetbrains.compose")
    id("com.android.kotlin.multiplatform.library")
}

kotlin {
    android {
        namespace = "com.dec.privatemusic.compose"
        compileSdk = 36
        minSdk = 26
        compilerOptions {
            jvmTarget = JvmTarget.JVM_17
        }
        androidResources {
            enable = true
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
        val desktopMain by getting
        desktopMain.dependencies {
            implementation(compose.desktop.currentOs)
            implementation("net.java.dev.jna:jna:5.17.0")
        }
        // JNA extracts the Rust library from these resources in packaged desktop apps.
        desktopMain.resources.srcDir(generatedNativeResources)
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

compose.desktop {
    application {
        mainClass = "com.dec.privatemusic.desktop.MainKt"
        nativeDistributions {
            targetFormats(TargetFormat.Dmg, TargetFormat.Msi, TargetFormat.Deb)
            packageName = "PrivateMusic"
            packageVersion = "1.0.1"
        }
    }
}

val cargoBuildDesktop by tasks.registering(Exec::class) {
    workingDir = rustRoot
    commandLine("cargo", "build", "--release")
}

val packageDesktopNative by tasks.registering(Copy::class) {
    dependsOn(cargoBuildDesktop)
    from(rustOutput.resolve("release/$nativeLibraryName"))
    into(generatedNativeResources.map { it.dir(jnaResourcePrefix) })
}

tasks.matching { it.name == "processDesktopMainResources" || it.name == "desktopProcessResources" }.configureEach {
    dependsOn(packageDesktopNative)
}

tasks.matching { it.name == "run" }.configureEach {
    val task = this as JavaExec
    task.dependsOn(packageDesktopNative)
    task.environment("JNA_LIBRARY_PATH", rustOutput.resolve("release").absolutePath)
}
