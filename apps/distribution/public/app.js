const fields = {
  download: document.querySelector("#download"),
  version: document.querySelector("#version"),
  published: document.querySelector("#published"),
  build: document.querySelector("#build-code"),
  sha: document.querySelector("#sha"),
  commit: document.querySelector("#commit"),
  error: document.querySelector("#error"),
};

fetch("/android/internal/latest.json", { cache: "no-store" })
  .then((response) => {
    if (!response.ok) throw new Error(`Manifest request failed: ${response.status}`);
    return response.json();
  })
  .then((manifest) => {
    fields.download.href = manifest.apk.url;
    fields.download.textContent = `Download ${formatBytes(manifest.apk.size)} APK`;
    fields.download.removeAttribute("aria-disabled");
    fields.version.textContent = manifest.version;
    fields.published.textContent = new Date(manifest.publishedAt).toLocaleDateString();
    fields.build.textContent = String(manifest.versionCode);
    fields.sha.textContent = manifest.apk.sha256;
    fields.commit.textContent = manifest.commit.slice(0, 12);
  })
  .catch(() => {
    fields.download.textContent = "Build unavailable";
    fields.error.hidden = false;
  });

function formatBytes(bytes) {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
