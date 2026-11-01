plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Keep in sync with forge.toml [android] and [app].bundle_id
val forgeMinSdk = 26
val forgeApplicationId = "FORGE_BUNDLE_ID"

android {
    namespace = forgeApplicationId
    compileSdk = 35

    defaultConfig {
        applicationId = forgeApplicationId
        minSdk = forgeMinSdk
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"
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
            // host build of forge-ffi (`cargo build -p forge-ffi` from the workspace root).
            it.systemProperty(
                "jna.library.path",
                rootProject.file("../../target/debug").absolutePath,
            )
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
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    debugImplementation("androidx.compose.ui:ui-tooling")

    implementation("net.java.dev.jna:jna:5.15.0@aar")

    testImplementation("junit:junit:4.13.2")
    // Desktop JNA (with its host native dispatcher) for JVM unit tests.
    testImplementation("net.java.dev.jna:jna:5.15.0")
}
