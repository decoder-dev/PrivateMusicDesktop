import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.dec.privatemusic.vklogin"
    compileSdk = 37

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    testOptions {
        // The pure logic under test touches no Android class; anything that slips in
        // fails loudly instead of returning a default.
        unitTests.isReturnDefaultValues = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.19.1")
    implementation("androidx.exifinterface:exifinterface:1.4.2")
    implementation("androidx.appcompat:appcompat:1.8.0")
    implementation(project(":tauri-android"))

    testImplementation("junit:junit:4.13.2")
    // The platform's org.json is a stub in unit tests: use the real implementation.
    testImplementation("org.json:json:20260814")
}
