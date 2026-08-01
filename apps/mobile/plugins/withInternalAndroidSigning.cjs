const { withAppBuildGradle } = require("expo/config-plugins");

const releaseSigning = `        internalRelease {
            def keystorePath = System.getenv("K3720_ANDROID_KEYSTORE_FILE")
            def storeSecret = System.getenv("K3720_ANDROID_KEYSTORE_PASSWORD")
            def aliasName = System.getenv("K3720_ANDROID_KEY_ALIAS")
            def keySecret = System.getenv("K3720_ANDROID_KEY_PASSWORD")
            if (!keystorePath || !storeSecret || !aliasName || !keySecret) {
                throw new GradleException("Missing k-3720 internal Android signing environment.")
            }
            storeFile file(keystorePath)
            storePassword storeSecret
            keyAlias aliasName
            keyPassword keySecret
        }
`;

module.exports = function withInternalAndroidSigning(config) {
  return withAppBuildGradle(config, (result) => {
    if (result.modResults.language !== "groovy") {
      throw new Error("k-3720 internal signing requires a Groovy app build.gradle.");
    }
    let contents = result.modResults.contents;
    const signingMarker = "    signingConfigs {\n";
    if (!contents.includes(signingMarker)) throw new Error("Android signingConfigs block not found.");
    contents = contents.replace(signingMarker, `${signingMarker}${releaseSigning}`);
    const releaseBlock = /(release \{[\s\S]*?)signingConfig signingConfigs\.debug/;
    if (!releaseBlock.test(contents)) throw new Error("Android release signing entry not found.");
    result.modResults.contents = contents.replace(releaseBlock, "$1signingConfig signingConfigs.internalRelease");
    return result;
  });
};
