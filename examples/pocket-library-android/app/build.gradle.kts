plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// forge.toml is the source of truth for the application ID and minSdk, whether the build
// starts from `forge build android` or from Android Studio.
val forgeToml: List<String> = rootProject.file("forge.toml").readLines()

fun forgeTomlValue(section: String, key: String): String? {
    var current = ""
    for (raw in forgeToml) {
        val line = raw.trim()
        if (line.startsWith("[")) {
            current = line.removePrefix("[").substringBefore("]").trim()
            continue
        }
        if (current != section || !line.contains("=")) continue
        val (k, v) = line.split("=", limit = 2).map { it.trim() }
        if (k == key) return v.substringBefore(" #").trim().removeSurrounding("\"")
    }
    return null
}

val forgeApplicationId: String =
    forgeTomlValue("android", "application_id")
        ?: forgeTomlValue("app", "bundle_id")
        ?: error("forge.toml: missing [app] bundle_id")
val forgeMinSdk: Int =
    forgeTomlValue("android", "min_sdk")?.toInt()
        ?: error("forge.toml: missing [android] min_sdk")

android {
    // The Kotlin package of the app code (fixed when the app was generated).
    namespace = "org.forgemobile.pocketlibrary"
    compileSdk = 35

    defaultConfig {
        applicationId = forgeApplicationId
        minSdk = forgeMinSdk
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
        unitTests.all {
            // JVM unit tests call the real Rust core through the generated bindings, using the
            // host build of this app's crate (`cargo build --manifest-path rust/Cargo.toml`,
            // which `forge test` runs first).
            val hostLibs = rootProject.file("rust/target/debug").absolutePath
            it.systemProperty("jna.library.path", hostLibs)
            // ForgeAndroid.init loads the same library through System.loadLibrary (JNI).
            it.systemProperty("java.library.path", hostLibs)
        }
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDir("${rootProject.projectDir}/Generated")
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    debugImplementation("androidx.compose.ui:ui-tooling")

    implementation("net.java.dev.jna:jna:5.19.1@aar")
    // Android's trust store for the Rust HTTPS client (see ForgeAndroid.init), for when the
    // catalogue comes to Android. Must equal the rustls-platform-verifier-android version in
    // Cargo.lock (scripts/check-template-sync.sh).
    implementation("org.rustls:rustls-platform-verifier:0.2.0")
    // The generated Kotlin bindings expose the Rust async exports as suspend functions.
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.8.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.8.1")
    // Desktop JNA (with its host native dispatcher) for JVM unit tests.
    testImplementation("net.java.dev.jna:jna:5.19.1")

    // Compose UI tests on an emulator (or a phone), finding controls by test tag.
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
