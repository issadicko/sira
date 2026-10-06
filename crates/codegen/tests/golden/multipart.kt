import java.io.File
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.asRequestBody
import okhttp3.RequestBody.Companion.toRequestBody

fun main() {
    val client = OkHttpClient()

    val body = MultipartBody.Builder()
        .setType(MultipartBody.FORM)
        .addFormDataPart("title", "Photo; \"cover\"")
        .addFormDataPart("note", null, "typed".toRequestBody("text/plain".toMediaType()))
        .addFormDataPart("avatar", "a.png", File("files/a.png").asRequestBody("image/png".toMediaType()))
        .addFormDataPart("doc", "b.txt", File("files/b.txt").asRequestBody(null))
        .build()

    val request = Request.Builder()
        .url("http://127.0.0.1:8765/upload")
        .method("POST", body)
        .addHeader("X-Trace", "1")
        .build()

    client.newCall(request).execute().use { response ->
        println(response.code)
        println(response.body?.string())
    }
}
