import java.io.File
import java.util.Properties

plugins {
    id("com.android.application") version "8.12.0"
    id("org.jetbrains.kotlin.android") version "2.2.10"
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.10"
}

val repo = rootDir.resolve("../..").canonicalFile
val mobile = repo.resolve("target/mobile/android")
val buildRust by
    tasks.registering(Exec::class) {
        workingDir(repo)
        environment("CARGO_TARGET_DIR", repo.resolve("target"))
        environment(
            "ANDROID_NDK_HOME",
            System.getenv("ANDROID_NDK_HOME") ?: android.sdkDirectory.resolve("ndk/27.1.12297006"),
        )
        commandLine("bash", "scripts/build-android.sh")
        inputs.files(
            repo.resolve("Cargo.toml"),
            repo.resolve("Cargo.lock"),
            repo.resolve("crates"),
            repo.resolve("vendor"),
            repo.resolve("scripts/build-android.sh"),
            repo.resolve("scripts/generate-kotlin.sh"),
        )
        outputs.dirs(mobile.resolve("jniLibs"), mobile.resolve("kotlin"))
    }

kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

val appVersion = rootDir.resolve("version.txt").readText().trim()
val versionParts = appVersion.split('.').map {
    require(it.matches(Regex("0|[1-9][0-9]{0,2}"))) { "Expected a stable Android release version" }
    it.toInt()
}
require(versionParts.size == 3)

android {
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    namespace = "dev.pimate.app"
    compileSdk = 36
    defaultConfig {
        applicationId = "dev.pimate.app"
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        targetSdk = 36
        versionCode = versionParts[0] * 1_000_000 + versionParts[1] * 1_000 + versionParts[2]
        versionName = appVersion
        ndk { abiFilters += setOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }

    val releaseSigning = System.getenv("PIMATE_SIGNING_PROPERTIES")?.let { path ->
        val file = File(path).canonicalFile
        val key = file.resolveSibling("pimate-release.jks").canonicalFile
        require(!file.startsWith(repo) && !key.startsWith(repo) && key.isFile)
        val props = Properties().apply { file.reader(Charsets.UTF_8).use { load(it) } }
        require(File(props.getProperty("storeFile")).canonicalFile == key)
        signingConfigs.create("pimateRelease") {
            storeFile = key
            storePassword = props.getProperty("storePassword")
            keyAlias = props.getProperty("keyAlias")
            keyPassword = props.getProperty("keyPassword")
        }
    }
    buildTypes {
        debug { applicationIdSuffix = ".rs.debug" }
        release { signingConfig = releaseSigning }
    }
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
    testImplementation("org.json:json:20240303") // Exercise the Android rows JSON in local JVM tests.
    testImplementation("junit:junit:4.13.2")
}
