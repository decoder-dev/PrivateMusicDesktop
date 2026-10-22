plugins {
    id("com.android.library") version "8.9.3"
    id("org.jetbrains.kotlin.android") version "2.1.21"
}

android {
    namespace = "com.dec.privatemusic.mediasession"
    compileSdk = 35

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        // Pure logic only: touching an Android API in a unit test is a bug.
        unitTests.isReturnDefaultValues = false
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    // MediaSessionCompat, PlaybackStateCompat, MediaStyle notification.
    implementation("androidx.media:media:1.7.0")
    implementation(project(":tauri-android"))

    testImplementation("junit:junit:4.13.2")
    // android.jar's org.json is a stub that throws in unit tests.
    testImplementation("org.json:json:20240303")
}
