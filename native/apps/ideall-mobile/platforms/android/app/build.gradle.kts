plugins {
    id("com.android.application")
}

val ideallVersionName = System.getenv("IDEALL_VERSION") ?: "0.2.0"
val ideallVersionCode = (System.getenv("IDEALL_VERSION_CODE") ?: "2").toIntOrNull()
    ?.takeIf { it > 0 }
    ?: throw GradleException("IDEALL_VERSION_CODE must be a positive integer")
val ideallAndroidAbis = (System.getenv("IDEALL_ANDROID_ABIS") ?: "arm64-v8a")
    .split(',')
    .map { it.trim() }
    .filter { it.isNotEmpty() }
if (ideallAndroidAbis.isEmpty() || ideallAndroidAbis.any { it !in setOf("arm64-v8a", "x86_64") }) {
    throw GradleException("IDEALL_ANDROID_ABIS only supports arm64-v8a and x86_64")
}
val releaseSigningValues = listOf(
    System.getenv("IDEALL_ANDROID_KEYSTORE"),
    System.getenv("IDEALL_ANDROID_KEYSTORE_PASSWORD"),
    System.getenv("IDEALL_ANDROID_KEY_ALIAS"),
    System.getenv("IDEALL_ANDROID_KEY_PASSWORD"),
)
val releaseSigningConfigured = releaseSigningValues.all { !it.isNullOrBlank() }
if (!releaseSigningConfigured && releaseSigningValues.any { !it.isNullOrBlank() }) {
    throw GradleException(
        "Android release signing requires IDEALL_ANDROID_KEYSTORE, " +
            "IDEALL_ANDROID_KEYSTORE_PASSWORD, IDEALL_ANDROID_KEY_ALIAS and " +
            "IDEALL_ANDROID_KEY_PASSWORD",
    )
}

android {
    namespace = "com.jinziyou.ideall"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.jinziyou.ideall"
        minSdk = 26
        targetSdk = 35
        versionCode = ideallVersionCode
        versionName = ideallVersionName
        ndk {
            abiFilters += ideallAndroidAbis
        }
        manifestPlaceholders["nativeLibraryName"] = "ideall_mobile"
    }

    signingConfigs {
        if (releaseSigningConfigured) {
            create("release") {
                storeFile = file(releaseSigningValues[0]!!)
                storePassword = releaseSigningValues[1]!!
                keyAlias = releaseSigningValues[2]!!
                keyPassword = releaseSigningValues[3]!!
                enableV1Signing = true
                enableV2Signing = true
                enableV3Signing = true
                enableV4Signing = true
            }
        }
    }

    buildTypes {
        debug {
            isDebuggable = true
            isJniDebuggable = true
        }
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            if (releaseSigningConfigured) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    sourceSets.getByName("main").jniLibs.srcDirs("src/main/jniLibs")

    packaging.jniLibs.keepDebugSymbols += "*/arm64-v8a/libideall_mobile.so"

    lint {
        abortOnError = true
        checkReleaseBuilds = true
    }
}
