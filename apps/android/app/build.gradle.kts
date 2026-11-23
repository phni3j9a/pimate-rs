plugins {
    id("com.android.application") version "8.12.0"
    id("org.jetbrains.kotlin.android") version "2.2.10"
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.10"
}
val repo = rootDir.resolve("../..").canonicalFile
val mobile = repo.resolve("target/mobile/android")
val buildRust by tasks.registering(Exec::class) {
    workingDir(repo)
    environment("CARGO_TARGET_DIR", repo.resolve("target"))
    environment("ANDROID_NDK_HOME", System.getenv("ANDROID_NDK_HOME") ?: android.sdkDirectory.resolve("ndk/27.1.12297006"))
    commandLine("bash", "scripts/build-android.sh")
    inputs.files(repo.resolve("Cargo.toml"), repo.resolve("Cargo.lock"), repo.resolve("crates"), repo.resolve("vendor"), repo.resolve("scripts/build-android.sh"), repo.resolve("scripts/generate-kotlin.sh"))
    outputs.dirs(mobile.resolve("jniLibs"), mobile.resolve("kotlin"))
}
android {
    namespace = "dev.pimate.app"
    compileSdk = 36
    defaultConfig { applicationId = "dev.pimate.app.rs"; minSdk = 26; targetSdk = 35; versionCode = 1; versionName = "0.1"; ndk { abiFilters += setOf("arm64-v8a", "armeabi-v7a", "x86_64") } }
    defaultConfig { testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner" }
    buildTypes { debug { applicationIdSuffix = ".debug" } }
    buildFeatures { compose = true }
    sourceSets["main"].java.srcDir(mobile.resolve("kotlin"))
    sourceSets["main"].jniLibs.srcDir(mobile.resolve("jniLibs"))
}
tasks.named("preBuild") { dependsOn(buildRust) }
dependencies {
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.9.2")
    implementation("androidx.compose.material3:material3:1.3.2")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4:1.7.8")
    androidTestImplementation("androidx.test:runner:1.6.2")
    testImplementation("junit:junit:4.13.2")
}
