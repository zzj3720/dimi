package org.k3720.update

import android.net.Uri
import expo.modules.kotlin.functions.Coroutine
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import java.io.File
import java.security.MessageDigest

class K3720UpdateModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("K3720Update")

    AsyncFunction("sha256") Coroutine { uri: String ->
      val path = requireNotNull(Uri.parse(uri).path) { "Update file URI has no path." }
      val digest = MessageDigest.getInstance("SHA-256")
      File(path).inputStream().buffered().use { input ->
        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
        while (true) {
          val count = input.read(buffer)
          if (count < 0) break
          digest.update(buffer, 0, count)
        }
      }
      digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xff) }
    }
  }
}
