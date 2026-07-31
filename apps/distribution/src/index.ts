const LATEST_PATH = "/android/internal/latest.json";
const APK_PREFIX = "/android/builds/";

export default {
  async fetch(request, env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/health") return Response.json({ status: "ok" });

    if (url.pathname === LATEST_PATH) {
      return serveBuild(env.BUILDS, "android/internal/latest.json", "no-store");
    }
    if (url.pathname.startsWith(APK_PREFIX) && url.pathname.endsWith(".apk")) {
      return serveBuild(env.BUILDS, url.pathname.slice(1), "public, max-age=31536000, immutable");
    }
    if (url.hostname.startsWith("install.") || url.hostname === "localhost") {
      return env.ASSETS.fetch(request);
    }
    return new Response("Not found.", { status: 404 });
  },
} satisfies ExportedHandler<Env>;

async function serveBuild(bucket: R2Bucket, key: string, cacheControl: string): Promise<Response> {
  const object = await bucket.get(key);
  if (object === null) return new Response("Build not found.", { status: 404 });

  const headers = new Headers({
    "Cache-Control": cacheControl,
    ETag: object.httpEtag,
    "X-Content-Type-Options": "nosniff",
  });
  object.writeHttpMetadata(headers);
  if (key.endsWith(".apk")) {
    headers.set("Content-Type", "application/vnd.android.package-archive");
    headers.set("Content-Disposition", 'attachment; filename="k-3720-internal.apk"');
  } else {
    headers.set("Content-Type", "application/json; charset=utf-8");
    headers.set("Access-Control-Allow-Origin", "*");
  }
  return new Response(object.body, { headers });
}
